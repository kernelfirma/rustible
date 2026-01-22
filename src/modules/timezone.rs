//! Timezone module - System timezone and time synchronization management
//!
//! This module manages system timezone, NTP configuration, and hardware clock
//! settings using timedatectl (systemd) or traditional methods.
//!
//! ## Features
//!
//! - **Timezone setting**: Set system timezone via timedatectl or /etc/localtime
//! - **NTP configuration**: Enable/disable NTP synchronization
//! - **Hardware clock management**: Configure RTC (Real-Time Clock) settings
//! - **timedatectl integration**: Full support for systemd-timedated
//!
//! ## Parameters
//!
//! - `name`: Timezone name (e.g., "America/New_York", "UTC") - required
//! - `hwclock`: Hardware clock mode ("UTC" or "local")
//! - `ntp`: Whether to enable NTP synchronization (boolean)
//! - `use`: Strategy for setting timezone ("timedatectl", "file", "auto")
//!
//! ## Examples
//!
//! ```yaml
//! # Set timezone to UTC
//! - timezone:
//!     name: UTC
//!
//! # Set timezone with NTP enabled
//! - timezone:
//!     name: America/New_York
//!     ntp: yes
//!
//! # Configure hardware clock to local time (for Windows dual-boot)
//! - timezone:
//!     name: Europe/London
//!     hwclock: local
//! ```

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Regex pattern for validating timezone names
/// Format: Area/Location or UTC/GMT variants
static TIMEZONE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^([A-Za-z]+(/[A-Za-z0-9_+-]+)+|UTC|GMT([+-]\d{1,2})?|Etc/[A-Za-z0-9_+-]+|posix/[A-Za-z0-9_/+-]+|right/[A-Za-z0-9_/+-]+)$")
        .expect("Invalid timezone regex")
});

/// Strategy for setting timezone
#[derive(Debug, Clone, PartialEq)]
pub enum TimezoneStrategy {
    /// Use timedatectl (systemd)
    Timedatectl,
    /// Use traditional file-based method (/etc/localtime)
    File,
    /// Detect automatically
    Auto,
}

impl TimezoneStrategy {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "timedatectl" | "systemd" => Ok(TimezoneStrategy::Timedatectl),
            "file" => Ok(TimezoneStrategy::File),
            "auto" => Ok(TimezoneStrategy::Auto),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid use '{}'. Valid values: timedatectl, file, auto",
                s
            ))),
        }
    }
}

impl std::str::FromStr for TimezoneStrategy {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TimezoneStrategy::from_str(s)
    }
}

/// Hardware clock mode
#[derive(Debug, Clone, PartialEq)]
pub enum HwclockMode {
    /// Hardware clock is set to UTC
    Utc,
    /// Hardware clock is set to local time
    Local,
}

impl HwclockMode {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "utc" => Ok(HwclockMode::Utc),
            "local" | "localtime" => Ok(HwclockMode::Local),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid hwclock '{}'. Valid values: UTC, local",
                s
            ))),
        }
    }
}

impl std::str::FromStr for HwclockMode {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        HwclockMode::from_str(s)
    }
}

/// Module for timezone management
pub struct TimezoneModule;

impl TimezoneModule {
    /// Get execution options with become support if needed
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

    /// Execute a command via connection
    fn execute_command(
        connection: &Arc<dyn Connection + Send + Sync>,
        command: &str,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String, String)> {
        let options = Self::get_exec_options(context);

        let result = Handle::current()
            .block_on(async { connection.execute(command, Some(options)).await })
            .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Detect available timezone strategy
    fn detect_strategy(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<TimezoneStrategy> {
        // Check for timedatectl and running systemd
        let cmd = "command -v timedatectl >/dev/null 2>&1 && test -d /run/systemd/system && echo yes || echo no";
        let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;

        if stdout.trim() == "yes" {
            return Ok(TimezoneStrategy::Timedatectl);
        }

        Ok(TimezoneStrategy::File)
    }

    /// Get current timezone using timedatectl
    fn get_timezone_timedatectl(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let cmd = "timedatectl show --property=Timezone --value 2>/dev/null || timedatectl status | grep 'Time zone' | awk -F': ' '{print $2}' | awk '{print $1}'";
        let (success, stdout, stderr) = Self::execute_command(connection, cmd, context)?;

        if success && !stdout.trim().is_empty() {
            Ok(stdout.trim().to_string())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to get timezone via timedatectl: {}",
                stderr
            )))
        }
    }

    /// Get current timezone by reading /etc/localtime symlink
    fn get_timezone_file(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        // Try reading symlink first
        let cmd = "readlink -f /etc/localtime 2>/dev/null | sed 's|.*/zoneinfo/||'";
        let (success, stdout, _) = Self::execute_command(connection, cmd, context)?;

        if success && !stdout.trim().is_empty() {
            let tz = stdout.trim();
            // Validate it looks like a timezone
            if tz.contains('/') || tz == "UTC" {
                return Ok(tz.to_string());
            }
        }

        // Try /etc/timezone (Debian-based)
        let cmd = "cat /etc/timezone 2>/dev/null";
        let (success, stdout, _) = Self::execute_command(connection, cmd, context)?;

        if success && !stdout.trim().is_empty() {
            return Ok(stdout.trim().to_string());
        }

        // Try /etc/sysconfig/clock (RHEL-based)
        let cmd = "grep '^ZONE=' /etc/sysconfig/clock 2>/dev/null | cut -d= -f2 | tr -d '\"'";
        let (success, stdout, _) = Self::execute_command(connection, cmd, context)?;

        if success && !stdout.trim().is_empty() {
            return Ok(stdout.trim().to_string());
        }

        Err(ModuleError::ExecutionFailed(
            "Could not determine current timezone".to_string(),
        ))
    }

    /// Get current timezone
    fn get_current_timezone(
        connection: &Arc<dyn Connection + Send + Sync>,
        strategy: &TimezoneStrategy,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        match strategy {
            TimezoneStrategy::Timedatectl => Self::get_timezone_timedatectl(connection, context),
            TimezoneStrategy::File | TimezoneStrategy::Auto => {
                // Try timedatectl first, fall back to file
                Self::get_timezone_timedatectl(connection, context)
                    .or_else(|_| Self::get_timezone_file(connection, context))
            }
        }
    }

    /// Set timezone using timedatectl
    fn set_timezone_timedatectl(
        connection: &Arc<dyn Connection + Send + Sync>,
        timezone: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = format!("timedatectl set-timezone {}", shell_escape(timezone));
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to set timezone: {}",
                stderr
            )))
        }
    }

    /// Set timezone using file-based method
    fn set_timezone_file(
        connection: &Arc<dyn Connection + Send + Sync>,
        timezone: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let zoneinfo_path = format!("/usr/share/zoneinfo/{}", timezone);

        // Verify timezone file exists
        let check_cmd = format!("test -f {}", shell_escape(&zoneinfo_path));
        let (exists, _, _) = Self::execute_command(connection, &check_cmd, context)?;

        if !exists {
            return Err(ModuleError::InvalidParameter(format!(
                "Timezone '{}' not found in /usr/share/zoneinfo",
                timezone
            )));
        }

        // Remove existing /etc/localtime and create symlink
        let cmd = format!(
            "rm -f /etc/localtime && ln -s {} /etc/localtime",
            shell_escape(&zoneinfo_path)
        );
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to set /etc/localtime: {}",
                stderr
            )));
        }

        // Update /etc/timezone if it exists (Debian-based)
        let cmd = format!(
            "test -f /etc/timezone && echo {} > /etc/timezone || true",
            shell_escape(timezone)
        );
        Self::execute_command(connection, &cmd, context)?;

        // Update /etc/sysconfig/clock if it exists (RHEL-based)
        let cmd = format!(
            "test -f /etc/sysconfig/clock && sed -i 's|^ZONE=.*|ZONE=\"{}\"|' /etc/sysconfig/clock || true",
            timezone
        );
        Self::execute_command(connection, &cmd, context)?;

        Ok(())
    }

    /// Get current NTP status
    fn get_ntp_status(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        // Try timedatectl first
        let cmd = "timedatectl show --property=NTP --value 2>/dev/null || timedatectl status | grep -i 'NTP.*enabled' | grep -qi yes && echo yes || echo no";
        let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;

        let output = stdout.trim().to_lowercase();
        Ok(output == "yes" || output == "true" || output == "active")
    }

    /// Set NTP status
    fn set_ntp_status(
        connection: &Arc<dyn Connection + Send + Sync>,
        enabled: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let status = if enabled { "true" } else { "false" };
        let cmd = format!("timedatectl set-ntp {}", status);
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok(())
        } else {
            // Fallback: try enabling/disabling common NTP services
            let services = ["chronyd", "ntpd", "systemd-timesyncd", "ntp"];
            let action = if enabled {
                "enable --now"
            } else {
                "disable --now"
            };

            for service in services {
                let cmd = format!("systemctl {} {} 2>/dev/null || true", action, service);
                let (worked, _, _) = Self::execute_command(connection, &cmd, context)?;
                if worked {
                    return Ok(());
                }
            }

            Err(ModuleError::ExecutionFailed(format!(
                "Failed to set NTP status: {}",
                stderr
            )))
        }
    }

    /// Get current hardware clock mode
    fn get_hwclock_mode(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<HwclockMode> {
        // Try timedatectl first
        let cmd = "timedatectl show --property=LocalRTC --value 2>/dev/null || timedatectl status | grep -i 'RTC in local TZ' | grep -qi yes && echo yes || echo no";
        let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;

        let output = stdout.trim().to_lowercase();
        if output == "yes" || output == "true" {
            Ok(HwclockMode::Local)
        } else {
            Ok(HwclockMode::Utc)
        }
    }

    /// Set hardware clock mode
    fn set_hwclock_mode(
        connection: &Arc<dyn Connection + Send + Sync>,
        mode: &HwclockMode,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let local_rtc = match mode {
            HwclockMode::Local => "true",
            HwclockMode::Utc => "false",
        };

        // Try timedatectl first
        let cmd = format!("timedatectl set-local-rtc {}", local_rtc);
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            return Ok(());
        }

        // Fallback: try hwclock directly
        let hwclock_opt = match mode {
            HwclockMode::Local => "--localtime",
            HwclockMode::Utc => "--utc",
        };

        let cmd = format!("hwclock --systohc {}", hwclock_opt);
        let (success, _, stderr2) = Self::execute_command(connection, &cmd, context)?;

        if success {
            // Update /etc/adjtime if it exists
            let adj_content = match mode {
                HwclockMode::Local => "LOCAL",
                HwclockMode::Utc => "UTC",
            };
            let cmd = format!(
                "test -f /etc/adjtime && sed -i 's/^\\(LOCAL\\|UTC\\)$/{}/g' /etc/adjtime || true",
                adj_content
            );
            Self::execute_command(connection, &cmd, context)?;

            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to set hardware clock mode: {} / {}",
                stderr, stderr2
            )))
        }
    }

    /// Validate timezone name
    fn validate_timezone(timezone: &str) -> ModuleResult<()> {
        if timezone.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Timezone cannot be empty".to_string(),
            ));
        }

        if !TIMEZONE_REGEX.is_match(timezone) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid timezone '{}': must be in format Area/Location (e.g., America/New_York) or UTC/GMT",
                timezone
            )));
        }

        Ok(())
    }

    /// Verify timezone exists on the system
    fn verify_timezone_exists(
        connection: &Arc<dyn Connection + Send + Sync>,
        timezone: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let zoneinfo_path = format!("/usr/share/zoneinfo/{}", timezone);
        let cmd = format!(
            "test -f {} && echo yes || echo no",
            shell_escape(&zoneinfo_path)
        );
        let (_, stdout, _) = Self::execute_command(connection, cmd.as_str(), context)?;
        Ok(stdout.trim() == "yes")
    }

    /// List available timezones
    #[allow(dead_code)]
    fn list_timezones(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<Vec<String>> {
        // Try timedatectl first
        let cmd = "timedatectl list-timezones 2>/dev/null || find /usr/share/zoneinfo -type f | sed 's|/usr/share/zoneinfo/||' | grep -E '^[A-Z]' | sort";
        let (success, stdout, _) = Self::execute_command(connection, cmd, context)?;

        if success {
            Ok(stdout.lines().map(|s| s.trim().to_string()).collect())
        } else {
            Ok(Vec::new())
        }
    }

    /// Get comprehensive time status
    fn get_time_status(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<serde_json::Value> {
        let cmd = "timedatectl status 2>/dev/null || echo 'timedatectl not available'";
        let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;

        Ok(serde_json::json!({
            "raw_output": stdout.trim()
        }))
    }
}

impl Module for TimezoneModule {
    fn name(&self) -> &'static str {
        "timezone"
    }

    fn description(&self) -> &'static str {
        "Manage system timezone, NTP, and hardware clock"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "Timezone module requires a connection for remote execution".to_string(),
            )
        })?;

        let timezone = params.get_string_required("name")?;
        Self::validate_timezone(&timezone)?;

        let strategy_str = params
            .get_string("use")?
            .unwrap_or_else(|| "auto".to_string());
        let strategy = TimezoneStrategy::from_str(&strategy_str)?;
        let ntp_enabled = params.get_bool("ntp")?;
        let hwclock_str = params.get_string("hwclock")?;
        let hwclock_mode = hwclock_str
            .as_ref()
            .map(|s| HwclockMode::from_str(s))
            .transpose()?;

        // Detect or use specified strategy
        let effective_strategy = match strategy {
            TimezoneStrategy::Auto => Self::detect_strategy(connection, context)?,
            s => s,
        };

        // Verify timezone exists
        if !Self::verify_timezone_exists(connection, &timezone, context)? {
            return Err(ModuleError::InvalidParameter(format!(
                "Timezone '{}' not found on the system. Use 'timedatectl list-timezones' to see available timezones.",
                timezone
            )));
        }

        // Get current timezone
        let current_timezone = Self::get_current_timezone(connection, &effective_strategy, context)
            .unwrap_or_else(|_| String::from("(unknown)"));

        let mut changed = false;
        let mut messages = Vec::new();

        // Set timezone if different
        if current_timezone != timezone {
            if context.check_mode {
                messages.push(format!(
                    "Would change timezone from '{}' to '{}'",
                    current_timezone, timezone
                ));
                changed = true;
            } else {
                match effective_strategy {
                    TimezoneStrategy::Timedatectl => {
                        Self::set_timezone_timedatectl(connection, &timezone, context)?;
                    }
                    TimezoneStrategy::File | TimezoneStrategy::Auto => {
                        Self::set_timezone_file(connection, &timezone, context)?;
                    }
                }
                messages.push(format!(
                    "Changed timezone from '{}' to '{}'",
                    current_timezone, timezone
                ));
                changed = true;
            }
        }

        // Handle NTP setting
        if let Some(should_enable_ntp) = ntp_enabled {
            let current_ntp = Self::get_ntp_status(connection, context).unwrap_or(false);

            if current_ntp != should_enable_ntp {
                if context.check_mode {
                    let action = if should_enable_ntp {
                        "enable"
                    } else {
                        "disable"
                    };
                    messages.push(format!("Would {} NTP synchronization", action));
                    changed = true;
                } else {
                    Self::set_ntp_status(connection, should_enable_ntp, context)?;
                    let action = if should_enable_ntp {
                        "Enabled"
                    } else {
                        "Disabled"
                    };
                    messages.push(format!("{} NTP synchronization", action));
                    changed = true;
                }
            }
        }

        // Handle hardware clock setting
        if let Some(ref desired_hwclock) = hwclock_mode {
            let current_hwclock = Self::get_hwclock_mode(connection, context)?;

            if &current_hwclock != desired_hwclock {
                if context.check_mode {
                    let mode_str = match desired_hwclock {
                        HwclockMode::Utc => "UTC",
                        HwclockMode::Local => "local time",
                    };
                    messages.push(format!("Would set hardware clock to {}", mode_str));
                    changed = true;
                } else {
                    Self::set_hwclock_mode(connection, desired_hwclock, context)?;
                    let mode_str = match desired_hwclock {
                        HwclockMode::Utc => "UTC",
                        HwclockMode::Local => "local time",
                    };
                    messages.push(format!("Set hardware clock to {}", mode_str));
                    changed = true;
                }
            }
        }

        // Build output
        let msg = if messages.is_empty() {
            format!("Timezone is already set to '{}'", timezone)
        } else {
            messages.join(". ")
        };

        let mut output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        // Add data
        output = output
            .with_data("timezone", serde_json::json!(timezone))
            .with_data("previous_timezone", serde_json::json!(current_timezone))
            .with_data(
                "strategy",
                serde_json::json!(format!("{:?}", effective_strategy).to_lowercase()),
            );

        if let Some(ntp) = ntp_enabled {
            output = output.with_data("ntp_enabled", serde_json::json!(ntp));
        }

        if let Some(ref hwclock) = hwclock_mode {
            let hwclock_str = match hwclock {
                HwclockMode::Utc => "UTC",
                HwclockMode::Local => "local",
            };
            output = output.with_data("hwclock", serde_json::json!(hwclock_str));
        }

        // Add time status if available
        if let Ok(status) = Self::get_time_status(connection, context) {
            output = output.with_data("time_status", status);
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timezone_strategy_from_str() {
        assert_eq!(
            TimezoneStrategy::from_str("timedatectl").unwrap(),
            TimezoneStrategy::Timedatectl
        );
        assert_eq!(
            TimezoneStrategy::from_str("systemd").unwrap(),
            TimezoneStrategy::Timedatectl
        );
        assert_eq!(
            TimezoneStrategy::from_str("file").unwrap(),
            TimezoneStrategy::File
        );
        assert_eq!(
            TimezoneStrategy::from_str("auto").unwrap(),
            TimezoneStrategy::Auto
        );
        assert!(TimezoneStrategy::from_str("invalid").is_err());
    }

    #[test]
    fn test_hwclock_mode_from_str() {
        assert_eq!(HwclockMode::from_str("utc").unwrap(), HwclockMode::Utc);
        assert_eq!(HwclockMode::from_str("UTC").unwrap(), HwclockMode::Utc);
        assert_eq!(HwclockMode::from_str("local").unwrap(), HwclockMode::Local);
        assert_eq!(
            HwclockMode::from_str("localtime").unwrap(),
            HwclockMode::Local
        );
        assert!(HwclockMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_validate_timezone_valid() {
        // Standard timezones
        assert!(TimezoneModule::validate_timezone("America/New_York").is_ok());
        assert!(TimezoneModule::validate_timezone("Europe/London").is_ok());
        assert!(TimezoneModule::validate_timezone("Asia/Tokyo").is_ok());
        assert!(TimezoneModule::validate_timezone("Australia/Sydney").is_ok());
        assert!(TimezoneModule::validate_timezone("Pacific/Auckland").is_ok());

        // UTC and GMT variants
        assert!(TimezoneModule::validate_timezone("UTC").is_ok());
        assert!(TimezoneModule::validate_timezone("GMT").is_ok());
        assert!(TimezoneModule::validate_timezone("GMT+0").is_ok());
        assert!(TimezoneModule::validate_timezone("GMT-5").is_ok());

        // Etc timezones
        assert!(TimezoneModule::validate_timezone("Etc/UTC").is_ok());
        assert!(TimezoneModule::validate_timezone("Etc/GMT+5").is_ok());
        assert!(TimezoneModule::validate_timezone("Etc/GMT-12").is_ok());

        // Deep nested
        assert!(TimezoneModule::validate_timezone("America/Argentina/Buenos_Aires").is_ok());
        assert!(TimezoneModule::validate_timezone("America/Indiana/Indianapolis").is_ok());
    }

    #[test]
    fn test_validate_timezone_invalid() {
        assert!(TimezoneModule::validate_timezone("").is_err());
        assert!(TimezoneModule::validate_timezone("InvalidTimezone").is_err());
        assert!(TimezoneModule::validate_timezone("America").is_err());
        assert!(TimezoneModule::validate_timezone("timezone; rm -rf /").is_err());
        assert!(TimezoneModule::validate_timezone("$(whoami)").is_err());
    }

    #[test]
    fn test_timezone_module_metadata() {
        let module = TimezoneModule;
        assert_eq!(module.name(), "timezone");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("America/New_York"), "America/New_York");
        assert_eq!(shell_escape("UTC"), "UTC");
        assert_eq!(shell_escape("GMT+5"), "GMT+5");
        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
    }
}
