//! UFW module - Uncomplicated Firewall management
//!
//! This module manages firewall rules using UFW (Ubuntu/Debian).
//! It supports enabling/disabling the firewall, adding/removing rules,
//! and managing application profiles.
//!
//! ## Parameters
//!
//! - `rule`: Rule action (allow, deny, reject, limit)
//! - `direction`: Traffic direction (in, out, routed)
//! - `port`: Port number or range (e.g., "22", "8000:9000")
//! - `proto`: Protocol (tcp, udp, any)
//! - `from_ip`: Source IP address or subnet
//! - `to_ip`: Destination IP address or subnet
//! - `from_port`: Source port
//! - `to_port`: Destination port
//! - `interface`: Network interface (e.g., "eth0")
//! - `interface_in`: Incoming interface for routed rules
//! - `interface_out`: Outgoing interface for routed rules
//! - `route`: Enable routing mode
//! - `app`: Application profile name (e.g., "OpenSSH", "Apache")
//! - `comment`: Rule comment/description
//! - `log`: Enable logging for this rule
//! - `log_level`: Logging level (off, low, medium, high, full)
//! - `insert`: Insert rule at specific position
//! - `insert_relative_to`: Position relative to (first-ipv4, last-ipv4, first-ipv6, last-ipv6)
//! - `state`: Desired state (enabled, disabled, reset, reloaded)
//! - `default`: Default policy (allow, deny, reject) - used with direction
//! - `delete`: Delete the specified rule

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

/// Regex for validating port specifications
static PORT_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d+)(:\d+)?$").expect("Invalid port regex"));

/// Regex for validating IP addresses (IPv4/IPv6 with optional CIDR)
static IP_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(any|\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(/\d{1,2})?|[a-fA-F0-9:]+(/\d{1,3})?)$")
        .expect("Invalid IP regex")
});

/// Regex for validating application names
static APP_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9 _-]*$").expect("Invalid app name regex"));

/// Regex for validating interface names
static INTERFACE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z][a-zA-Z0-9._-]*$").expect("Invalid interface regex"));

/// Rule actions
#[derive(Debug, Clone, PartialEq)]
pub enum UfwRule {
    Allow,
    Deny,
    Reject,
    Limit,
}

impl UfwRule {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "allow" => Ok(UfwRule::Allow),
            "deny" => Ok(UfwRule::Deny),
            "reject" => Ok(UfwRule::Reject),
            "limit" => Ok(UfwRule::Limit),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid rule action '{}'. Valid actions: allow, deny, reject, limit",
                s
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            UfwRule::Allow => "allow",
            UfwRule::Deny => "deny",
            UfwRule::Reject => "reject",
            UfwRule::Limit => "limit",
        }
    }
}

impl std::str::FromStr for UfwRule {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UfwRule::from_str(s)
    }
}

/// Traffic direction
#[derive(Debug, Clone, PartialEq)]
pub enum UfwDirection {
    In,
    Out,
    Routed,
}

impl UfwDirection {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "in" | "incoming" => Ok(UfwDirection::In),
            "out" | "outgoing" => Ok(UfwDirection::Out),
            "routed" | "route" => Ok(UfwDirection::Routed),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid direction '{}'. Valid directions: in, out, routed",
                s
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            UfwDirection::In => "in",
            UfwDirection::Out => "out",
            UfwDirection::Routed => "routed",
        }
    }
}

impl std::str::FromStr for UfwDirection {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UfwDirection::from_str(s)
    }
}

/// Protocol types
#[derive(Debug, Clone, PartialEq)]
pub enum UfwProto {
    Tcp,
    Udp,
    Any,
}

impl UfwProto {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "tcp" => Ok(UfwProto::Tcp),
            "udp" => Ok(UfwProto::Udp),
            "any" | "" => Ok(UfwProto::Any),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid protocol '{}'. Valid protocols: tcp, udp, any",
                s
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            UfwProto::Tcp => "tcp",
            UfwProto::Udp => "udp",
            UfwProto::Any => "any",
        }
    }
}

impl std::str::FromStr for UfwProto {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UfwProto::from_str(s)
    }
}

/// Logging levels
#[derive(Debug, Clone, PartialEq)]
pub enum UfwLogLevel {
    Off,
    Low,
    Medium,
    High,
    Full,
}

impl UfwLogLevel {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "off" => Ok(UfwLogLevel::Off),
            "low" | "on" => Ok(UfwLogLevel::Low),
            "medium" => Ok(UfwLogLevel::Medium),
            "high" => Ok(UfwLogLevel::High),
            "full" => Ok(UfwLogLevel::Full),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid log level '{}'. Valid levels: off, low, medium, high, full",
                s
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            UfwLogLevel::Off => "off",
            UfwLogLevel::Low => "low",
            UfwLogLevel::Medium => "medium",
            UfwLogLevel::High => "high",
            UfwLogLevel::Full => "full",
        }
    }
}

impl std::str::FromStr for UfwLogLevel {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UfwLogLevel::from_str(s)
    }
}

/// UFW state
#[derive(Debug, Clone, PartialEq)]
pub enum UfwState {
    Enabled,
    Disabled,
    Reset,
    Reloaded,
}

impl UfwState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "enabled" => Ok(UfwState::Enabled),
            "disabled" => Ok(UfwState::Disabled),
            "reset" => Ok(UfwState::Reset),
            "reloaded" => Ok(UfwState::Reloaded),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: enabled, disabled, reset, reloaded",
                s
            ))),
        }
    }
}

impl std::str::FromStr for UfwState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UfwState::from_str(s)
    }
}

/// Default policy
#[derive(Debug, Clone, PartialEq)]
pub enum UfwDefault {
    Allow,
    Deny,
    Reject,
}

impl UfwDefault {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "allow" => Ok(UfwDefault::Allow),
            "deny" => Ok(UfwDefault::Deny),
            "reject" => Ok(UfwDefault::Reject),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid default policy '{}'. Valid policies: allow, deny, reject",
                s
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            UfwDefault::Allow => "allow",
            UfwDefault::Deny => "deny",
            UfwDefault::Reject => "reject",
        }
    }
}

impl std::str::FromStr for UfwDefault {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UfwDefault::from_str(s)
    }
}

/// Configuration parsed from module parameters
#[derive(Debug, Clone)]
struct UfwConfig {
    rule: Option<UfwRule>,
    direction: Option<UfwDirection>,
    port: Option<String>,
    proto: Option<UfwProto>,
    from_ip: Option<String>,
    to_ip: Option<String>,
    from_port: Option<String>,
    to_port: Option<String>,
    interface: Option<String>,
    interface_in: Option<String>,
    interface_out: Option<String>,
    route: bool,
    app: Option<String>,
    comment: Option<String>,
    log: Option<bool>,
    log_level: Option<UfwLogLevel>,
    insert: Option<u32>,
    insert_relative_to: Option<String>,
    state: Option<UfwState>,
    default: Option<UfwDefault>,
    delete: bool,
}

impl UfwConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let rule = if let Some(r) = params.get_string("rule")? {
            Some(UfwRule::from_str(&r)?)
        } else {
            None
        };

        let direction = if let Some(d) = params.get_string("direction")? {
            Some(UfwDirection::from_str(&d)?)
        } else {
            None
        };

        let proto = if let Some(p) = params.get_string("proto")? {
            Some(UfwProto::from_str(&p)?)
        } else {
            None
        };

        let log_level = if let Some(l) = params.get_string("log_level")? {
            Some(UfwLogLevel::from_str(&l)?)
        } else {
            None
        };

        let state = if let Some(s) = params.get_string("state")? {
            Some(UfwState::from_str(&s)?)
        } else {
            None
        };

        let default = if let Some(d) = params.get_string("default")? {
            Some(UfwDefault::from_str(&d)?)
        } else {
            None
        };

        let port = params.get_string("port")?;
        if let Some(ref p) = port {
            validate_port(p)?;
        }

        let from_ip = params.get_string("from_ip")?;
        if let Some(ref ip) = from_ip {
            validate_ip(ip)?;
        }

        let to_ip = params.get_string("to_ip")?;
        if let Some(ref ip) = to_ip {
            validate_ip(ip)?;
        }

        let from_port = params.get_string("from_port")?;
        if let Some(ref p) = from_port {
            validate_port(p)?;
        }

        let to_port = params.get_string("to_port")?;
        if let Some(ref p) = to_port {
            validate_port(p)?;
        }

        let interface = params.get_string("interface")?;
        if let Some(ref i) = interface {
            validate_interface(i)?;
        }

        let interface_in = params.get_string("interface_in")?;
        if let Some(ref i) = interface_in {
            validate_interface(i)?;
        }

        let interface_out = params.get_string("interface_out")?;
        if let Some(ref i) = interface_out {
            validate_interface(i)?;
        }

        let app = params.get_string("app")?;
        if let Some(ref a) = app {
            validate_app_name(a)?;
        }

        Ok(Self {
            rule,
            direction,
            port,
            proto,
            from_ip,
            to_ip,
            from_port,
            to_port,
            interface,
            interface_in,
            interface_out,
            route: params.get_bool_or("route", false),
            app,
            comment: params.get_string("comment")?,
            log: params.get_bool("log")?,
            log_level,
            insert: params.get_u32("insert")?,
            insert_relative_to: params.get_string("insert_relative_to")?,
            state,
            default,
            delete: params.get_bool_or("delete", false),
        })
    }

    /// Check if this is a rule operation
    fn is_rule_operation(&self) -> bool {
        self.rule.is_some() || self.app.is_some()
    }

    /// Check if this is a state operation
    fn is_state_operation(&self) -> bool {
        self.state.is_some()
    }

    /// Check if this is a default policy operation
    fn is_default_operation(&self) -> bool {
        self.default.is_some()
    }
}

/// UFW module
pub struct UfwModule;

impl UfwModule {
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
        let connection = connection.clone();
        let command = command.to_string();
        let fut = async move { connection.execute(&command, Some(options)).await };

        let result = if let Ok(handle) = Handle::try_current() {
            std::thread::scope(|s| s.spawn(move || handle.block_on(fut)).join())
                .map_err(|_| {
                    ModuleError::ExecutionFailed("Tokio runtime thread panicked".to_string())
                })?
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to create tokio runtime: {}",
                        e
                    ))
                })?;
            rt.block_on(fut)
        }
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Check if UFW is available
    fn check_ufw_available(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let (success, _, _) =
            Self::execute_command(connection, "command -v ufw >/dev/null 2>&1", context)?;
        Ok(success)
    }

    /// Get UFW status
    fn get_ufw_status(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String)> {
        let (_, stdout, _) = Self::execute_command(connection, "ufw status verbose", context)?;
        let is_active = stdout.contains("Status: active");
        Ok((is_active, stdout))
    }

    /// Enable UFW
    fn enable_ufw(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let (success, _, stderr) =
            Self::execute_command(connection, "ufw --force enable", context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to enable UFW: {}",
                stderr
            )))
        }
    }

    /// Disable UFW
    fn disable_ufw(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let (success, _, stderr) =
            Self::execute_command(connection, "ufw --force disable", context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to disable UFW: {}",
                stderr
            )))
        }
    }

    /// Reset UFW
    fn reset_ufw(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let (success, _, stderr) = Self::execute_command(connection, "ufw --force reset", context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to reset UFW: {}",
                stderr
            )))
        }
    }

    /// Reload UFW
    fn reload_ufw(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let (success, _, stderr) = Self::execute_command(connection, "ufw reload", context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to reload UFW: {}",
                stderr
            )))
        }
    }

    /// Set default policy
    fn set_default(
        connection: &Arc<dyn Connection + Send + Sync>,
        direction: &str,
        policy: &UfwDefault,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = format!("ufw default {} {}", policy.as_str(), direction);
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to set default policy: {}",
                stderr
            )))
        }
    }

    /// Get default policy for a direction
    fn get_default(
        connection: &Arc<dyn Connection + Send + Sync>,
        direction: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        let (_, stdout) = Self::get_ufw_status(connection, context)?;

        // Parse the status output to find default policy
        for line in stdout.lines() {
            let line_lower: String = line.to_lowercase();
            if line_lower.contains(&format!("default: {} (", direction.to_lowercase())) {
                // Extract policy from "Default: incoming (deny), outgoing (allow)"
                if let Some(start) = line_lower.find(&format!("{} (", direction.to_lowercase())) {
                    let after = &line[start..];
                    if let Some(paren_start) = after.find('(') {
                        if let Some(paren_end) = after.find(')') {
                            let policy = &after[paren_start + 1..paren_end];
                            return Ok(Some(policy.to_string()));
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    /// Build the rule command
    fn build_rule_cmd(config: &UfwConfig, delete: bool) -> String {
        let mut parts = Vec::new();
        parts.push("ufw".to_string());

        // Delete prefix
        if delete {
            parts.push("delete".to_string());
        }

        // Insert position
        if let Some(pos) = config.insert {
            if !delete {
                parts.push(format!("insert {}", pos));
            }
        }

        // Route
        if config.route || config.direction == Some(UfwDirection::Routed) {
            parts.push("route".to_string());
        }

        // Rule action
        if let Some(ref rule) = config.rule {
            parts.push(rule.as_str().to_string());
        }

        // Direction
        if let Some(ref direction) = config.direction {
            if *direction != UfwDirection::Routed {
                parts.push(direction.as_str().to_string());
            }
        }

        // Interface
        if let Some(ref iface) = config.interface {
            parts.push(format!("on {}", shell_escape(iface)));
        }
        if let Some(ref iface_in) = config.interface_in {
            parts.push(format!("in on {}", shell_escape(iface_in)));
        }
        if let Some(ref iface_out) = config.interface_out {
            parts.push(format!("out on {}", shell_escape(iface_out)));
        }

        // Logging
        if let Some(ref log_level) = config.log_level {
            parts.push(format!("log-{}", log_level.as_str()));
        } else if config.log == Some(true) {
            parts.push("log".to_string());
        }

        // From
        if let Some(ref from) = config.from_ip {
            parts.push(format!("from {}", shell_escape(from)));
            if let Some(ref port) = config.from_port {
                parts.push(format!("port {}", shell_escape(port)));
            }
        }

        // To
        if let Some(ref to) = config.to_ip {
            parts.push(format!("to {}", shell_escape(to)));
        }

        // Application profile
        if let Some(ref app) = config.app {
            parts.push(format!("app {}", shell_escape(app)));
        } else if let Some(ref port) = config.port {
            // Port (only if no app specified)
            if let Some(ref _to) = config.to_ip {
                // Already have 'to', add port
                parts.push(format!("port {}", shell_escape(port)));
            } else if config.from_ip.is_some() {
                // Have 'from', need 'to any port'
                parts.push(format!("to any port {}", shell_escape(port)));
            } else {
                // Standalone port
                parts.push(shell_escape(port).into_owned());
            }
        }

        // To port (when different from main port)
        if let Some(ref to_port) = config.to_port {
            if config.port.is_none() {
                parts.push(format!("port {}", shell_escape(to_port)));
            }
        }

        // Protocol
        if let Some(ref proto) = config.proto {
            if *proto != UfwProto::Any {
                parts.push(format!("proto {}", proto.as_str()));
            }
        }

        // Comment
        if let Some(ref comment) = config.comment {
            if !delete {
                parts.push(format!("comment {}", shell_escape(comment)));
            }
        }

        parts.join(" ")
    }

    /// Check if a rule exists
    fn rule_exists(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &UfwConfig,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let (_, status) = Self::get_ufw_status(connection, context)?;

        // Build a pattern to match the rule
        let mut pattern_parts = Vec::new();

        if let Some(ref port) = config.port {
            pattern_parts.push(port.clone());
        }

        if let Some(ref proto) = config.proto {
            if *proto != UfwProto::Any {
                pattern_parts.push(proto.as_str().to_uppercase());
            }
        }

        if let Some(ref rule) = config.rule {
            pattern_parts.push(rule.as_str().to_uppercase());
        }

        if let Some(ref from) = config.from_ip {
            if from != "any" {
                pattern_parts.push(from.clone());
            }
        }

        if let Some(ref to) = config.to_ip {
            if to != "any" {
                pattern_parts.push(to.clone());
            }
        }

        if let Some(ref app) = config.app {
            pattern_parts.push(app.clone());
        }

        // Check if all pattern parts are found in the same line
        for line in status.lines() {
            let line_upper: String = line.to_uppercase();
            let all_match = pattern_parts
                .iter()
                .all(|p| line_upper.contains(&p.to_uppercase()));
            if all_match && !pattern_parts.is_empty() {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Add a rule
    fn add_rule(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &UfwConfig,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = Self::build_rule_cmd(config, false);
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success || stderr.contains("Skipping") || stderr.contains("existing") {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to add UFW rule: {}",
                stderr
            )))
        }
    }

    /// Delete a rule
    fn delete_rule(
        connection: &Arc<dyn Connection + Send + Sync>,
        config: &UfwConfig,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = Self::build_rule_cmd(config, true);
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success || stderr.contains("not found") || stderr.contains("Could not delete") {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to delete UFW rule: {}",
                stderr
            )))
        }
    }

    /// Set logging level
    fn set_logging(
        connection: &Arc<dyn Connection + Send + Sync>,
        level: &UfwLogLevel,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = format!("ufw logging {}", level.as_str());
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to set UFW logging: {}",
                stderr
            )))
        }
    }
}

impl Module for UfwModule {
    fn name(&self) -> &'static str {
        "ufw"
    }

    fn description(&self) -> &'static str {
        "Manage firewall rules using UFW (Uncomplicated Firewall)"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::HostExclusive
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "UFW module requires a connection for remote execution".to_string(),
            )
        })?;

        let config = UfwConfig::from_params(params)?;

        // Check if UFW is available
        if !Self::check_ufw_available(connection, context)? {
            return Err(ModuleError::ExecutionFailed(
                "UFW is not available on this system".to_string(),
            ));
        }

        let mut changed = false;
        let mut messages = Vec::new();

        // Handle state changes (enable/disable/reset/reload)
        if config.is_state_operation() {
            let state = config.state.as_ref().unwrap();
            let (is_active, _) = Self::get_ufw_status(connection, context)?;

            match state {
                UfwState::Enabled => {
                    if !is_active {
                        if context.check_mode {
                            messages.push("Would enable UFW".to_string());
                            changed = true;
                        } else {
                            Self::enable_ufw(connection, context)?;
                            messages.push("Enabled UFW".to_string());
                            changed = true;
                        }
                    } else {
                        messages.push("UFW is already enabled".to_string());
                    }
                }
                UfwState::Disabled => {
                    if is_active {
                        if context.check_mode {
                            messages.push("Would disable UFW".to_string());
                            changed = true;
                        } else {
                            Self::disable_ufw(connection, context)?;
                            messages.push("Disabled UFW".to_string());
                            changed = true;
                        }
                    } else {
                        messages.push("UFW is already disabled".to_string());
                    }
                }
                UfwState::Reset => {
                    if context.check_mode {
                        messages.push("Would reset UFW".to_string());
                        changed = true;
                    } else {
                        Self::reset_ufw(connection, context)?;
                        messages.push("Reset UFW".to_string());
                        changed = true;
                    }
                }
                UfwState::Reloaded => {
                    if context.check_mode {
                        messages.push("Would reload UFW".to_string());
                        changed = true;
                    } else {
                        Self::reload_ufw(connection, context)?;
                        messages.push("Reloaded UFW".to_string());
                        changed = true;
                    }
                }
            }
        }

        // Handle default policy changes
        if config.is_default_operation() {
            let policy = config.default.as_ref().unwrap();
            let direction = config
                .direction
                .as_ref()
                .map(|d| d.as_str())
                .unwrap_or("incoming");

            let current = Self::get_default(connection, direction, context)?;
            let target = policy.as_str();

            if current.as_deref() != Some(target) {
                if context.check_mode {
                    messages.push(format!(
                        "Would set default {} policy to {}",
                        direction, target
                    ));
                    changed = true;
                } else {
                    Self::set_default(connection, direction, policy, context)?;
                    messages.push(format!("Set default {} policy to {}", direction, target));
                    changed = true;
                }
            } else {
                messages.push(format!(
                    "Default {} policy is already {}",
                    direction, target
                ));
            }
        }

        // Handle rule operations
        if config.is_rule_operation() {
            let exists = Self::rule_exists(connection, &config, context)?;

            if config.delete {
                if exists {
                    if context.check_mode {
                        messages.push("Would delete UFW rule".to_string());
                        changed = true;
                    } else {
                        Self::delete_rule(connection, &config, context)?;
                        messages.push("Deleted UFW rule".to_string());
                        changed = true;
                    }
                } else {
                    messages.push("Rule does not exist".to_string());
                }
            } else if !exists {
                if context.check_mode {
                    messages.push("Would add UFW rule".to_string());
                    changed = true;
                } else {
                    Self::add_rule(connection, &config, context)?;
                    messages.push("Added UFW rule".to_string());
                    changed = true;
                }
            } else {
                messages.push("Rule already exists".to_string());
            }
        }

        // Handle global logging level
        if let Some(ref log_level) = config.log_level {
            if !config.is_rule_operation() {
                if context.check_mode {
                    messages.push(format!("Would set logging to {}", log_level.as_str()));
                    changed = true;
                } else {
                    Self::set_logging(connection, log_level, context)?;
                    messages.push(format!("Set logging to {}", log_level.as_str()));
                    changed = true;
                }
            }
        }

        // Validate that at least one operation was specified
        if messages.is_empty() {
            return Err(ModuleError::MissingParameter(
                "At least one of state, default, or rule must be specified".to_string(),
            ));
        }

        let msg = messages.join(". ");
        let output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        Ok(output)
    }
}

/// Validate port specification
fn validate_port(port: &str) -> ModuleResult<()> {
    if !PORT_REGEX.is_match(port) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid port specification '{}': must be a port number or range (e.g., '22' or '8000:9000')",
            port
        )));
    }
    Ok(())
}

/// Validate IP address
fn validate_ip(ip: &str) -> ModuleResult<()> {
    if !IP_REGEX.is_match(ip) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid IP address '{}': must be 'any' or a valid IPv4/IPv6 address with optional CIDR",
            ip
        )));
    }
    Ok(())
}

/// Validate application name
fn validate_app_name(name: &str) -> ModuleResult<()> {
    if !APP_NAME_REGEX.is_match(name) {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid application name '{}': must start with a letter and contain only alphanumeric characters, spaces, underscores, and hyphens",
            name
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
    fn test_ufw_rule_from_str() {
        assert_eq!(UfwRule::from_str("allow").unwrap(), UfwRule::Allow);
        assert_eq!(UfwRule::from_str("deny").unwrap(), UfwRule::Deny);
        assert_eq!(UfwRule::from_str("reject").unwrap(), UfwRule::Reject);
        assert_eq!(UfwRule::from_str("limit").unwrap(), UfwRule::Limit);
        assert!(UfwRule::from_str("invalid").is_err());
    }

    #[test]
    fn test_ufw_direction_from_str() {
        assert_eq!(UfwDirection::from_str("in").unwrap(), UfwDirection::In);
        assert_eq!(UfwDirection::from_str("out").unwrap(), UfwDirection::Out);
        assert_eq!(
            UfwDirection::from_str("routed").unwrap(),
            UfwDirection::Routed
        );
        assert!(UfwDirection::from_str("invalid").is_err());
    }

    #[test]
    fn test_ufw_proto_from_str() {
        assert_eq!(UfwProto::from_str("tcp").unwrap(), UfwProto::Tcp);
        assert_eq!(UfwProto::from_str("udp").unwrap(), UfwProto::Udp);
        assert_eq!(UfwProto::from_str("any").unwrap(), UfwProto::Any);
        assert!(UfwProto::from_str("invalid").is_err());
    }

    #[test]
    fn test_ufw_state_from_str() {
        assert_eq!(UfwState::from_str("enabled").unwrap(), UfwState::Enabled);
        assert_eq!(UfwState::from_str("disabled").unwrap(), UfwState::Disabled);
        assert_eq!(UfwState::from_str("reset").unwrap(), UfwState::Reset);
        assert_eq!(UfwState::from_str("reloaded").unwrap(), UfwState::Reloaded);
        assert!(UfwState::from_str("invalid").is_err());
    }

    #[test]
    fn test_ufw_default_from_str() {
        assert_eq!(UfwDefault::from_str("allow").unwrap(), UfwDefault::Allow);
        assert_eq!(UfwDefault::from_str("deny").unwrap(), UfwDefault::Deny);
        assert_eq!(UfwDefault::from_str("reject").unwrap(), UfwDefault::Reject);
        assert!(UfwDefault::from_str("invalid").is_err());
    }

    #[test]
    fn test_validate_port() {
        assert!(validate_port("22").is_ok());
        assert!(validate_port("8000:9000").is_ok());
        assert!(validate_port("443").is_ok());
        assert!(validate_port("invalid").is_err());
        assert!(validate_port("22/tcp").is_err()); // UFW uses different format
    }

    #[test]
    fn test_validate_ip() {
        assert!(validate_ip("192.168.1.1").is_ok());
        assert!(validate_ip("192.168.1.0/24").is_ok());
        assert!(validate_ip("any").is_ok());
        assert!(validate_ip("invalid").is_err());
    }

    #[test]
    fn test_validate_app_name() {
        assert!(validate_app_name("OpenSSH").is_ok());
        assert!(validate_app_name("Apache Full").is_ok());
        assert!(validate_app_name("Nginx HTTP").is_ok());
        assert!(validate_app_name("").is_err());
        assert!(validate_app_name("123app").is_err());
    }

    #[test]
    fn test_validate_interface() {
        assert!(validate_interface("eth0").is_ok());
        assert!(validate_interface("enp0s3").is_ok());
        assert!(validate_interface("wlan0").is_ok());
        assert!(validate_interface("").is_err());
        assert!(validate_interface("0eth").is_err());
    }

    #[test]
    fn test_ufw_module_metadata() {
        let module = UfwModule;
        assert_eq!(module.name(), "ufw");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }

    #[test]
    fn test_build_rule_cmd_simple() {
        let mut params = ModuleParams::new();
        params.insert("rule".to_string(), serde_json::json!("allow"));
        params.insert("port".to_string(), serde_json::json!("22"));

        let config = UfwConfig::from_params(&params).unwrap();
        let cmd = UfwModule::build_rule_cmd(&config, false);

        assert!(cmd.contains("ufw"));
        assert!(cmd.contains("allow"));
        assert!(cmd.contains("22"));
    }

    #[test]
    fn test_build_rule_cmd_with_from() {
        let mut params = ModuleParams::new();
        params.insert("rule".to_string(), serde_json::json!("allow"));
        params.insert("from_ip".to_string(), serde_json::json!("192.168.1.0/24"));
        params.insert("port".to_string(), serde_json::json!("22"));
        params.insert("proto".to_string(), serde_json::json!("tcp"));

        let config = UfwConfig::from_params(&params).unwrap();
        let cmd = UfwModule::build_rule_cmd(&config, false);

        assert!(cmd.contains("from 192.168.1.0/24"));
        assert!(cmd.contains("proto tcp"));
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("with-dash"), "with-dash");
        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
    }
}
