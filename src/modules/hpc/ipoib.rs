//! HPC IPoIB (IP over InfiniBand) interface configuration module
//!
//! Manages IPoIB network interfaces for InfiniBand networks.
//! Supports creating/configuring interfaces with different modes, MTU settings, and IP addresses.
//!
//! # Parameters
//!
//! - `interface` (required): Interface name (e.g., "ib0", "ib1")
//! - `mode` (optional): IPoIB mode - "connected" or "datagram" (default: "datagram")
//! - `mtu` (optional): Maximum Transmission Unit (e.g., 65520 for connected, 2044 for datagram)
//! - `ip_address` (optional): IP address with prefix (e.g., "192.168.1.10/24")
//! - `state` (optional): Desired state - "present" or "absent" (default: "present")

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

/// IPoIB interface mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IpoibMode {
    Connected,
    Datagram,
}

impl IpoibMode {
    /// Parse a string into an IpoibMode.
    pub fn from_str(s: &str) -> Option<IpoibMode> {
        match s.to_lowercase().as_str() {
            "connected" => Some(IpoibMode::Connected),
            "datagram" => Some(IpoibMode::Datagram),
            _ => None,
        }
    }

    /// Convert to string representation for sysfs.
    pub fn to_sysfs_value(&self) -> &'static str {
        match self {
            IpoibMode::Connected => "connected",
            IpoibMode::Datagram => "datagram",
        }
    }

    /// Get default MTU for this mode.
    pub fn default_mtu(&self) -> u32 {
        match self {
            IpoibMode::Connected => 65520,
            IpoibMode::Datagram => 2044,
        }
    }
}

/// Desired state for the IPoIB interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InterfaceState {
    Present,
    Absent,
}

impl InterfaceState {
    pub fn from_str(s: &str) -> Option<InterfaceState> {
        match s.to_lowercase().as_str() {
            "present" => Some(InterfaceState::Present),
            "absent" => Some(InterfaceState::Absent),
            _ => None,
        }
    }
}

/// Current state of an IPoIB interface parsed from system commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub exists: bool,
    pub is_up: bool,
    pub mode: Option<IpoibMode>,
    pub mtu: Option<u32>,
    pub ip_address: Option<String>,
}

impl InterfaceInfo {
    /// Parse interface information from `ip link show` output.
    pub fn from_ip_link_output(output: &str) -> InterfaceInfo {
        let exists = !output.contains("does not exist");
        let is_up = output.contains("state UP");

        let mtu = output
            .split_whitespace()
            .position(|s| s == "mtu")
            .and_then(|i| output.split_whitespace().nth(i + 1))
            .and_then(|s| s.parse::<u32>().ok());

        InterfaceInfo {
            exists,
            is_up,
            mode: None,
            mtu,
            ip_address: None,
        }
    }

    /// Parse IPoIB mode from sysfs output.
    pub fn parse_mode(mode_str: &str) -> Option<IpoibMode> {
        let trimmed = mode_str.trim();
        if trimmed.contains("connected") {
            Some(IpoibMode::Connected)
        } else if trimmed.contains("datagram") {
            Some(IpoibMode::Datagram)
        } else {
            None
        }
    }

    /// Parse IP address from `ip addr show` output.
    pub fn parse_ip_address(output: &str, interface: &str) -> Option<String> {
        for line in output.lines() {
            if line.trim().starts_with("inet ") && output.contains(interface) {
                return line.trim().split_whitespace().nth(1).map(|s| s.to_string());
            }
        }
        None
    }
}

pub struct IpoibModule;

impl IpoibModule {
    /// Check if the interface exists and get its current state.
    fn get_interface_state(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        interface: &str,
    ) -> ModuleResult<InterfaceInfo> {
        let cmd = format!("ip link show {}", interface);
        let (ok, stdout, _stderr) = run_cmd(connection, &cmd, context)?;

        let mut info = if ok {
            InterfaceInfo::from_ip_link_output(&stdout)
        } else {
            InterfaceInfo {
                exists: false,
                is_up: false,
                mode: None,
                mtu: None,
                ip_address: None,
            }
        };

        // If interface exists, try to get mode from sysfs
        if info.exists {
            let mode_cmd = format!(
                "cat /sys/class/net/{}/mode 2>/dev/null || echo unknown",
                interface
            );
            if let Ok((true, mode_output, _)) = run_cmd(connection, &mode_cmd, context) {
                info.mode = InterfaceInfo::parse_mode(&mode_output);
            }

            // Get IP address
            let ip_cmd = format!("ip addr show {}", interface);
            if let Ok((true, ip_output, _)) = run_cmd(connection, &ip_cmd, context) {
                info.ip_address = InterfaceInfo::parse_ip_address(&ip_output, interface);
            }
        }

        Ok(info)
    }

    /// Check if configuration would change the interface.
    fn would_change(
        current: &InterfaceInfo,
        desired_mode: Option<IpoibMode>,
        desired_mtu: Option<u32>,
        desired_ip: Option<&str>,
    ) -> bool {
        // Check mode change
        if let Some(mode) = desired_mode {
            if current.mode != Some(mode) {
                return true;
            }
        }

        // Check MTU change
        if let Some(mtu) = desired_mtu {
            if current.mtu != Some(mtu) {
                return true;
            }
        }

        // Check IP address change
        if let Some(ip) = desired_ip {
            if current.ip_address.as_deref() != Some(ip) {
                return true;
            }
        }

        false
    }

    /// Configure the interface to the desired state (present).
    fn configure_interface(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        interface: &str,
        mode: IpoibMode,
        mtu: u32,
        ip_address: Option<&str>,
    ) -> ModuleResult<()> {
        // Bring interface up if it's not already
        let up_cmd = format!("ip link set {} up", interface);
        run_cmd_ok(connection, &up_cmd, context)?;

        // Set IPoIB mode via sysfs
        let mode_cmd = format!(
            "echo {} > /sys/class/net/{}/mode 2>/dev/null || true",
            mode.to_sysfs_value(),
            interface
        );
        let _ = run_cmd(connection, &mode_cmd, context);

        // Set MTU
        let mtu_cmd = format!("ip link set {} mtu {}", interface, mtu);
        run_cmd_ok(connection, &mtu_cmd, context)?;

        // Set IP address if provided
        if let Some(ip) = ip_address {
            // Flush existing addresses first
            let flush_cmd = format!("ip addr flush dev {}", interface);
            let _ = run_cmd(connection, &flush_cmd, context);

            // Add new address
            let ip_cmd = format!("ip addr add {} dev {}", ip, interface);
            run_cmd_ok(connection, &ip_cmd, context)?;
        }

        Ok(())
    }

    /// Remove interface configuration (absent state).
    fn remove_interface(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        interface: &str,
    ) -> ModuleResult<()> {
        // Flush IP addresses
        let flush_cmd = format!("ip addr flush dev {}", interface);
        let _ = run_cmd(connection, &flush_cmd, context);

        // Bring interface down
        let down_cmd = format!("ip link set {} down", interface);
        run_cmd_ok(connection, &down_cmd, context)?;

        Ok(())
    }
}

impl Module for IpoibModule {
    fn name(&self) -> &'static str {
        "ipoib"
    }

    fn description(&self) -> &'static str {
        "Configure IPoIB (IP over InfiniBand) network interfaces"
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

        let interface = params.get_string_required("interface")?;

        let mode_str = params
            .get_string("mode")?
            .unwrap_or_else(|| "datagram".to_string());
        let mode = IpoibMode::from_str(&mode_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid mode '{}'. Must be 'connected' or 'datagram'",
                mode_str
            ))
        })?;

        let mtu = params
            .get_i64("mtu")?
            .map(|v| v as u32)
            .unwrap_or_else(|| mode.default_mtu());

        let ip_address = params.get_string("ip_address")?;

        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = InterfaceState::from_str(&state_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'present' or 'absent'",
                state_str
            ))
        })?;

        // Get current interface state
        let current = Self::get_interface_state(connection, context, &interface)?;

        match state {
            InterfaceState::Present => {
                if !current.exists {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Interface {} does not exist. Ensure InfiniBand drivers are loaded.",
                        interface
                    )));
                }

                // Check if configuration would change
                let would_change = !current.is_up
                    || Self::would_change(&current, Some(mode), Some(mtu), ip_address.as_deref());

                if !would_change {
                    return Ok(ModuleOutput::ok(format!(
                        "Interface {} is already configured as desired",
                        interface
                    ))
                    .with_data("interface", serde_json::json!(interface))
                    .with_data("mode", serde_json::json!(mode))
                    .with_data("mtu", serde_json::json!(mtu))
                    .with_data("ip_address", serde_json::json!(ip_address)));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would configure interface {} (mode: {:?}, mtu: {}, ip: {:?})",
                        interface, mode, mtu, ip_address
                    ))
                    .with_data("interface", serde_json::json!(interface))
                    .with_data("mode", serde_json::json!(mode))
                    .with_data("mtu", serde_json::json!(mtu))
                    .with_data("ip_address", serde_json::json!(ip_address)));
                }

                Self::configure_interface(
                    connection,
                    context,
                    &interface,
                    mode,
                    mtu,
                    ip_address.as_deref(),
                )?;

                Ok(ModuleOutput::changed(format!(
                    "Configured interface {} (mode: {:?}, mtu: {}, ip: {:?})",
                    interface, mode, mtu, ip_address
                ))
                .with_data("interface", serde_json::json!(interface))
                .with_data("mode", serde_json::json!(mode))
                .with_data("mtu", serde_json::json!(mtu))
                .with_data("ip_address", serde_json::json!(ip_address)))
            }
            InterfaceState::Absent => {
                if !current.exists || (!current.is_up && current.ip_address.is_none()) {
                    return Ok(ModuleOutput::ok(format!(
                        "Interface {} is already in absent state",
                        interface
                    ))
                    .with_data("interface", serde_json::json!(interface))
                    .with_data("state", serde_json::json!("absent")));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would remove configuration from interface {}",
                        interface
                    ))
                    .with_data("interface", serde_json::json!(interface))
                    .with_data("state", serde_json::json!("absent")));
                }

                Self::remove_interface(connection, context, &interface)?;

                Ok(ModuleOutput::changed(format!(
                    "Removed configuration from interface {}",
                    interface
                ))
                .with_data("interface", serde_json::json!(interface))
                .with_data("state", serde_json::json!("absent")))
            }
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["interface"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("mode", serde_json::json!("datagram"));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipoib_mode_from_str() {
        assert_eq!(IpoibMode::from_str("connected"), Some(IpoibMode::Connected));
        assert_eq!(IpoibMode::from_str("datagram"), Some(IpoibMode::Datagram));
        assert_eq!(IpoibMode::from_str("CONNECTED"), Some(IpoibMode::Connected));
        assert_eq!(IpoibMode::from_str("DATAGRAM"), Some(IpoibMode::Datagram));
        assert_eq!(IpoibMode::from_str("invalid"), None);
        assert_eq!(IpoibMode::from_str(""), None);
    }

    #[test]
    fn test_ipoib_mode_to_sysfs_value() {
        assert_eq!(IpoibMode::Connected.to_sysfs_value(), "connected");
        assert_eq!(IpoibMode::Datagram.to_sysfs_value(), "datagram");
    }

    #[test]
    fn test_ipoib_mode_default_mtu() {
        assert_eq!(IpoibMode::Connected.default_mtu(), 65520);
        assert_eq!(IpoibMode::Datagram.default_mtu(), 2044);
    }

    #[test]
    fn test_interface_state_from_str() {
        assert_eq!(
            InterfaceState::from_str("present"),
            Some(InterfaceState::Present)
        );
        assert_eq!(
            InterfaceState::from_str("absent"),
            Some(InterfaceState::Absent)
        );
        assert_eq!(
            InterfaceState::from_str("PRESENT"),
            Some(InterfaceState::Present)
        );
        assert_eq!(InterfaceState::from_str("invalid"), None);
    }

    #[test]
    fn test_interface_info_from_ip_link_output() {
        let output = "2: ib0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 2044 qdisc mq state UP mode DEFAULT group default qlen 256";
        let info = InterfaceInfo::from_ip_link_output(output);
        assert!(info.exists);
        assert!(info.is_up);
        assert_eq!(info.mtu, Some(2044));

        let output_down = "2: ib0: <BROADCAST,MULTICAST> mtu 2044 qdisc mq state DOWN";
        let info_down = InterfaceInfo::from_ip_link_output(output_down);
        assert!(info_down.exists);
        assert!(!info_down.is_up);

        let output_missing = "Device does not exist";
        let info_missing = InterfaceInfo::from_ip_link_output(output_missing);
        assert!(!info_missing.exists);
    }

    #[test]
    fn test_interface_info_parse_mode() {
        assert_eq!(
            InterfaceInfo::parse_mode("connected\n"),
            Some(IpoibMode::Connected)
        );
        assert_eq!(
            InterfaceInfo::parse_mode("datagram\n"),
            Some(IpoibMode::Datagram)
        );
        assert_eq!(
            InterfaceInfo::parse_mode("mode: connected"),
            Some(IpoibMode::Connected)
        );
        assert_eq!(InterfaceInfo::parse_mode("unknown"), None);
    }

    #[test]
    fn test_interface_info_parse_ip_address() {
        let output = "2: ib0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 2044\n    inet 192.168.1.10/24 brd 192.168.1.255 scope global ib0";
        let ip = InterfaceInfo::parse_ip_address(output, "ib0");
        assert_eq!(ip, Some("192.168.1.10/24".to_string()));

        let output_no_ip = "2: ib0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 2044";
        let ip_none = InterfaceInfo::parse_ip_address(output_no_ip, "ib0");
        assert_eq!(ip_none, None);
    }

    #[test]
    fn test_would_change() {
        let current = InterfaceInfo {
            exists: true,
            is_up: true,
            mode: Some(IpoibMode::Datagram),
            mtu: Some(2044),
            ip_address: Some("192.168.1.10/24".to_string()),
        };

        // No change
        assert!(!IpoibModule::would_change(
            &current,
            Some(IpoibMode::Datagram),
            Some(2044),
            Some("192.168.1.10/24")
        ));

        // Mode change
        assert!(IpoibModule::would_change(
            &current,
            Some(IpoibMode::Connected),
            Some(2044),
            Some("192.168.1.10/24")
        ));

        // MTU change
        assert!(IpoibModule::would_change(
            &current,
            Some(IpoibMode::Datagram),
            Some(65520),
            Some("192.168.1.10/24")
        ));

        // IP change
        assert!(IpoibModule::would_change(
            &current,
            Some(IpoibMode::Datagram),
            Some(2044),
            Some("192.168.1.20/24")
        ));
    }

    #[test]
    fn test_module_name_and_description() {
        let module = IpoibModule;
        assert_eq!(module.name(), "ipoib");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_module_required_params() {
        let module = IpoibModule;
        let required = module.required_params();
        assert!(required.contains(&"interface"));
        assert_eq!(required.len(), 1);
    }

    #[test]
    fn test_module_optional_params() {
        let module = IpoibModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("mode"));
        assert!(optional.contains_key("state"));
        assert_eq!(
            optional.get("mode").unwrap(),
            &serde_json::json!("datagram")
        );
        assert_eq!(
            optional.get("state").unwrap(),
            &serde_json::json!("present")
        );
    }

    #[test]
    fn test_ipoib_mode_serde() {
        let mode = IpoibMode::Connected;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"connected\"");

        let parsed: IpoibMode = serde_json::from_str("\"datagram\"").unwrap();
        assert_eq!(parsed, IpoibMode::Datagram);
    }

    #[test]
    fn test_interface_state_serde() {
        let state = InterfaceState::Present;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"present\"");

        let parsed: InterfaceState = serde_json::from_str("\"absent\"").unwrap();
        assert_eq!(parsed, InterfaceState::Absent);
    }
}
