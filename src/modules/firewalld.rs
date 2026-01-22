//! Firewalld module - Firewall management for systems using firewalld
//!
//! This module manages firewall rules using firewalld (Red Hat, Fedora, CentOS).
//! It supports zone management, service configuration, port rules, and rich rules.
//!
//! ## Parameters
//!
//! - `zone`: The firewalld zone to operate on (default: public)
//! - `service`: Name of a service to add/remove (e.g., "http", "https", "ssh")
//! - `port`: Port/protocol specification (e.g., "8080/tcp", "53/udp")
//! - `source`: Source address/network to add/remove from zone
//! - `interface`: Interface to bind to the zone
//! - `masquerade`: Enable/disable masquerading for the zone
//! - `rich_rule`: Rich rule specification
//! - `icmp_block`: ICMP type to block
//! - `icmp_block_inversion`: Invert ICMP block behavior
//! - `target`: Default target for the zone (ACCEPT, DROP, REJECT, default)
//! - `state`: Desired state (enabled, disabled, present, absent)
//! - `permanent`: Make changes permanent (default: true)
//! - `immediate`: Apply changes immediately to runtime (default: true)
//! - `offline`: Run in offline mode (no daemon)
//! - `timeout`: Timeout for temporary rules in seconds

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Regex for validating zone names
static ZONE_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]*$").expect("Invalid zone name regex"));

/// Regex for validating service names
static SERVICE_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9_-]*$").expect("Invalid service name regex"));

/// Regex for validating port specifications (port/protocol)
static PORT_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d+(-\d+)?/(tcp|udp|sctp|dccp))$").expect("Invalid port regex"));

/// Regex for validating source addresses (IP or CIDR)
static SOURCE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(/\d{1,2})?|[a-fA-F0-9:]+(/\d{1,3})?)$")
        .expect("Invalid source regex")
});

/// Regex for validating interface names
static INTERFACE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9._-]*$").expect("Invalid interface regex"));

/// Desired state for firewall rules
#[derive(Debug, Clone, PartialEq)]
pub enum FirewalldState {
    /// Rule should be present/enabled
    Enabled,
    /// Rule should be absent/disabled
    Disabled,
    /// Alias for Enabled
    Present,
    /// Alias for Disabled
    Absent,
}

impl FirewalldState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "enabled" | "present" => Ok(FirewalldState::Enabled),
            "disabled" | "absent" => Ok(FirewalldState::Disabled),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: enabled, disabled, present, absent",
                s
            ))),
        }
    }

    pub fn should_be_present(&self) -> bool {
        matches!(self, FirewalldState::Enabled | FirewalldState::Present)
    }
}

impl std::str::FromStr for FirewalldState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        FirewalldState::from_str(s)
    }
}

/// Zone target options
#[derive(Debug, Clone, PartialEq)]
pub enum ZoneTarget {
    Default,
    Accept,
    Drop,
    Reject,
}

impl ZoneTarget {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_uppercase().as_str() {
            "DEFAULT" | "%%REJECT%%" => Ok(ZoneTarget::Default),
            "ACCEPT" => Ok(ZoneTarget::Accept),
            "DROP" => Ok(ZoneTarget::Drop),
            "REJECT" => Ok(ZoneTarget::Reject),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid target '{}'. Valid targets: default, ACCEPT, DROP, REJECT",
                s
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ZoneTarget::Default => "default",
            ZoneTarget::Accept => "ACCEPT",
            ZoneTarget::Drop => "DROP",
            ZoneTarget::Reject => "REJECT",
        }
    }
}

impl std::str::FromStr for ZoneTarget {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ZoneTarget::from_str(s)
    }
}

/// Configuration parsed from module parameters
#[derive(Debug, Clone)]
struct FirewalldConfig {
    zone: String,
    service: Option<String>,
    port: Option<String>,
    source: Option<String>,
    interface: Option<String>,
    masquerade: Option<bool>,
    rich_rule: Option<String>,
    icmp_block: Option<String>,
    icmp_block_inversion: Option<bool>,
    target: Option<ZoneTarget>,
    state: FirewalldState,
    permanent: bool,
    immediate: bool,
    offline: bool,
    timeout: Option<u32>,
}

impl FirewalldConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let zone = params
            .get_string("zone")?
            .unwrap_or_else(|| "public".to_string());
        validate_zone_name(&zone)?;

        let service = params.get_string("service")?;
        if let Some(ref s) = service {
            validate_service_name(s)?;
        }

        let port = params.get_string("port")?;
        if let Some(ref p) = port {
            validate_port(p)?;
        }

        let source = params.get_string("source")?;
        if let Some(ref s) = source {
            validate_source(s)?;
        }

        let interface = params.get_string("interface")?;
        if let Some(ref i) = interface {
            validate_interface(i)?;
        }

        let target = if let Some(t) = params.get_string("target")? {
            Some(ZoneTarget::from_str(&t)?)
        } else {
            None
        };

        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "enabled".to_string());
        let state = FirewalldState::from_str(&state_str)?;

        Ok(Self {
            zone,
            service,
            port,
            source,
            interface,
            masquerade: params.get_bool("masquerade")?,
            rich_rule: params.get_string("rich_rule")?,
            icmp_block: params.get_string("icmp_block")?,
            icmp_block_inversion: params.get_bool("icmp_block_inversion")?,
            target,
            state,
            permanent: params.get_bool_or("permanent", true),
            immediate: params.get_bool_or("immediate", true),
            offline: params.get_bool_or("offline", false),
            timeout: params.get_u32("timeout")?,
        })
    }

    /// Check if any rule-specific parameter is set
    fn has_rule(&self) -> bool {
        self.service.is_some()
            || self.port.is_some()
            || self.source.is_some()
            || self.interface.is_some()
            || self.masquerade.is_some()
            || self.rich_rule.is_some()
            || self.icmp_block.is_some()
            || self.icmp_block_inversion.is_some()
            || self.target.is_some()
    }
}

/// Firewalld module
pub struct FirewalldModule;

impl FirewalldModule {
    /// Get execution options with become support
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

    /// Check if firewalld is available and running
    fn check_firewalld(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let (success, _, _) = Self::execute_command(
            connection,
            "command -v firewall-cmd >/dev/null 2>&1 && firewall-cmd --state 2>/dev/null | grep -q running",
            context,
        )?;
        Ok(success)
    }

    /// Build the firewall-cmd command with common options
    fn build_cmd(config: &FirewalldConfig, action: &str) -> String {
        let mut cmd = String::from("firewall-cmd");

        if config.offline {
            cmd.push_str(" --offline");
        }

        if config.permanent {
            cmd.push_str(" --permanent");
        }

        cmd.push_str(&format!(" --zone={}", shell_escape(&config.zone)));

        if let Some(timeout) = config.timeout {
            cmd.push_str(&format!(" --timeout={}", timeout));
        }

        cmd.push_str(&format!(" {}", action));

        cmd
    }

    /// Query if a service is enabled in a zone
    fn query_service(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        service: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = Self::build_cmd(
            config,
            &format!("--query-service={}", shell_escape(service)),
        );
        let (success, _, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(success)
    }

    /// Add or remove a service
    fn manage_service(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        service: &str,
        add: bool,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = if add {
            format!("--add-service={}", shell_escape(service))
        } else {
            format!("--remove-service={}", shell_escape(service))
        };

        let cmd = Self::build_cmd(config, &action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} service '{}': {}",
                if add { "add" } else { "remove" },
                service,
                stderr
            )))
        }
    }

    /// Query if a port is enabled in a zone
    fn query_port(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        port: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = Self::build_cmd(config, &format!("--query-port={}", shell_escape(port)));
        let (success, _, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(success)
    }

    /// Add or remove a port
    fn manage_port(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        port: &str,
        add: bool,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = if add {
            format!("--add-port={}", shell_escape(port))
        } else {
            format!("--remove-port={}", shell_escape(port))
        };

        let cmd = Self::build_cmd(config, &action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} port '{}': {}",
                if add { "add" } else { "remove" },
                port,
                stderr
            )))
        }
    }

    /// Query if a source is in a zone
    fn query_source(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        source: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = Self::build_cmd(config, &format!("--query-source={}", shell_escape(source)));
        let (success, _, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(success)
    }

    /// Add or remove a source
    fn manage_source(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        source: &str,
        add: bool,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = if add {
            format!("--add-source={}", shell_escape(source))
        } else {
            format!("--remove-source={}", shell_escape(source))
        };

        let cmd = Self::build_cmd(config, &action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} source '{}': {}",
                if add { "add" } else { "remove" },
                source,
                stderr
            )))
        }
    }

    /// Query if an interface is in a zone
    fn query_interface(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        interface: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = Self::build_cmd(
            config,
            &format!("--query-interface={}", shell_escape(interface)),
        );
        let (success, _, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(success)
    }

    /// Add or remove an interface
    fn manage_interface(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        interface: &str,
        add: bool,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = if add {
            format!("--add-interface={}", shell_escape(interface))
        } else {
            format!("--remove-interface={}", shell_escape(interface))
        };

        let cmd = Self::build_cmd(config, &action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} interface '{}': {}",
                if add { "add" } else { "remove" },
                interface,
                stderr
            )))
        }
    }

    /// Query if masquerading is enabled
    fn query_masquerade(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = Self::build_cmd(config, "--query-masquerade");
        let (success, _, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(success)
    }

    /// Enable or disable masquerading
    fn manage_masquerade(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        enable: bool,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = if enable {
            "--add-masquerade"
        } else {
            "--remove-masquerade"
        };

        let cmd = Self::build_cmd(config, action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} masquerade: {}",
                if enable { "enable" } else { "disable" },
                stderr
            )))
        }
    }

    /// Query if a rich rule exists
    fn query_rich_rule(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        rule: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = Self::build_cmd(config, &format!("--query-rich-rule={}", shell_escape(rule)));
        let (success, _, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(success)
    }

    /// Add or remove a rich rule
    fn manage_rich_rule(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        rule: &str,
        add: bool,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = if add {
            format!("--add-rich-rule={}", shell_escape(rule))
        } else {
            format!("--remove-rich-rule={}", shell_escape(rule))
        };

        let cmd = Self::build_cmd(config, &action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} rich rule: {}",
                if add { "add" } else { "remove" },
                stderr
            )))
        }
    }

    /// Query if an ICMP block exists
    fn query_icmp_block(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        icmp_type: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = Self::build_cmd(
            config,
            &format!("--query-icmp-block={}", shell_escape(icmp_type)),
        );
        let (success, _, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(success)
    }

    /// Add or remove an ICMP block
    fn manage_icmp_block(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        icmp_type: &str,
        add: bool,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = if add {
            format!("--add-icmp-block={}", shell_escape(icmp_type))
        } else {
            format!("--remove-icmp-block={}", shell_escape(icmp_type))
        };

        let cmd = Self::build_cmd(config, &action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} ICMP block '{}': {}",
                if add { "add" } else { "remove" },
                icmp_type,
                stderr
            )))
        }
    }

    /// Query ICMP block inversion status
    fn query_icmp_block_inversion(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = Self::build_cmd(config, "--query-icmp-block-inversion");
        let (success, _, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(success)
    }

    /// Enable or disable ICMP block inversion
    fn manage_icmp_block_inversion(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        enable: bool,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = if enable {
            "--add-icmp-block-inversion"
        } else {
            "--remove-icmp-block-inversion"
        };

        let cmd = Self::build_cmd(config, action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} ICMP block inversion: {}",
                if enable { "enable" } else { "disable" },
                stderr
            )))
        }
    }

    /// Get zone target
    fn get_zone_target(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let cmd = Self::build_cmd(config, "--get-target");
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok(stdout.trim().to_string())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to get zone target: {}",
                stderr
            )))
        }
    }

    /// Set zone target
    fn set_zone_target(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &FirewalldConfig,
        target: &ZoneTarget,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let action = format!("--set-target={}", target.as_str());

        let cmd = Self::build_cmd(config, &action);
        let (success, stdout, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok((true, stdout))
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to set zone target: {}",
                stderr
            )))
        }
    }

    /// Reload firewalld to apply permanent changes
    fn reload_firewalld(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let (success, _, stderr) =
            Self::execute_command(connection, "firewall-cmd --reload", context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to reload firewalld: {}",
                stderr
            )))
        }
    }
}

impl Module for FirewalldModule {
    fn name(&self) -> &'static str {
        "firewalld"
    }

    fn description(&self) -> &'static str {
        "Manage firewall rules using firewalld"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Firewall operations should be exclusive per host to avoid conflicts
        ParallelizationHint::HostExclusive
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "Firewalld module requires a connection for remote execution".to_string(),
            )
        })?;

        let config = FirewalldConfig::from_params(params)?;

        // Check if firewalld is available
        if !config.offline && !Self::check_firewalld(connection, context)? {
            return Err(ModuleError::ExecutionFailed(
                "firewalld is not available or not running".to_string(),
            ));
        }

        // Validate that at least one rule type is specified
        if !config.has_rule() {
            return Err(ModuleError::MissingParameter(
                "At least one of service, port, source, interface, masquerade, rich_rule, icmp_block, icmp_block_inversion, or target must be specified".to_string()
            ));
        }

        let mut changed = false;
        let mut messages = Vec::new();
        let should_enable = config.state.should_be_present();

        // Handle service
        if let Some(ref service) = config.service {
            let is_enabled = Self::query_service(connection, &config, service, context)?;

            if should_enable && !is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would add service '{}' to zone '{}'",
                        service, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_service(connection, &config, service, true, context)?;
                    messages.push(format!(
                        "Added service '{}' to zone '{}'",
                        service, config.zone
                    ));
                    changed = true;
                }
            } else if !should_enable && is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would remove service '{}' from zone '{}'",
                        service, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_service(connection, &config, service, false, context)?;
                    messages.push(format!(
                        "Removed service '{}' from zone '{}'",
                        service, config.zone
                    ));
                    changed = true;
                }
            }
        }

        // Handle port
        if let Some(ref port) = config.port {
            let is_enabled = Self::query_port(connection, &config, port, context)?;

            if should_enable && !is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would add port '{}' to zone '{}'",
                        port, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_port(connection, &config, port, true, context)?;
                    messages.push(format!("Added port '{}' to zone '{}'", port, config.zone));
                    changed = true;
                }
            } else if !should_enable && is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would remove port '{}' from zone '{}'",
                        port, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_port(connection, &config, port, false, context)?;
                    messages.push(format!(
                        "Removed port '{}' from zone '{}'",
                        port, config.zone
                    ));
                    changed = true;
                }
            }
        }

        // Handle source
        if let Some(ref source) = config.source {
            let is_enabled = Self::query_source(connection, &config, source, context)?;

            if should_enable && !is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would add source '{}' to zone '{}'",
                        source, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_source(connection, &config, source, true, context)?;
                    messages.push(format!(
                        "Added source '{}' to zone '{}'",
                        source, config.zone
                    ));
                    changed = true;
                }
            } else if !should_enable && is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would remove source '{}' from zone '{}'",
                        source, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_source(connection, &config, source, false, context)?;
                    messages.push(format!(
                        "Removed source '{}' from zone '{}'",
                        source, config.zone
                    ));
                    changed = true;
                }
            }
        }

        // Handle interface
        if let Some(ref interface) = config.interface {
            let is_enabled = Self::query_interface(connection, &config, interface, context)?;

            if should_enable && !is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would add interface '{}' to zone '{}'",
                        interface, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_interface(connection, &config, interface, true, context)?;
                    messages.push(format!(
                        "Added interface '{}' to zone '{}'",
                        interface, config.zone
                    ));
                    changed = true;
                }
            } else if !should_enable && is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would remove interface '{}' from zone '{}'",
                        interface, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_interface(connection, &config, interface, false, context)?;
                    messages.push(format!(
                        "Removed interface '{}' from zone '{}'",
                        interface, config.zone
                    ));
                    changed = true;
                }
            }
        }

        // Handle masquerade
        if let Some(want_masquerade) = config.masquerade {
            let is_enabled = Self::query_masquerade(connection, &config, context)?;

            if want_masquerade && !is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would enable masquerading on zone '{}'",
                        config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_masquerade(connection, &config, true, context)?;
                    messages.push(format!("Enabled masquerading on zone '{}'", config.zone));
                    changed = true;
                }
            } else if !want_masquerade && is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would disable masquerading on zone '{}'",
                        config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_masquerade(connection, &config, false, context)?;
                    messages.push(format!("Disabled masquerading on zone '{}'", config.zone));
                    changed = true;
                }
            }
        }

        // Handle rich rule
        if let Some(ref rule) = config.rich_rule {
            let is_enabled = Self::query_rich_rule(connection, &config, rule, context)?;

            if should_enable && !is_enabled {
                if context.check_mode {
                    messages.push(format!("Would add rich rule to zone '{}'", config.zone));
                    changed = true;
                } else {
                    Self::manage_rich_rule(connection, &config, rule, true, context)?;
                    messages.push(format!("Added rich rule to zone '{}'", config.zone));
                    changed = true;
                }
            } else if !should_enable && is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would remove rich rule from zone '{}'",
                        config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_rich_rule(connection, &config, rule, false, context)?;
                    messages.push(format!("Removed rich rule from zone '{}'", config.zone));
                    changed = true;
                }
            }
        }

        // Handle ICMP block
        if let Some(ref icmp_type) = config.icmp_block {
            let is_enabled = Self::query_icmp_block(connection, &config, icmp_type, context)?;

            if should_enable && !is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would add ICMP block '{}' to zone '{}'",
                        icmp_type, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_icmp_block(connection, &config, icmp_type, true, context)?;
                    messages.push(format!(
                        "Added ICMP block '{}' to zone '{}'",
                        icmp_type, config.zone
                    ));
                    changed = true;
                }
            } else if !should_enable && is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would remove ICMP block '{}' from zone '{}'",
                        icmp_type, config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_icmp_block(connection, &config, icmp_type, false, context)?;
                    messages.push(format!(
                        "Removed ICMP block '{}' from zone '{}'",
                        icmp_type, config.zone
                    ));
                    changed = true;
                }
            }
        }

        // Handle ICMP block inversion
        if let Some(want_inversion) = config.icmp_block_inversion {
            let is_enabled = Self::query_icmp_block_inversion(connection, &config, context)?;

            if want_inversion && !is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would enable ICMP block inversion on zone '{}'",
                        config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_icmp_block_inversion(connection, &config, true, context)?;
                    messages.push(format!(
                        "Enabled ICMP block inversion on zone '{}'",
                        config.zone
                    ));
                    changed = true;
                }
            } else if !want_inversion && is_enabled {
                if context.check_mode {
                    messages.push(format!(
                        "Would disable ICMP block inversion on zone '{}'",
                        config.zone
                    ));
                    changed = true;
                } else {
                    Self::manage_icmp_block_inversion(connection, &config, false, context)?;
                    messages.push(format!(
                        "Disabled ICMP block inversion on zone '{}'",
                        config.zone
                    ));
                    changed = true;
                }
            }
        }

        // Handle target
        if let Some(ref target) = config.target {
            let current_target = Self::get_zone_target(connection, &config, context)?;
            let target_str = target.as_str();

            if current_target.to_uppercase() != target_str.to_uppercase() {
                if context.check_mode {
                    messages.push(format!(
                        "Would set target to '{}' on zone '{}'",
                        target_str, config.zone
                    ));
                    changed = true;
                } else {
                    Self::set_zone_target(connection, &config, target, context)?;
                    messages.push(format!(
                        "Set target to '{}' on zone '{}'",
                        target_str, config.zone
                    ));
                    changed = true;
                }
            }
        }

        // Reload if permanent changes were made and immediate is requested
        if changed && config.permanent && config.immediate && !context.check_mode {
            Self::reload_firewalld(connection, context)?;
            messages.push("Reloaded firewalld".to_string());
        }

        let msg = if messages.is_empty() {
            format!("Zone '{}' already in desired state", config.zone)
        } else {
            messages.join(". ")
        };

        let output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        Ok(output.with_data("zone", serde_json::json!(config.zone)))
    }
}

/// Validate zone name
fn validate_zone_name(name: &str) -> ModuleResult<()> {
    if name.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Zone name cannot be empty".to_string(),
        ));
    }

    if !ZONE_NAME_REGEX.is_match(name) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid zone name '{}': must start with a letter and contain only alphanumeric characters, underscores, and hyphens",
            name
        )));
    }

    Ok(())
}

/// Validate service name
fn validate_service_name(name: &str) -> ModuleResult<()> {
    if name.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Service name cannot be empty".to_string(),
        ));
    }

    if !SERVICE_NAME_REGEX.is_match(name) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid service name '{}': must start with a letter and contain only alphanumeric characters, underscores, and hyphens",
            name
        )));
    }

    Ok(())
}

/// Validate port specification
fn validate_port(port: &str) -> ModuleResult<()> {
    if !PORT_REGEX.is_match(port) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid port specification '{}': must be in format 'port/protocol' (e.g., '8080/tcp', '53/udp', '8000-9000/tcp')",
            port
        )));
    }

    Ok(())
}

/// Validate source address
fn validate_source(source: &str) -> ModuleResult<()> {
    if !SOURCE_REGEX.is_match(source) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid source address '{}': must be an IP address or CIDR notation",
            source
        )));
    }

    Ok(())
}

/// Validate interface name
fn validate_interface(name: &str) -> ModuleResult<()> {
    if !INTERFACE_REGEX.is_match(name) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid interface name '{}': must start with a letter and contain only alphanumeric characters, dots, underscores, and hyphens",
            name
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firewalld_state_from_str() {
        assert_eq!(
            FirewalldState::from_str("enabled").unwrap(),
            FirewalldState::Enabled
        );
        assert_eq!(
            FirewalldState::from_str("disabled").unwrap(),
            FirewalldState::Disabled
        );
        assert_eq!(
            FirewalldState::from_str("present").unwrap(),
            FirewalldState::Enabled
        );
        assert_eq!(
            FirewalldState::from_str("absent").unwrap(),
            FirewalldState::Disabled
        );
        assert!(FirewalldState::from_str("invalid").is_err());
    }

    #[test]
    fn test_zone_target_from_str() {
        assert_eq!(ZoneTarget::from_str("ACCEPT").unwrap(), ZoneTarget::Accept);
        assert_eq!(ZoneTarget::from_str("DROP").unwrap(), ZoneTarget::Drop);
        assert_eq!(ZoneTarget::from_str("REJECT").unwrap(), ZoneTarget::Reject);
        assert_eq!(
            ZoneTarget::from_str("default").unwrap(),
            ZoneTarget::Default
        );
        assert!(ZoneTarget::from_str("invalid").is_err());
    }

    #[test]
    fn test_validate_zone_name() {
        assert!(validate_zone_name("public").is_ok());
        assert!(validate_zone_name("my-zone").is_ok());
        assert!(validate_zone_name("zone_1").is_ok());
        assert!(validate_zone_name("").is_err());
        assert!(validate_zone_name("123zone").is_err());
        assert!(validate_zone_name("zone;cmd").is_err());
    }

    #[test]
    fn test_validate_service_name() {
        assert!(validate_service_name("http").is_ok());
        assert!(validate_service_name("ssh").is_ok());
        assert!(validate_service_name("my-service").is_ok());
        assert!(validate_service_name("").is_err());
        assert!(validate_service_name("service;cmd").is_err());
    }

    #[test]
    fn test_validate_port() {
        assert!(validate_port("80/tcp").is_ok());
        assert!(validate_port("443/tcp").is_ok());
        assert!(validate_port("53/udp").is_ok());
        assert!(validate_port("8000-9000/tcp").is_ok());
        assert!(validate_port("invalid").is_err());
        assert!(validate_port("80").is_err());
        assert!(validate_port("80/invalid").is_err());
    }

    #[test]
    fn test_validate_source() {
        assert!(validate_source("192.168.1.1").is_ok());
        assert!(validate_source("192.168.1.0/24").is_ok());
        assert!(validate_source("10.0.0.0/8").is_ok());
        assert!(validate_source("invalid").is_err());
    }

    #[test]
    fn test_validate_interface() {
        assert!(validate_interface("eth0").is_ok());
        assert!(validate_interface("enp0s3").is_ok());
        assert!(validate_interface("br-docker").is_ok());
        assert!(validate_interface("").is_err());
        assert!(validate_interface("0eth").is_err());
    }

    #[test]
    fn test_firewalld_module_metadata() {
        let module = FirewalldModule;
        assert_eq!(module.name(), "firewalld");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("with-dash"), "with-dash");
        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
    }

    #[test]
    fn test_config_has_rule() {
        let mut params = ModuleParams::new();
        params.insert("zone".to_string(), serde_json::json!("public"));

        let config = FirewalldConfig::from_params(&params).unwrap();
        assert!(!config.has_rule());

        params.insert("service".to_string(), serde_json::json!("http"));
        let config = FirewalldConfig::from_params(&params).unwrap();
        assert!(config.has_rule());
    }
}
