//! Hostname module - Hostname management
//!
//! This module manages the system hostname, supporting both transient (runtime)
//! and persistent hostname configuration.

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Regex pattern for validating hostnames per RFC 1123
static HOSTNAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$")
        .expect("Invalid hostname regex")
});

/// Hostname management strategy
#[derive(Debug, Clone, PartialEq)]
pub enum HostnameStrategy {
    /// Use hostnamectl (systemd)
    Systemd,
    /// Use traditional /etc/hostname
    File,
    /// Detect automatically
    Auto,
}

impl HostnameStrategy {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "systemd" => Ok(HostnameStrategy::Systemd),
            "file" => Ok(HostnameStrategy::File),
            "auto" => Ok(HostnameStrategy::Auto),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid use '{}'. Valid values: systemd, file, auto",
                s
            ))),
        }
    }
}

/// Module for hostname management
pub struct HostnameModule;

impl HostnameModule {
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

    /// Detect available hostname strategy
    fn detect_strategy(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<HostnameStrategy> {
        // Check for systemd/hostnamectl
        let cmd = "which hostnamectl >/dev/null 2>&1 && echo yes || echo no";
        let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;

        if stdout.trim() == "yes" {
            // Verify systemd is actually running
            let cmd = "test -d /run/systemd/system && echo yes || echo no";
            let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;
            if stdout.trim() == "yes" {
                return Ok(HostnameStrategy::Systemd);
            }
        }

        Ok(HostnameStrategy::File)
    }

    /// Get current hostname
    fn get_current_hostname(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let (success, stdout, stderr) = Self::execute_command(connection, "hostname", context)?;

        if success {
            Ok(stdout.trim().to_string())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to get hostname: {}",
                stderr
            )))
        }
    }

    /// Get pretty hostname (systemd only)
    fn get_pretty_hostname(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        let cmd = "hostnamectl --static 2>/dev/null | head -1";
        let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;
        let hostname = stdout.trim();
        if hostname.is_empty() {
            Ok(None)
        } else {
            Ok(Some(hostname.to_string()))
        }
    }

    /// Set hostname using hostnamectl (systemd)
    fn set_hostname_systemd(
        connection: &Arc<dyn Connection + Send + Sync>,
        hostname: &str,
        pretty_hostname: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Set static hostname
        let cmd = format!("hostnamectl set-hostname {}", shell_escape(hostname));
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to set hostname: {}",
                stderr
            )));
        }

        // Set pretty hostname if provided
        if let Some(pretty) = pretty_hostname {
            let cmd = format!("hostnamectl set-hostname --pretty {}", shell_escape(pretty));
            let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to set pretty hostname: {}",
                    stderr
                )));
            }
        }

        Ok(())
    }

    /// Set hostname using traditional file method
    fn set_hostname_file(
        connection: &Arc<dyn Connection + Send + Sync>,
        hostname: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Write to /etc/hostname
        let cmd = format!("echo {} > /etc/hostname", shell_escape(hostname));
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to write /etc/hostname: {}",
                stderr
            )));
        }

        // Apply the hostname immediately
        let cmd = format!("hostname {}", shell_escape(hostname));
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to set runtime hostname: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Update /etc/hosts if needed
    fn update_etc_hosts(
        connection: &Arc<dyn Connection + Send + Sync>,
        old_hostname: &str,
        new_hostname: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        // Read current /etc/hosts
        let (success, hosts_content, _) =
            Self::execute_command(connection, "cat /etc/hosts", context)?;

        if !success {
            return Ok(false);
        }

        // Check if old hostname is in hosts file
        if !hosts_content.contains(old_hostname) {
            return Ok(false);
        }

        // Replace old hostname with new hostname
        let new_hosts = hosts_content.replace(old_hostname, new_hostname);

        if new_hosts == hosts_content {
            return Ok(false);
        }

        let cmd = format!(
            "cat << 'RUSTIBLE_EOF' > /etc/hosts\n{}\nRUSTIBLE_EOF",
            new_hosts.trim()
        );
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to update /etc/hosts: {}",
                stderr
            )));
        }

        Ok(true)
    }

    /// Validate hostname according to RFC 1123
    fn validate_hostname(hostname: &str) -> ModuleResult<()> {
        if hostname.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Hostname cannot be empty".to_string(),
            ));
        }

        if hostname.len() > 253 {
            return Err(ModuleError::InvalidParameter(format!(
                "Hostname '{}' is too long (max 253 characters)",
                hostname
            )));
        }

        if !HOSTNAME_REGEX.is_match(hostname) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid hostname '{}': must start and end with alphanumeric, contain only alphanumeric and hyphens",
                hostname
            )));
        }

        Ok(())
    }
}

impl Module for HostnameModule {
    fn name(&self) -> &'static str {
        "hostname"
    }

    fn description(&self) -> &'static str {
        "Manage system hostname"
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
                "Hostname module requires a connection for remote execution".to_string(),
            )
        })?;

        let name = params.get_string_required("name")?;
        Self::validate_hostname(&name)?;

        let strategy_str = params
            .get_string("use")?
            .unwrap_or_else(|| "auto".to_string());
        let strategy = HostnameStrategy::from_str(&strategy_str)?;
        let pretty_hostname = params.get_string("pretty_hostname")?;
        let update_hosts = params.get_bool_or("update_hosts", true);

        // Detect or use specified strategy
        let effective_strategy = match strategy {
            HostnameStrategy::Auto => Self::detect_strategy(connection, context)?,
            s => s,
        };

        // Get current hostname
        let current_hostname = Self::get_current_hostname(connection, context)?;

        // Check if change is needed
        if current_hostname == name {
            // Check if pretty hostname needs update (systemd only)
            if effective_strategy == HostnameStrategy::Systemd {
                if let Some(ref pretty) = pretty_hostname {
                    let current_pretty =
                        Self::get_pretty_hostname(connection, context)?.unwrap_or_default();
                    if current_pretty != *pretty {
                        if context.check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would update pretty hostname to '{}'",
                                pretty
                            )));
                        }

                        Self::set_hostname_systemd(connection, &name, Some(pretty), context)?;
                        return Ok(ModuleOutput::changed(format!(
                            "Updated pretty hostname to '{}'",
                            pretty
                        ))
                        .with_data("name", serde_json::json!(name))
                        .with_data("pretty_hostname", serde_json::json!(pretty)));
                    }
                }
            }

            return Ok(ModuleOutput::ok(format!("Hostname is already '{}'", name))
                .with_data("name", serde_json::json!(name)));
        }

        // Check mode
        if context.check_mode {
            let mut msg = format!(
                "Would change hostname from '{}' to '{}'",
                current_hostname, name
            );
            if update_hosts {
                msg.push_str(" and update /etc/hosts");
            }
            return Ok(ModuleOutput::changed(msg));
        }

        // Apply the change based on strategy
        match effective_strategy {
            HostnameStrategy::Systemd => {
                Self::set_hostname_systemd(connection, &name, pretty_hostname.as_deref(), context)?;
            }
            HostnameStrategy::File | HostnameStrategy::Auto => {
                Self::set_hostname_file(connection, &name, context)?;
            }
        }

        let mut messages = vec![format!(
            "Changed hostname from '{}' to '{}'",
            current_hostname, name
        )];

        // Update /etc/hosts if requested
        if update_hosts {
            let hosts_updated =
                Self::update_etc_hosts(connection, &current_hostname, &name, context)?;
            if hosts_updated {
                messages.push("Updated /etc/hosts".to_string());
            }
        }

        let mut output = ModuleOutput::changed(messages.join(". "))
            .with_data("name", serde_json::json!(name))
            .with_data("previous_name", serde_json::json!(current_hostname))
            .with_data(
                "strategy",
                serde_json::json!(format!("{:?}", effective_strategy).to_lowercase()),
            );

        if let Some(ref pretty) = pretty_hostname {
            output = output.with_data("pretty_hostname", serde_json::json!(pretty));
        }

        Ok(output)
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        self.execute(params, &check_context)
    }

    fn diff(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let connection = match context.connection.as_ref() {
            Some(c) => c,
            None => return Ok(None),
        };

        let name = params.get_string_required("name")?;
        let current_hostname = Self::get_current_hostname(connection, context).unwrap_or_default();

        if current_hostname == name {
            return Ok(None);
        }

        Ok(Some(Diff::new(
            format!("hostname: {}", current_hostname),
            format!("hostname: {}", name),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hostname_strategy_from_str() {
        assert_eq!(
            HostnameStrategy::from_str("systemd").unwrap(),
            HostnameStrategy::Systemd
        );
        assert_eq!(
            HostnameStrategy::from_str("file").unwrap(),
            HostnameStrategy::File
        );
        assert_eq!(
            HostnameStrategy::from_str("auto").unwrap(),
            HostnameStrategy::Auto
        );
        assert!(HostnameStrategy::from_str("invalid").is_err());
    }

    #[test]
    fn test_validate_hostname_valid() {
        assert!(HostnameModule::validate_hostname("server1").is_ok());
        assert!(HostnameModule::validate_hostname("web-server").is_ok());
        assert!(HostnameModule::validate_hostname("db01").is_ok());
        assert!(HostnameModule::validate_hostname("app.example.com").is_ok());
        assert!(HostnameModule::validate_hostname("mail-server-01.corp.example.com").is_ok());
        assert!(HostnameModule::validate_hostname("a").is_ok());
        assert!(HostnameModule::validate_hostname("a1").is_ok());
    }

    #[test]
    fn test_validate_hostname_invalid() {
        assert!(HostnameModule::validate_hostname("").is_err());
        assert!(HostnameModule::validate_hostname("-server").is_err());
        assert!(HostnameModule::validate_hostname("server-").is_err());
        assert!(HostnameModule::validate_hostname("server_name").is_err());
        assert!(HostnameModule::validate_hostname("server name").is_err());
        assert!(HostnameModule::validate_hostname(".server").is_err());
        assert!(HostnameModule::validate_hostname("server.").is_err());

        // Test max length (253 characters)
        let long_hostname = "a".repeat(254);
        assert!(HostnameModule::validate_hostname(&long_hostname).is_err());
    }

    #[test]
    fn test_validate_hostname_labels() {
        // Each label can be up to 63 characters
        let valid_label = format!("{}.example.com", "a".repeat(63));
        assert!(HostnameModule::validate_hostname(&valid_label).is_ok());
    }

    #[test]
    fn test_hostname_module_metadata() {
        let module = HostnameModule;
        assert_eq!(module.name(), "hostname");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("with-hyphen"), "with-hyphen");
        assert_eq!(shell_escape("with.dot"), "with.dot");
        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
    }
}
