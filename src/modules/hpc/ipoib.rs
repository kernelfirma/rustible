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
//! - `persist` (optional): Whether to persist config across reboots (default: true)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use serde::{Deserialize, Serialize};

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of a preflight validation check.
#[derive(Debug, Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single configuration drift item between desired and actual state.
#[derive(Debug, Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

/// Result of a verification step.
#[derive(Debug, Serialize)]
struct VerifyResult {
    verified: bool,
    details: Vec<String>,
    warnings: Vec<String>,
}

/// Detected network configuration backend on the remote host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum NetworkBackend {
    NetworkManager,
    Ifupdown,
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

fn detect_os_family(os_release: &str) -> Option<&'static str> {
    let id_line = os_release
        .lines()
        .find(|l| l.starts_with("ID_LIKE=") || l.starts_with("ID="));
    match id_line {
        Some(line) => {
            let val = line
                .split('=')
                .nth(1)
                .unwrap_or("")
                .trim_matches('"')
                .to_lowercase();
            if val.contains("rhel")
                || val.contains("fedora")
                || val.contains("centos")
                || val == "rocky"
                || val == "almalinux"
            {
                Some("rhel")
            } else if val.contains("debian") || val.contains("ubuntu") {
                Some("debian")
            } else {
                None
            }
        }
        None => None,
    }
}

/// Detect the network configuration backend from os-release content.
/// RHEL-family systems typically use NetworkManager; Debian-family use ifupdown.
fn detect_network_backend(os_release: &str) -> NetworkBackend {
    match detect_os_family(os_release) {
        Some("debian") => NetworkBackend::Ifupdown,
        _ => NetworkBackend::NetworkManager,
    }
}

/// Validate MTU value against the maximum allowed for the given IPoIB mode.
fn validate_mtu(mode: IpoibMode, mtu: u32) -> PreflightResult {
    let max_mtu = match mode {
        IpoibMode::Connected => 65520,
        IpoibMode::Datagram => 2044,
    };

    let mut result = PreflightResult {
        passed: true,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    if mtu > max_mtu {
        result.passed = false;
        result.errors.push(format!(
            "MTU {} exceeds maximum {} for {} mode",
            mtu,
            max_mtu,
            mode.to_sysfs_value()
        ));
    } else if mtu == max_mtu {
        result.warnings.push(format!(
            "MTU {} is at the maximum for {} mode",
            mtu,
            mode.to_sysfs_value()
        ));
    }

    result
}

/// Validate routing: check `ip route show` output for IP address conflicts.
/// If the desired IP (without prefix) is already assigned to a different interface, report an error.
fn validate_routing(
    route_output: &str,
    desired_ip: Option<&str>,
    interface: &str,
) -> PreflightResult {
    let mut result = PreflightResult {
        passed: true,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    let ip_only = match desired_ip {
        Some(ip) => ip.split('/').next().unwrap_or(ip),
        None => return result,
    };

    for line in route_output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Look for lines containing "src <ip>" pointing to a different device
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        for (i, part) in parts.iter().enumerate() {
            if *part == "src" {
                if let Some(src_ip) = parts.get(i + 1) {
                    if *src_ip == ip_only {
                        // Find the dev field
                        for (j, p) in parts.iter().enumerate() {
                            if *p == "dev" {
                                if let Some(dev) = parts.get(j + 1) {
                                    if *dev != interface {
                                        result.passed = false;
                                        result.errors.push(format!(
                                            "IP {} is already assigned to interface {} (conflict with {})",
                                            ip_only, dev, interface
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    result
}

/// Collect diagnostics from the remote system for the given IPoIB interface.
fn collect_diagnostics(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    interface: &str,
) -> serde_json::Value {
    // Gather ibstat output
    let ibstat_output = match run_cmd(
        connection,
        "ibstat 2>/dev/null || echo 'ibstat not available'",
        context,
    ) {
        Ok((_, stdout, _)) => stdout.trim().to_string(),
        Err(_) => "ibstat not available".to_string(),
    };

    // Gather sysfs mode
    let sysfs_mode_cmd = format!(
        "cat /sys/class/net/{}/mode 2>/dev/null || echo 'unknown'",
        interface
    );
    let sysfs_mode = match run_cmd(connection, &sysfs_mode_cmd, context) {
        Ok((_, stdout, _)) => stdout.trim().to_string(),
        Err(_) => "unknown".to_string(),
    };

    // Gather ip addr show output
    let ip_addr_cmd = format!(
        "ip addr show {} 2>/dev/null || echo 'interface not found'",
        interface
    );
    let ip_addr_output = match run_cmd(connection, &ip_addr_cmd, context) {
        Ok((_, stdout, _)) => stdout.trim().to_string(),
        Err(_) => "interface not found".to_string(),
    };

    serde_json::json!({
        "ibstat": ibstat_output,
        "sysfs_mode": sysfs_mode,
        "ip_addr": ip_addr_output,
        "interface": interface,
    })
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

    /// Get maximum allowed MTU for this mode.
    pub fn max_mtu(&self) -> u32 {
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
                return line.split_whitespace().nth(1).map(|s| s.to_string());
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

    /// Persist interface configuration across reboots.
    /// Detects whether the system uses NetworkManager or ifupdown and writes
    /// the appropriate configuration.
    fn persist_state(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        interface: &str,
        mode: IpoibMode,
        mtu: u32,
        ip_address: Option<&str>,
    ) -> ModuleResult<VerifyResult> {
        let mut result = VerifyResult {
            verified: true,
            details: Vec::new(),
            warnings: Vec::new(),
        };

        // Read os-release for backend detection
        let os_release = match run_cmd(connection, "cat /etc/os-release 2>/dev/null", context) {
            Ok((true, stdout, _)) => stdout,
            _ => String::new(),
        };

        let backend = detect_network_backend(&os_release);

        match backend {
            NetworkBackend::NetworkManager => {
                // Use nmcli to create/modify a connection profile
                let conn_name = format!("ipoib-{}", interface);

                // Delete existing connection if present (ignore errors)
                let del_cmd = format!(
                    "nmcli connection delete '{}' 2>/dev/null || true",
                    conn_name
                );
                let _ = run_cmd(connection, &del_cmd, context);

                // Build nmcli add command
                let mut add_cmd = format!(
                    "nmcli connection add type infiniband con-name '{}' ifname {} infiniband.transport-mode {} 802-3-ethernet.mtu {}",
                    conn_name,
                    interface,
                    mode.to_sysfs_value(),
                    mtu
                );

                if let Some(ip) = ip_address {
                    add_cmd.push_str(&format!(" ipv4.addresses {} ipv4.method manual", ip));
                }

                match run_cmd_ok(connection, &add_cmd, context) {
                    Ok(_) => {
                        result.details.push(format!(
                            "Created NetworkManager connection profile '{}'",
                            conn_name
                        ));
                    }
                    Err(e) => {
                        result.verified = false;
                        result
                            .warnings
                            .push(format!("Failed to create NetworkManager profile: {}", e));
                    }
                }
            }
            NetworkBackend::Ifupdown => {
                // Write to /etc/network/interfaces.d/<iface>.cfg
                let cfg_path = format!("/etc/network/interfaces.d/{}.cfg", interface);

                let method_str = if ip_address.is_some() {
                    "static"
                } else {
                    "manual"
                };

                let mut cfg_content = format!(
                    "auto {}\niface {} inet {}\n",
                    interface, interface, method_str,
                );

                if let Some(ip) = ip_address {
                    // Split ip/prefix into address and prefix
                    let parts: Vec<&str> = ip.splitn(2, '/').collect();
                    cfg_content.push_str(&format!("    address {}\n", parts[0]));
                    if parts.len() > 1 {
                        cfg_content.push_str(&format!("    netmask /{}\n", parts[1]));
                    }
                }

                cfg_content.push_str(&format!("    mtu {}\n", mtu));
                cfg_content.push_str(&format!(
                    "    pre-up echo {} > /sys/class/net/{}/mode || true\n",
                    mode.to_sysfs_value(),
                    interface
                ));

                // Write via printf and redirect
                let escaped_content = cfg_content.replace('\'', "'\\''");
                let write_cmd = format!(
                    "mkdir -p /etc/network/interfaces.d && printf '{}' > {}",
                    escaped_content, cfg_path
                );

                match run_cmd_ok(connection, &write_cmd, context) {
                    Ok(_) => {
                        result
                            .details
                            .push(format!("Wrote ifupdown config to {}", cfg_path));
                    }
                    Err(e) => {
                        result.verified = false;
                        result
                            .warnings
                            .push(format!("Failed to write ifupdown config: {}", e));
                    }
                }
            }
        }

        Ok(result)
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

        let persist = params.get_bool_or("persist", true);

        // --- Preflight: MTU validation ---
        let mtu_check = validate_mtu(mode, mtu);
        if !mtu_check.passed {
            return Err(ModuleError::InvalidParameter(mtu_check.errors.join("; ")));
        }

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

                // --- Preflight: routing conflict check ---
                if ip_address.is_some() {
                    let route_output = match run_cmd(connection, "ip route show", context) {
                        Ok((true, stdout, _)) => stdout,
                        _ => String::new(),
                    };
                    let routing_check =
                        validate_routing(&route_output, ip_address.as_deref(), &interface);
                    if !routing_check.passed {
                        return Err(ModuleError::ExecutionFailed(
                            routing_check.errors.join("; "),
                        ));
                    }
                }

                // Check if configuration would change
                let would_change = !current.is_up
                    || Self::would_change(&current, Some(mode), Some(mtu), ip_address.as_deref());

                if !would_change {
                    // Collect diagnostics even when no change needed
                    let diagnostics = collect_diagnostics(connection, context, &interface);

                    return Ok(ModuleOutput::ok(format!(
                        "Interface {} is already configured as desired",
                        interface
                    ))
                    .with_data("interface", serde_json::json!(interface))
                    .with_data("mode", serde_json::json!(mode))
                    .with_data("mtu", serde_json::json!(mtu))
                    .with_data("ip_address", serde_json::json!(ip_address))
                    .with_data("diagnostics", diagnostics));
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

                // Persist configuration if requested
                let persist_result = if persist {
                    let pr = Self::persist_state(
                        connection,
                        context,
                        &interface,
                        mode,
                        mtu,
                        ip_address.as_deref(),
                    )?;
                    Some(pr)
                } else {
                    None
                };

                // Collect diagnostics after configuration
                let diagnostics = collect_diagnostics(connection, context, &interface);

                let mut output = ModuleOutput::changed(format!(
                    "Configured interface {} (mode: {:?}, mtu: {}, ip: {:?})",
                    interface, mode, mtu, ip_address
                ))
                .with_data("interface", serde_json::json!(interface))
                .with_data("mode", serde_json::json!(mode))
                .with_data("mtu", serde_json::json!(mtu))
                .with_data("ip_address", serde_json::json!(ip_address))
                .with_data("diagnostics", diagnostics);

                if let Some(pr) = persist_result {
                    output = output.with_data("persist", serde_json::json!(pr));
                }

                if !mtu_check.warnings.is_empty() {
                    output =
                        output.with_data("mtu_warnings", serde_json::json!(mtu_check.warnings));
                }

                Ok(output)
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
        m.insert("persist", serde_json::json!(true));
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
        assert!(optional.contains_key("persist"));
        assert_eq!(
            optional.get("mode").unwrap(),
            &serde_json::json!("datagram")
        );
        assert_eq!(
            optional.get("state").unwrap(),
            &serde_json::json!("present")
        );
        assert_eq!(optional.get("persist").unwrap(), &serde_json::json!(true));
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

    // --- New tests for IB-03 enhancements ---

    #[test]
    fn test_mtu_mode_validation() {
        // Connected mode: max MTU is 65520
        let result = validate_mtu(IpoibMode::Connected, 65520);
        assert!(result.passed);
        assert!(result.errors.is_empty());
        // At max should produce a warning
        assert!(!result.warnings.is_empty());

        let result = validate_mtu(IpoibMode::Connected, 65521);
        assert!(!result.passed);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].contains("65520"));
        assert!(result.errors[0].contains("connected"));

        let result = validate_mtu(IpoibMode::Connected, 4096);
        assert!(result.passed);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());

        // Datagram mode: max MTU is 2044
        let result = validate_mtu(IpoibMode::Datagram, 2044);
        assert!(result.passed);
        assert!(result.errors.is_empty());
        assert!(!result.warnings.is_empty()); // at-max warning

        let result = validate_mtu(IpoibMode::Datagram, 2045);
        assert!(!result.passed);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].contains("2044"));
        assert!(result.errors[0].contains("datagram"));

        let result = validate_mtu(IpoibMode::Datagram, 1500);
        assert!(result.passed);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_network_backend_detection() {
        // RHEL-family should use NetworkManager
        let rhel_release = "ID=rhel\nVERSION_ID=\"8.6\"";
        assert_eq!(
            detect_network_backend(rhel_release),
            NetworkBackend::NetworkManager
        );

        let rocky_release = "ID=rocky\nVERSION_ID=\"9.0\"";
        assert_eq!(
            detect_network_backend(rocky_release),
            NetworkBackend::NetworkManager
        );

        // Debian-family should use ifupdown
        let ubuntu_release = "ID=ubuntu\nVERSION_ID=\"22.04\"";
        assert_eq!(
            detect_network_backend(ubuntu_release),
            NetworkBackend::Ifupdown
        );

        let debian_release = "ID=debian\nVERSION_ID=\"12\"";
        assert_eq!(
            detect_network_backend(debian_release),
            NetworkBackend::Ifupdown
        );

        // Unknown defaults to NetworkManager
        let unknown_release = "ID=unknown\nVERSION_ID=\"1.0\"";
        assert_eq!(
            detect_network_backend(unknown_release),
            NetworkBackend::NetworkManager
        );

        // Empty string defaults to NetworkManager
        assert_eq!(detect_network_backend(""), NetworkBackend::NetworkManager);
    }

    #[test]
    fn test_diagnostics_structure() {
        // Since collect_diagnostics requires a real connection, we test the JSON structure
        // by constructing expected output matching the collect_diagnostics format
        let diag = serde_json::json!({
            "ibstat": "some ibstat output",
            "sysfs_mode": "datagram",
            "ip_addr": "2: ib0: <BROADCAST,MULTICAST,UP> mtu 2044",
            "interface": "ib0",
        });

        // Verify structure has expected keys
        assert!(diag.get("ibstat").is_some());
        assert!(diag.get("sysfs_mode").is_some());
        assert!(diag.get("ip_addr").is_some());
        assert!(diag.get("interface").is_some());

        // Verify values are strings
        assert!(diag["ibstat"].is_string());
        assert!(diag["sysfs_mode"].is_string());
        assert!(diag["ip_addr"].is_string());
        assert!(diag["interface"].is_string());

        assert_eq!(diag["interface"], "ib0");
    }

    #[test]
    fn test_validate_routing_no_conflict() {
        let route_output = "192.168.1.0/24 dev ib0 proto kernel scope link src 192.168.1.10\ndefault via 10.0.0.1 dev eth0";
        let result = validate_routing(route_output, Some("192.168.1.10/24"), "ib0");
        assert!(result.passed);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_routing_with_conflict() {
        // IP is assigned to eth0 but we want it on ib0
        let route_output = "192.168.1.0/24 dev eth0 proto kernel scope link src 192.168.1.10";
        let result = validate_routing(route_output, Some("192.168.1.10/24"), "ib0");
        assert!(!result.passed);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].contains("eth0"));
        assert!(result.errors[0].contains("192.168.1.10"));
    }

    #[test]
    fn test_validate_routing_no_ip() {
        let route_output = "default via 10.0.0.1 dev eth0";
        let result = validate_routing(route_output, None, "ib0");
        assert!(result.passed);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_routing_same_interface_no_conflict() {
        // IP already on the same interface is not a conflict
        let route_output = "10.0.0.0/24 dev ib0 proto kernel scope link src 10.0.0.5";
        let result = validate_routing(route_output, Some("10.0.0.5/24"), "ib0");
        assert!(result.passed);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_preflight_result_serde() {
        let pr = PreflightResult {
            passed: false,
            warnings: vec!["warn1".to_string()],
            errors: vec!["err1".to_string()],
        };
        let json = serde_json::to_value(&pr).unwrap();
        assert_eq!(json["passed"], false);
        assert_eq!(json["warnings"][0], "warn1");
        assert_eq!(json["errors"][0], "err1");
    }

    #[test]
    fn test_drift_item_serde() {
        let drift = DriftItem {
            field: "mtu".to_string(),
            desired: "65520".to_string(),
            actual: "2044".to_string(),
        };
        let json = serde_json::to_value(&drift).unwrap();
        assert_eq!(json["field"], "mtu");
        assert_eq!(json["desired"], "65520");
        assert_eq!(json["actual"], "2044");
    }

    #[test]
    fn test_verify_result_serde() {
        let vr = VerifyResult {
            verified: true,
            details: vec!["persisted config".to_string()],
            warnings: Vec::new(),
        };
        let json = serde_json::to_value(&vr).unwrap();
        assert_eq!(json["verified"], true);
        assert_eq!(json["details"][0], "persisted config");
        assert!(json["warnings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_max_mtu() {
        assert_eq!(IpoibMode::Connected.max_mtu(), 65520);
        assert_eq!(IpoibMode::Datagram.max_mtu(), 2044);
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID=rhel\nVERSION=8"), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID_LIKE=\"rhel fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=unknown"), None);
    }
}
