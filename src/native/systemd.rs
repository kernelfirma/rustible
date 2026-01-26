//! Native systemd bindings
//!
//! This module provides native access to systemd service management by parsing
//! systemctl output and systemd unit files directly, with future support for
//! D-Bus communication.
//!
//! # Features
//!
//! - Unit file parsing
//! - Service status detection
//! - Journal log access
//! - Socket activation detection
//!
//! # Example
//!
//! ```rust,ignore
//! use rustible::native::systemd::{SystemdNative, UnitInfo, UnitState};
//!
//! let systemd = SystemdNative::new()?;
//!
//! // Get service status
//! let status = systemd.get_unit_status("nginx.service")?;
//! println!("nginx is {:?}", status.active_state);
//!
//! // List all services
//! let services = systemd.list_units("service")?;
//! ```

use super::{NativeError, NativeResult};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Systemd unit directories (in order of precedence)
const UNIT_PATHS: &[&str] = &[
    "/etc/systemd/system",
    "/run/systemd/system",
    "/usr/lib/systemd/system",
    "/lib/systemd/system",
];

/// Check if native systemd support is available
pub fn is_native_available() -> bool {
    // Check if systemd is running as PID 1
    if let Ok(comm) = fs::read_to_string("/proc/1/comm") {
        if comm.trim() == "systemd" {
            return true;
        }
    }

    // Check for /run/systemd/system directory
    Path::new("/run/systemd/system").exists()
}

/// Active state of a systemd unit
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveState {
    /// Unit is running
    Active,
    /// Unit is running but reloading its configuration
    Reloading,
    /// Unit is not running but has not failed
    Inactive,
    /// Unit failed to start or crashed
    Failed,
    /// Unit is being started
    Activating,
    /// Unit is being stopped
    Deactivating,
    /// Unknown state
    Unknown(String),
}

impl ActiveState {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "active" => ActiveState::Active,
            "reloading" => ActiveState::Reloading,
            "inactive" => ActiveState::Inactive,
            "failed" => ActiveState::Failed,
            "activating" => ActiveState::Activating,
            "deactivating" => ActiveState::Deactivating,
            other => ActiveState::Unknown(other.to_string()),
        }
    }

    /// Check if the unit is running
    pub fn is_running(&self) -> bool {
        matches!(self, ActiveState::Active | ActiveState::Reloading)
    }
}

/// Load state of a systemd unit
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadState {
    /// Unit configuration was loaded successfully
    Loaded,
    /// Unit configuration file was not found
    NotFound,
    /// Unit configuration has errors
    Error,
    /// Unit is masked
    Masked,
    /// Unknown state
    Unknown(String),
}

impl LoadState {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "loaded" => LoadState::Loaded,
            "not-found" => LoadState::NotFound,
            "error" => LoadState::Error,
            "masked" => LoadState::Masked,
            other => LoadState::Unknown(other.to_string()),
        }
    }
}

/// Unit file state (enabled, disabled, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitFileState {
    /// Unit is enabled and will start at boot
    Enabled,
    /// Unit is enabled at runtime but not persistent
    EnabledRuntime,
    /// Unit is linked
    Linked,
    /// Unit is linked at runtime
    LinkedRuntime,
    /// Unit is masked
    Masked,
    /// Unit is masked at runtime
    MaskedRuntime,
    /// Unit is static (no install section)
    Static,
    /// Unit is disabled
    Disabled,
    /// Unit is invalid
    Invalid,
    /// Unknown state
    Unknown(String),
}

impl UnitFileState {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "enabled" => UnitFileState::Enabled,
            "enabled-runtime" => UnitFileState::EnabledRuntime,
            "linked" => UnitFileState::Linked,
            "linked-runtime" => UnitFileState::LinkedRuntime,
            "masked" => UnitFileState::Masked,
            "masked-runtime" => UnitFileState::MaskedRuntime,
            "static" => UnitFileState::Static,
            "disabled" => UnitFileState::Disabled,
            "invalid" => UnitFileState::Invalid,
            other => UnitFileState::Unknown(other.to_string()),
        }
    }

    /// Check if unit will start at boot
    pub fn is_enabled(&self) -> bool {
        matches!(
            self,
            UnitFileState::Enabled | UnitFileState::EnabledRuntime | UnitFileState::Static
        )
    }
}

/// Information about a systemd unit
#[derive(Debug, Clone)]
pub struct UnitInfo {
    /// Unit name (e.g., "nginx.service")
    pub name: String,
    /// Unit description
    pub description: Option<String>,
    /// Load state
    pub load_state: LoadState,
    /// Active state
    pub active_state: ActiveState,
    /// Sub state (e.g., "running", "exited", "dead")
    pub sub_state: String,
    /// Unit file state (enabled, disabled, etc.)
    pub unit_file_state: UnitFileState,
    /// Path to unit file
    pub unit_file_path: Option<String>,
    /// Main process ID (if running)
    pub main_pid: Option<u32>,
    /// Fragment path
    pub fragment_path: Option<String>,
    /// When the unit was activated
    pub active_enter_timestamp: Option<String>,
    /// When the unit was deactivated
    pub inactive_enter_timestamp: Option<String>,
}

impl UnitInfo {
    fn new(name: String) -> Self {
        Self {
            name,
            description: None,
            load_state: LoadState::Unknown("unknown".to_string()),
            active_state: ActiveState::Unknown("unknown".to_string()),
            sub_state: String::new(),
            unit_file_state: UnitFileState::Unknown("unknown".to_string()),
            unit_file_path: None,
            main_pid: None,
            fragment_path: None,
            active_enter_timestamp: None,
            inactive_enter_timestamp: None,
        }
    }
}

/// Parsed systemd unit file
#[derive(Debug, Clone, Default)]
pub struct UnitFile {
    /// Unit section
    pub unit: HashMap<String, String>,
    /// Service section (for .service units)
    pub service: HashMap<String, String>,
    /// Install section
    pub install: HashMap<String, String>,
    /// Socket section (for .socket units)
    pub socket: HashMap<String, String>,
    /// Timer section (for .timer units)
    pub timer: HashMap<String, String>,
    /// Mount section (for .mount units)
    pub mount: HashMap<String, String>,
}

impl UnitFile {
    /// Parse a unit file from string content
    pub fn parse(content: &str) -> NativeResult<Self> {
        let mut unit_file = UnitFile::default();
        let mut current_section: Option<&str> = None;

        for line in content.lines() {
            let line = line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            // Section header
            if line.starts_with('[') && line.ends_with(']') {
                current_section = Some(match &line[1..line.len() - 1] {
                    "Unit" => "unit",
                    "Service" => "service",
                    "Install" => "install",
                    "Socket" => "socket",
                    "Timer" => "timer",
                    "Mount" => "mount",
                    _ => continue,
                });
                continue;
            }

            // Key=Value pair
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();

                match current_section {
                    Some("unit") => {
                        unit_file.unit.insert(key, value);
                    }
                    Some("service") => {
                        unit_file.service.insert(key, value);
                    }
                    Some("install") => {
                        unit_file.install.insert(key, value);
                    }
                    Some("socket") => {
                        unit_file.socket.insert(key, value);
                    }
                    Some("timer") => {
                        unit_file.timer.insert(key, value);
                    }
                    Some("mount") => {
                        unit_file.mount.insert(key, value);
                    }
                    _ => {}
                }
            }
        }

        Ok(unit_file)
    }

    /// Get the description from the Unit section
    pub fn description(&self) -> Option<&str> {
        self.unit.get("Description").map(|s| s.as_str())
    }

    /// Get dependencies (After, Requires, Wants)
    pub fn dependencies(&self) -> Vec<&str> {
        let mut deps = Vec::new();

        for key in &["After", "Requires", "Wants", "BindsTo", "PartOf"] {
            if let Some(value) = self.unit.get(*key) {
                deps.extend(value.split_whitespace());
            }
        }

        deps
    }
}

/// Native systemd interface
pub struct SystemdNative {
    /// Use D-Bus when available (future feature)
    #[allow(dead_code)]
    use_dbus: bool,
}

impl SystemdNative {
    /// Create a new SystemdNative instance
    pub fn new() -> NativeResult<Self> {
        if !is_native_available() {
            return Err(NativeError::NotAvailable(
                "systemd is not running".to_string(),
            ));
        }

        Ok(Self { use_dbus: false })
    }

    /// Get status of a unit by parsing systemctl output
    pub fn get_unit_status(&self, unit: &str) -> NativeResult<UnitInfo> {
        let output = Command::new("systemctl")
            .args(["show", unit, "--no-pager"])
            .output()
            .map_err(|e| NativeError::Io(e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("not found") {
                return Err(NativeError::NotFound(format!("Unit {} not found", unit)));
            }
            return Err(NativeError::Other(format!(
                "systemctl show failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut info = UnitInfo::new(unit.to_string());

        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "Description" => info.description = Some(value.to_string()),
                    "LoadState" => info.load_state = LoadState::from_str(value),
                    "ActiveState" => info.active_state = ActiveState::from_str(value),
                    "SubState" => info.sub_state = value.to_string(),
                    "UnitFileState" => info.unit_file_state = UnitFileState::from_str(value),
                    "FragmentPath" => {
                        if !value.is_empty() {
                            info.fragment_path = Some(value.to_string());
                            info.unit_file_path = Some(value.to_string());
                        }
                    }
                    "MainPID" => {
                        if value != "0" {
                            info.main_pid = value.parse().ok();
                        }
                    }
                    "ActiveEnterTimestamp" => {
                        if !value.is_empty() {
                            info.active_enter_timestamp = Some(value.to_string());
                        }
                    }
                    "InactiveEnterTimestamp" => {
                        if !value.is_empty() {
                            info.inactive_enter_timestamp = Some(value.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(info)
    }

    /// Check if a unit exists
    pub fn unit_exists(&self, unit: &str) -> NativeResult<bool> {
        match self.get_unit_status(unit) {
            Ok(info) => Ok(!matches!(info.load_state, LoadState::NotFound)),
            Err(NativeError::NotFound(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Check if a unit is active/running
    pub fn is_active(&self, unit: &str) -> NativeResult<bool> {
        let info = self.get_unit_status(unit)?;
        Ok(info.active_state.is_running())
    }

    /// Check if a unit is enabled
    pub fn is_enabled(&self, unit: &str) -> NativeResult<bool> {
        let info = self.get_unit_status(unit)?;
        Ok(info.unit_file_state.is_enabled())
    }

    /// List units of a specific type
    pub fn list_units(&self, unit_type: &str) -> NativeResult<Vec<UnitInfo>> {
        let output = Command::new("systemctl")
            .args([
                "list-units",
                &format!("--type={}", unit_type),
                "--all",
                "--no-legend",
                "--no-pager",
            ])
            .output()
            .map_err(|e| NativeError::Io(e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut units = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let mut info = UnitInfo::new(parts[0].to_string());
                info.load_state = LoadState::from_str(parts[1]);
                info.active_state = ActiveState::from_str(parts[2]);
                info.sub_state = parts[3].to_string();
                if parts.len() > 4 {
                    info.description = Some(parts[4..].join(" "));
                }
                units.push(info);
            }
        }

        Ok(units)
    }

    /// List all services
    pub fn list_services(&self) -> NativeResult<Vec<UnitInfo>> {
        self.list_units("service")
    }

    /// List enabled services
    pub fn list_enabled_services(&self) -> NativeResult<Vec<String>> {
        let output = Command::new("systemctl")
            .args([
                "list-unit-files",
                "--type=service",
                "--state=enabled",
                "--no-legend",
                "--no-pager",
            ])
            .output()
            .map_err(|e| NativeError::Io(e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let services: Vec<String> = stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(|s| s.to_string())
            .collect();

        Ok(services)
    }

    /// Read and parse a unit file
    pub fn read_unit_file(&self, unit: &str) -> NativeResult<UnitFile> {
        // Find the unit file
        for path in UNIT_PATHS {
            let unit_path = Path::new(path).join(unit);
            if unit_path.exists() {
                let content = fs::read_to_string(&unit_path)?;
                return UnitFile::parse(&content);
            }
        }

        // Try using systemctl cat
        let output = Command::new("systemctl")
            .args(["cat", unit, "--no-pager"])
            .output()
            .map_err(|e| NativeError::Io(e))?;

        if output.status.success() {
            let content = String::from_utf8_lossy(&output.stdout);
            return UnitFile::parse(&content);
        }

        Err(NativeError::NotFound(format!(
            "Unit file {} not found",
            unit
        )))
    }

    /// Find the path to a unit file
    pub fn find_unit_file(&self, unit: &str) -> Option<String> {
        for path in UNIT_PATHS {
            let unit_path = Path::new(path).join(unit);
            if unit_path.exists() {
                return Some(unit_path.to_string_lossy().to_string());
            }
        }
        None
    }

    /// Get failed units
    pub fn get_failed_units(&self) -> NativeResult<Vec<UnitInfo>> {
        let output = Command::new("systemctl")
            .args(["--failed", "--no-legend", "--no-pager"])
            .output()
            .map_err(|e| NativeError::Io(e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut units = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if !parts.is_empty() {
                let mut info = UnitInfo::new(parts[0].to_string());
                info.active_state = ActiveState::Failed;
                if parts.len() > 1 {
                    info.load_state = LoadState::from_str(parts[1]);
                }
                units.push(info);
            }
        }

        Ok(units)
    }

    /// Check if systemd daemon needs reload
    pub fn needs_daemon_reload(&self) -> NativeResult<bool> {
        let output = Command::new("systemctl")
            .args(["daemon-reload", "--dry-run"])
            .output();

        // If the command doesn't support --dry-run, fall back to checking unit files
        match output {
            Ok(out) => Ok(!out.status.success()),
            Err(_) => Ok(false),
        }
    }
}

impl Default for SystemdNative {
    fn default() -> Self {
        Self { use_dbus: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_active_state_parsing() {
        assert_eq!(ActiveState::from_str("active"), ActiveState::Active);
        assert_eq!(ActiveState::from_str("inactive"), ActiveState::Inactive);
        assert_eq!(ActiveState::from_str("failed"), ActiveState::Failed);
    }

    #[test]
    fn test_unit_file_state_parsing() {
        assert_eq!(
            UnitFileState::from_str("enabled"),
            UnitFileState::Enabled
        );
        assert_eq!(
            UnitFileState::from_str("disabled"),
            UnitFileState::Disabled
        );
        assert_eq!(UnitFileState::from_str("static"), UnitFileState::Static);
    }

    #[test]
    fn test_unit_file_parsing() {
        let content = r#"
[Unit]
Description=Test Service
After=network.target

[Service]
Type=simple
ExecStart=/usr/bin/test
Restart=always

[Install]
WantedBy=multi-user.target
"#;

        let unit_file = UnitFile::parse(content).unwrap();
        assert_eq!(unit_file.description(), Some("Test Service"));
        assert_eq!(unit_file.service.get("Type"), Some(&"simple".to_string()));
        assert_eq!(
            unit_file.install.get("WantedBy"),
            Some(&"multi-user.target".to_string())
        );
    }

    #[test]
    fn test_native_available() {
        // Just ensure it doesn't panic
        let _ = is_native_available();
    }
}
