//! Sysctl module - Kernel parameters management
//!
//! This module manages kernel parameters via sysctl. It can set runtime
//! parameters and optionally persist them in configuration files.

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

/// Regex pattern for validating sysctl parameter names
static SYSCTL_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z0-9_./]+$").expect("Invalid sysctl name regex"));

/// Desired state for a sysctl parameter
#[derive(Debug, Clone, PartialEq)]
pub enum SysctlState {
    Present,
    Absent,
}

impl SysctlState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(SysctlState::Present),
            "absent" => Ok(SysctlState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Module for sysctl parameter management
pub struct SysctlModule;

impl SysctlModule {
    /// Get execution options with become support if needed
    fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
        let mut options = ExecuteOptions::new();
        if context.r#become {
            options = options.with_escalation(context.become_user.clone());
            if let Some(ref method) = context.become_method {
                options.escalate_method = Some(method.clone());
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

    /// Get current value of a sysctl parameter
    fn get_current_value(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        let cmd = format!("sysctl -n {} 2>/dev/null", shell_escape(name));
        let (success, stdout, _) = Self::execute_command(connection, &cmd, context)?;

        if success && !stdout.trim().is_empty() {
            Ok(Some(stdout.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    /// Set a sysctl parameter at runtime
    fn set_runtime_value(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        value: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = format!("sysctl -w {}={}", shell_escape(name), shell_escape(value));
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to set sysctl {}: {}",
                name, stderr
            )))
        }
    }

    /// Read sysctl configuration file
    fn read_sysctl_conf(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let cmd = format!("cat {} 2>/dev/null || true", shell_escape(path));
        let (_, stdout, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(stdout)
    }

    /// Write sysctl configuration file
    fn write_sysctl_conf(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        content: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Ensure directory exists
        if let Some(dir) = std::path::Path::new(path).parent() {
            let mkdir_cmd = format!("mkdir -p {}", shell_escape(dir.to_str().unwrap_or("")));
            Self::execute_command(connection, &mkdir_cmd, context)?;
        }

        let cmd = format!(
            "cat << 'RUSTIBLE_EOF' > {}\n{}\nRUSTIBLE_EOF",
            shell_escape(path),
            content.trim()
        );
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to write {}: {}",
                path, stderr
            )))
        }
    }

    /// Find a parameter in sysctl config content
    fn find_in_config(config: &str, name: &str) -> Option<(usize, String)> {
        for (i, line) in config.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse key = value or key=value
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                if key == name {
                    let value = line[eq_pos + 1..].trim();
                    return Some((i, value.to_string()));
                }
            }
        }
        None
    }

    /// Update or add parameter in sysctl config
    fn update_config(config: &str, name: &str, value: &str) -> (String, bool) {
        let mut lines: Vec<String> = config.lines().map(|s| s.to_string()).collect();
        let mut found = false;
        let new_line = format!("{} = {}", name, value);

        for line in &mut lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim();
                if key == name {
                    *line = new_line.clone();
                    found = true;
                    break;
                }
            }
        }

        if !found {
            if !lines.is_empty() && !lines.last().map(|l| l.is_empty()).unwrap_or(true) {
                lines.push(String::new());
            }
            lines.push(new_line);
        }

        (lines.join("\n"), !found)
    }

    /// Remove parameter from sysctl config
    fn remove_from_config(config: &str, name: &str) -> (String, bool) {
        let mut lines = Vec::new();
        let mut removed = false;

        for line in config.lines() {
            let trimmed = line.trim();

            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                if let Some(eq_pos) = trimmed.find('=') {
                    let key = trimmed[..eq_pos].trim();
                    if key == name {
                        removed = true;
                        continue;
                    }
                }
            }
            lines.push(line);
        }

        (lines.join("\n"), removed)
    }

    /// Reload sysctl configuration
    fn reload_sysctl(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = match path {
            Some(p) => format!("sysctl -p {}", shell_escape(p)),
            None => "sysctl -p".to_string(),
        };

        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to reload sysctl: {}",
                stderr
            )))
        }
    }

    /// Validate sysctl parameter name
    fn validate_name(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Sysctl parameter name cannot be empty".to_string(),
            ));
        }

        if !SYSCTL_NAME_REGEX.is_match(name) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid sysctl parameter name '{}': must contain only alphanumeric characters, underscores, dots, and slashes",
                name
            )));
        }

        Ok(())
    }
}

impl Module for SysctlModule {
    fn name(&self) -> &'static str {
        "sysctl"
    }

    fn description(&self) -> &'static str {
        "Manage kernel parameters via sysctl"
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
                "Sysctl module requires a connection for remote execution".to_string(),
            )
        })?;

        let name = params.get_string_required("name")?;
        Self::validate_name(&name)?;

        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = SysctlState::from_str(&state_str)?;

        let value = params.get_string("value")?;
        let sysctl_file = params
            .get_string("sysctl_file")?
            .unwrap_or_else(|| "/etc/sysctl.d/99-rustible.conf".to_string());
        let reload = params.get_bool_or("reload", true);
        let ignoreerrors = params.get_bool_or("ignoreerrors", false);

        let current_value = Self::get_current_value(connection, &name, context)?;
        let config_content = Self::read_sysctl_conf(connection, &sysctl_file, context)?;
        let config_value = Self::find_in_config(&config_content, &name);

        let mut changed = false;
        let mut messages = Vec::new();

        match state {
            SysctlState::Absent => {
                // Remove from config file if present
                if config_value.is_some() {
                    if context.check_mode {
                        messages.push(format!("Would remove '{}' from {}", name, sysctl_file));
                        changed = true;
                    } else {
                        let (new_config, _) = Self::remove_from_config(&config_content, &name);
                        Self::write_sysctl_conf(connection, &sysctl_file, &new_config, context)?;
                        messages.push(format!("Removed '{}' from {}", name, sysctl_file));
                        changed = true;
                    }
                }

                // Note: We don't actually "unset" a sysctl at runtime, just remove from config
                if !changed {
                    return Ok(ModuleOutput::ok(format!(
                        "Sysctl '{}' already absent from configuration",
                        name
                    )));
                }
            }

            SysctlState::Present => {
                let value = value.ok_or_else(|| {
                    ModuleError::MissingParameter(
                        "value is required when state is present".to_string(),
                    )
                })?;

                // Check if runtime value needs updating
                let runtime_needs_update = current_value.as_ref() != Some(&value);

                // Check if config needs updating
                let config_needs_update = config_value.as_ref().map(|(_, v)| v) != Some(&value);

                if runtime_needs_update {
                    if context.check_mode {
                        messages.push(format!(
                            "Would set '{}' to '{}' (currently '{}')",
                            name,
                            value,
                            current_value.as_deref().unwrap_or("(not set)")
                        ));
                        changed = true;
                    } else {
                        match Self::set_runtime_value(connection, &name, &value, context) {
                            Ok(()) => {
                                messages.push(format!("Set '{}' to '{}'", name, value));
                                changed = true;
                            }
                            Err(e) => {
                                if ignoreerrors {
                                    messages.push(format!(
                                        "Failed to set runtime value (ignored): {}",
                                        e
                                    ));
                                } else {
                                    return Err(e);
                                }
                            }
                        }
                    }
                }

                if config_needs_update {
                    if context.check_mode {
                        messages.push(format!("Would update '{}' in {}", name, sysctl_file));
                        changed = true;
                    } else {
                        let (new_config, _) = Self::update_config(&config_content, &name, &value);
                        Self::write_sysctl_conf(connection, &sysctl_file, &new_config, context)?;
                        messages.push(format!("Updated '{}' in {}", name, sysctl_file));
                        changed = true;

                        // Reload if requested
                        if reload {
                            Self::reload_sysctl(connection, Some(&sysctl_file), context)?;
                            messages.push("Reloaded sysctl".to_string());
                        }
                    }
                }

                if !changed {
                    return Ok(ModuleOutput::ok(format!(
                        "Sysctl '{}' already set to '{}'",
                        name, value
                    ))
                    .with_data("value", serde_json::json!(value)));
                }
            }
        }

        let msg = messages.join(". ");
        let mut output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        if let Some(v) = &current_value {
            // Use current_value for reference since value was consumed in Present branch
            output = output.with_data("current_value", serde_json::json!(v));
        }
        if let Some(cv) = current_value {
            output = output.with_data("previous_value", serde_json::json!(cv));
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sysctl_state_from_str() {
        assert_eq!(
            SysctlState::from_str("present").unwrap(),
            SysctlState::Present
        );
        assert_eq!(
            SysctlState::from_str("absent").unwrap(),
            SysctlState::Absent
        );
        assert!(SysctlState::from_str("invalid").is_err());
    }

    #[test]
    fn test_validate_name() {
        assert!(SysctlModule::validate_name("net.ipv4.ip_forward").is_ok());
        assert!(SysctlModule::validate_name("kernel.pid_max").is_ok());
        assert!(SysctlModule::validate_name("vm.swappiness").is_ok());
        assert!(SysctlModule::validate_name("net/core/somaxconn").is_ok());
        assert!(SysctlModule::validate_name("").is_err());
        assert!(SysctlModule::validate_name("param; rm -rf").is_err());
    }

    #[test]
    fn test_find_in_config() {
        let config = r#"
# Network settings
net.ipv4.ip_forward = 1
net.ipv4.conf.all.rp_filter = 1

# Memory settings
vm.swappiness=10
"#;

        let result = SysctlModule::find_in_config(config, "net.ipv4.ip_forward");
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, "1");

        let result = SysctlModule::find_in_config(config, "vm.swappiness");
        assert!(result.is_some());
        assert_eq!(result.unwrap().1, "10");

        let result = SysctlModule::find_in_config(config, "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_update_config() {
        let config = "net.ipv4.ip_forward = 0\nvm.swappiness = 60";

        // Update existing
        let (result, is_new) = SysctlModule::update_config(config, "net.ipv4.ip_forward", "1");
        assert!(!is_new);
        assert!(result.contains("net.ipv4.ip_forward = 1"));
        assert!(result.contains("vm.swappiness = 60"));

        // Add new
        let (result, is_new) = SysctlModule::update_config(config, "kernel.pid_max", "65535");
        assert!(is_new);
        assert!(result.contains("kernel.pid_max = 65535"));
    }

    #[test]
    fn test_remove_from_config() {
        let config = r#"net.ipv4.ip_forward = 1
vm.swappiness = 10
kernel.pid_max = 65535"#;

        let (result, removed) = SysctlModule::remove_from_config(config, "vm.swappiness");
        assert!(removed);
        assert!(!result.contains("vm.swappiness"));
        assert!(result.contains("net.ipv4.ip_forward"));
        assert!(result.contains("kernel.pid_max"));

        let (_, removed) = SysctlModule::remove_from_config(config, "nonexistent");
        assert!(!removed);
    }

    #[test]
    fn test_sysctl_module_metadata() {
        let module = SysctlModule;
        assert_eq!(module.name(), "sysctl");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }
}
