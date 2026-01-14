//! SELinux module - SELinux policy and context management
//!
//! This module provides comprehensive SELinux management capabilities including:
//!
//! - **Mode Management**: Set SELinux mode (enforcing, permissive, disabled)
//! - **Boolean Management**: Manage SELinux boolean values (on/off)
//! - **Context Management**: Set file/directory security contexts
//! - **Port Management**: Manage SELinux port type definitions
//!
//! ## Parameters
//!
//! ### Common Parameters
//! - `policy`: SELinux policy name (e.g., "targeted", "mls")
//! - `configfile`: Path to SELinux config file (default: /etc/selinux/config)
//!
//! ### Mode Management
//! - `state`: SELinux mode (enforcing, permissive, disabled)
//!
//! ### Boolean Management
//! - `boolean`: Boolean name to manage
//! - `boolean_state`: Boolean state (on/off, true/false, 1/0)
//! - `persistent`: Make boolean change persistent across reboots
//!
//! ### Context Management
//! - `target`: Target file/directory path for context operations
//! - `setype`: SELinux type (e.g., "httpd_sys_content_t")
//! - `seuser`: SELinux user (e.g., "system_u")
//! - `selevel`: SELinux level/range (e.g., "s0")
//! - `serole`: SELinux role (e.g., "object_r")
//! - `ftype`: File type for fcontext (a, f, d, c, b, s, l, p)
//! - `reload`: Reload policy after context change
//!
//! ### Port Management
//! - `ports`: Port(s) to manage (single, range "1000-2000", or list)
//! - `proto`: Protocol (tcp, udp, dccp, sctp)
//! - `port_type`: SELinux port type (e.g., "http_port_t")
//! - `port_state`: Port state (present, absent)

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

/// Regex pattern for validating SELinux type names
static SELINUX_TYPE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*_t$").expect("Invalid SELinux type regex"));

/// Regex pattern for validating SELinux user names
static SELINUX_USER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*_u$").expect("Invalid SELinux user regex"));

/// Regex pattern for validating SELinux role names
static SELINUX_ROLE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*_r$").expect("Invalid SELinux role regex"));

/// Regex pattern for validating SELinux boolean names
static SELINUX_BOOLEAN_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").expect("Invalid SELinux boolean regex"));

/// Regex pattern for validating port numbers
static PORT_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[0-9]+(-[0-9]+)?$").expect("Invalid port regex"));

/// SELinux enforcement modes
#[derive(Debug, Clone, PartialEq)]
pub enum SELinuxMode {
    /// Fully enforcing SELinux policy
    Enforcing,
    /// Log violations but don't enforce
    Permissive,
    /// SELinux completely disabled
    Disabled,
}

impl SELinuxMode {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "enforcing" | "1" => Ok(SELinuxMode::Enforcing),
            "permissive" | "0" => Ok(SELinuxMode::Permissive),
            "disabled" => Ok(SELinuxMode::Disabled),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid SELinux state '{}'. Valid states: enforcing, permissive, disabled",
                s
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SELinuxMode::Enforcing => "enforcing",
            SELinuxMode::Permissive => "permissive",
            SELinuxMode::Disabled => "disabled",
        }
    }

    fn as_setenforce(&self) -> Option<&'static str> {
        match self {
            SELinuxMode::Enforcing => Some("1"),
            SELinuxMode::Permissive => Some("0"),
            SELinuxMode::Disabled => None, // Can't use setenforce for disabled
        }
    }
}

/// SELinux port protocol
#[derive(Debug, Clone, PartialEq)]
pub enum SELinuxProtocol {
    Tcp,
    Udp,
    Dccp,
    Sctp,
}

impl SELinuxProtocol {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "tcp" => Ok(SELinuxProtocol::Tcp),
            "udp" => Ok(SELinuxProtocol::Udp),
            "dccp" => Ok(SELinuxProtocol::Dccp),
            "sctp" => Ok(SELinuxProtocol::Sctp),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid protocol '{}'. Valid protocols: tcp, udp, dccp, sctp",
                s
            ))),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SELinuxProtocol::Tcp => "tcp",
            SELinuxProtocol::Udp => "udp",
            SELinuxProtocol::Dccp => "dccp",
            SELinuxProtocol::Sctp => "sctp",
        }
    }
}

/// SELinux file type for fcontext
#[derive(Debug, Clone, PartialEq)]
pub enum SELinuxFileType {
    /// All files (default)
    All,
    /// Regular files
    File,
    /// Directories
    Directory,
    /// Character devices
    CharDevice,
    /// Block devices
    BlockDevice,
    /// Sockets
    Socket,
    /// Symbolic links
    SymLink,
    /// Named pipes (FIFOs)
    Pipe,
}

impl SELinuxFileType {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "a" | "all" => Ok(SELinuxFileType::All),
            "f" | "file" | "" => Ok(SELinuxFileType::File),
            "d" | "directory" | "dir" => Ok(SELinuxFileType::Directory),
            "c" | "char" | "character" => Ok(SELinuxFileType::CharDevice),
            "b" | "block" => Ok(SELinuxFileType::BlockDevice),
            "s" | "socket" | "sock" => Ok(SELinuxFileType::Socket),
            "l" | "link" | "symlink" => Ok(SELinuxFileType::SymLink),
            "p" | "pipe" | "fifo" => Ok(SELinuxFileType::Pipe),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid file type '{}'. Valid types: a (all), f (file), d (directory), c (char), b (block), s (socket), l (symlink), p (pipe)",
                s
            ))),
        }
    }

    fn as_semanage_arg(&self) -> &'static str {
        match self {
            SELinuxFileType::All => "a",
            SELinuxFileType::File => "f",
            SELinuxFileType::Directory => "d",
            SELinuxFileType::CharDevice => "c",
            SELinuxFileType::BlockDevice => "b",
            SELinuxFileType::Socket => "s",
            SELinuxFileType::SymLink => "l",
            SELinuxFileType::Pipe => "p",
        }
    }
}

/// SELinux port state
#[derive(Debug, Clone, PartialEq)]
pub enum PortState {
    Present,
    Absent,
}

impl PortState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(PortState::Present),
            "absent" => Ok(PortState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid port state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Operation type for the SELinux module
#[derive(Debug, Clone, PartialEq)]
enum SELinuxOperation {
    /// Set SELinux mode (enforcing/permissive/disabled)
    Mode,
    /// Manage SELinux boolean
    Boolean,
    /// Manage file context
    Context,
    /// Manage port type
    Port,
}

/// Module for SELinux management
pub struct SELinuxModule;

impl SELinuxModule {
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

    /// Check if SELinux is available on the system
    fn is_selinux_available(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = "which getenforce >/dev/null 2>&1 && echo yes || echo no";
        let (_, stdout, _) = Self::execute_command(connection, cmd, context)?;
        Ok(stdout.trim() == "yes")
    }

    /// Get current SELinux mode
    fn get_current_mode(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<SELinuxMode> {
        let (success, stdout, stderr) = Self::execute_command(connection, "getenforce", context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to get SELinux mode: {}",
                stderr
            )));
        }

        SELinuxMode::from_str(stdout.trim())
    }

    /// Get configured SELinux mode from config file
    fn get_configured_mode(
        connection: &Arc<dyn Connection + Send + Sync>,
        configfile: &str,
        context: &ModuleContext,
    ) -> ModuleResult<SELinuxMode> {
        let cmd = format!(
            "grep -E '^SELINUX=' {} 2>/dev/null | cut -d= -f2 | tr -d '[:space:]'",
            shell_escape(configfile)
        );
        let (success, stdout, _) = Self::execute_command(connection, &cmd, context)?;

        if !success || stdout.trim().is_empty() {
            // Default to enforcing if config not found
            return Ok(SELinuxMode::Enforcing);
        }

        SELinuxMode::from_str(stdout.trim())
    }

    /// Set SELinux mode at runtime using setenforce
    fn set_runtime_mode(
        connection: &Arc<dyn Connection + Send + Sync>,
        mode: &SELinuxMode,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        if let Some(value) = mode.as_setenforce() {
            let cmd = format!("setenforce {}", value);
            let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to set SELinux mode: {}",
                    stderr
                )));
            }
        }
        Ok(())
    }

    /// Update SELinux config file
    fn update_config_file(
        connection: &Arc<dyn Connection + Send + Sync>,
        configfile: &str,
        mode: &SELinuxMode,
        policy: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Read current config
        let cmd = format!("cat {} 2>/dev/null || true", shell_escape(configfile));
        let (_, content, _) = Self::execute_command(connection, &cmd, context)?;

        let mut new_lines: Vec<String> = Vec::new();
        let mut found_selinux = false;
        let mut found_selinuxtype = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("SELINUX=") {
                new_lines.push(format!("SELINUX={}", mode.as_str()));
                found_selinux = true;
            } else if trimmed.starts_with("SELINUXTYPE=") && policy.is_some() {
                new_lines.push(format!("SELINUXTYPE={}", policy.unwrap()));
                found_selinuxtype = true;
            } else {
                new_lines.push(line.to_string());
            }
        }

        // Add missing entries
        if !found_selinux {
            new_lines.push(format!("SELINUX={}", mode.as_str()));
        }
        if !found_selinuxtype && policy.is_some() {
            new_lines.push(format!("SELINUXTYPE={}", policy.unwrap()));
        }

        let new_content = new_lines.join("\n");
        let cmd = format!(
            "cat << 'RUSTIBLE_EOF' > {}\n{}\nRUSTIBLE_EOF",
            shell_escape(configfile),
            new_content.trim()
        );
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to update {}: {}",
                configfile, stderr
            )));
        }

        Ok(())
    }

    /// Get current value of an SELinux boolean
    fn get_boolean_value(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<bool>> {
        let cmd = format!("getsebool {} 2>/dev/null", shell_escape(name));
        let (success, stdout, _) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Ok(None);
        }

        // Output format: "boolean_name --> on" or "boolean_name --> off"
        if stdout.contains("--> on") {
            Ok(Some(true))
        } else if stdout.contains("--> off") {
            Ok(Some(false))
        } else {
            Ok(None)
        }
    }

    /// Set SELinux boolean value
    fn set_boolean_value(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        value: bool,
        persistent: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let value_str = if value { "on" } else { "off" };
        let persistent_flag = if persistent { "-P " } else { "" };

        let cmd = format!(
            "setsebool {}{} {}",
            persistent_flag,
            shell_escape(name),
            value_str
        );
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to set boolean '{}': {}",
                name, stderr
            )));
        }

        Ok(())
    }

    /// Get current file context
    fn get_file_context(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<(String, String, String, String)>> {
        // Use ls -Z to get context
        let cmd = format!(
            "ls -Zd {} 2>/dev/null | awk '{{print $1}}'",
            shell_escape(path)
        );
        let (success, stdout, _) = Self::execute_command(connection, &cmd, context)?;

        if !success || stdout.trim().is_empty() || stdout.trim() == "?" {
            return Ok(None);
        }

        // Parse context format: user:role:type:level
        let context_str = stdout.trim();
        let parts: Vec<&str> = context_str.split(':').collect();

        if parts.len() >= 4 {
            Ok(Some((
                parts[0].to_string(),
                parts[1].to_string(),
                parts[2].to_string(),
                parts[3..].join(":"), // level can contain colons
            )))
        } else if parts.len() == 3 {
            // Some systems don't show level
            Ok(Some((
                parts[0].to_string(),
                parts[1].to_string(),
                parts[2].to_string(),
                String::new(),
            )))
        } else {
            Ok(None)
        }
    }

    /// Set file context using chcon
    fn set_file_context(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        seuser: Option<&str>,
        serole: Option<&str>,
        setype: Option<&str>,
        selevel: Option<&str>,
        recursive: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let mut args = Vec::new();

        if recursive {
            args.push("-R".to_string());
        }

        if let Some(u) = seuser {
            args.push(format!("-u {}", shell_escape(u)));
        }
        if let Some(r) = serole {
            args.push(format!("-r {}", shell_escape(r)));
        }
        if let Some(t) = setype {
            args.push(format!("-t {}", shell_escape(t)));
        }
        if let Some(l) = selevel {
            args.push(format!("-l {}", shell_escape(l)));
        }

        if args.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "At least one of seuser, serole, setype, or selevel must be specified".to_string(),
            ));
        }

        let cmd = format!("chcon {} {}", args.join(" "), shell_escape(path));
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to set context for '{}': {}",
                path, stderr
            )));
        }

        Ok(())
    }

    /// Add or modify file context in policy using semanage fcontext
    fn manage_fcontext(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        setype: &str,
        ftype: &SELinuxFileType,
        present: bool,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        // Check if fcontext already exists
        let cmd = format!(
            "semanage fcontext -l 2>/dev/null | grep -F '{}' || true",
            shell_escape(path)
        );
        let (_, stdout, _) = Self::execute_command(connection, &cmd, context)?;

        let exists = !stdout.trim().is_empty();
        let has_correct_type = stdout.contains(setype);

        if present {
            if exists && has_correct_type {
                return Ok(false); // No change needed
            }

            let action = if exists { "-m" } else { "-a" };
            let cmd = format!(
                "semanage fcontext {} -t {} -f {} '{}'",
                action,
                shell_escape(setype),
                ftype.as_semanage_arg(),
                path
            );
            let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to manage fcontext: {}",
                    stderr
                )));
            }

            Ok(true)
        } else {
            if !exists {
                return Ok(false); // Already absent
            }

            let cmd = format!("semanage fcontext -d '{}'", path);
            let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

            if !success && !stderr.contains("does not exist") {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to remove fcontext: {}",
                    stderr
                )));
            }

            Ok(true)
        }
    }

    /// Restore file context using restorecon
    fn restore_context(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        recursive: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let recursive_flag = if recursive { "-R" } else { "" };
        let cmd = format!("restorecon -v {} {}", recursive_flag, shell_escape(path));
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to restore context: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Check if port type is defined
    fn get_port_type(
        connection: &Arc<dyn Connection + Send + Sync>,
        port: &str,
        proto: &SELinuxProtocol,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        let cmd = format!(
            "semanage port -l 2>/dev/null | grep -E '{}[[:space:]]+{}' | awk '{{print $1}}' | head -1",
            proto.as_str(),
            port
        );
        let (success, stdout, _) = Self::execute_command(connection, &cmd, context)?;

        if !success || stdout.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(stdout.trim().to_string()))
        }
    }

    /// Manage SELinux port type
    fn manage_port(
        connection: &Arc<dyn Connection + Send + Sync>,
        port: &str,
        proto: &SELinuxProtocol,
        port_type: &str,
        state: &PortState,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let current_type = Self::get_port_type(connection, port, proto, context)?;

        match state {
            PortState::Present => {
                if current_type.as_deref() == Some(port_type) {
                    return Ok(false); // Already correct
                }

                let action = if current_type.is_some() { "-m" } else { "-a" };
                let cmd = format!(
                    "semanage port {} -t {} -p {} {}",
                    action,
                    shell_escape(port_type),
                    proto.as_str(),
                    port
                );
                let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

                if !success {
                    // Check if already defined by policy
                    if stderr.contains("already defined") {
                        // Try modify instead
                        let cmd = format!(
                            "semanage port -m -t {} -p {} {}",
                            shell_escape(port_type),
                            proto.as_str(),
                            port
                        );
                        let (success2, _, stderr2) =
                            Self::execute_command(connection, &cmd, context)?;
                        if !success2 {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to manage port: {}",
                                stderr2
                            )));
                        }
                    } else {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to add port: {}",
                            stderr
                        )));
                    }
                }

                Ok(true)
            }
            PortState::Absent => {
                if current_type.is_none() {
                    return Ok(false); // Already absent
                }

                let cmd = format!("semanage port -d -p {} {}", proto.as_str(), port);
                let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

                if !success
                    && !stderr.contains("does not exist")
                    && !stderr.contains("is defined in policy")
                {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to remove port: {}",
                        stderr
                    )));
                }

                Ok(success)
            }
        }
    }

    /// Determine which operation to perform based on parameters
    fn determine_operation(params: &ModuleParams) -> ModuleResult<SELinuxOperation> {
        let has_state = params.get("state").is_some();
        let has_boolean = params.get("boolean").is_some();
        let has_target = params.get("target").is_some();
        let has_ports = params.get("ports").is_some();

        if has_boolean {
            Ok(SELinuxOperation::Boolean)
        } else if has_target {
            Ok(SELinuxOperation::Context)
        } else if has_ports {
            Ok(SELinuxOperation::Port)
        } else if has_state {
            Ok(SELinuxOperation::Mode)
        } else {
            Err(ModuleError::InvalidParameter(
                "Must specify one of: state (for mode), boolean, target (for context), or ports"
                    .to_string(),
            ))
        }
    }

    /// Execute mode management operation
    fn execute_mode(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: &Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let state_str = params.get_string_required("state")?;
        let desired_mode = SELinuxMode::from_str(&state_str)?;
        let configfile = params
            .get_string("configfile")?
            .unwrap_or_else(|| "/etc/selinux/config".to_string());
        let policy = params.get_string("policy")?;

        let current_mode = Self::get_current_mode(connection, context)?;
        let configured_mode = Self::get_configured_mode(connection, &configfile, context)?;

        let runtime_needs_change = current_mode != desired_mode;
        let config_needs_change = configured_mode != desired_mode;

        if !runtime_needs_change && !config_needs_change {
            return Ok(ModuleOutput::ok(format!(
                "SELinux is already in '{}' mode",
                desired_mode.as_str()
            ))
            .with_data("mode", serde_json::json!(desired_mode.as_str()))
            .with_data("reboot_required", serde_json::json!(false)));
        }

        if context.check_mode {
            let mut msg = format!(
                "Would change SELinux mode from '{}' to '{}'",
                current_mode.as_str(),
                desired_mode.as_str()
            );
            if desired_mode == SELinuxMode::Disabled || current_mode == SELinuxMode::Disabled {
                msg.push_str(" (requires reboot)");
            }
            return Ok(ModuleOutput::changed(msg));
        }

        let mut messages = Vec::new();
        let mut reboot_required = false;

        // Update config file first
        if config_needs_change {
            Self::update_config_file(
                connection,
                &configfile,
                &desired_mode,
                policy.as_deref(),
                context,
            )?;
            messages.push(format!(
                "Updated {} with mode '{}'",
                configfile,
                desired_mode.as_str()
            ));
        }

        // Try to change runtime mode if not switching to/from disabled
        if runtime_needs_change {
            if current_mode == SELinuxMode::Disabled || desired_mode == SELinuxMode::Disabled {
                reboot_required = true;
                messages.push("Reboot required to complete mode change".to_string());
            } else {
                Self::set_runtime_mode(connection, &desired_mode, context)?;
                messages.push(format!(
                    "Changed runtime mode from '{}' to '{}'",
                    current_mode.as_str(),
                    desired_mode.as_str()
                ));
            }
        }

        Ok(ModuleOutput::changed(messages.join(". "))
            .with_data("mode", serde_json::json!(desired_mode.as_str()))
            .with_data("previous_mode", serde_json::json!(current_mode.as_str()))
            .with_data("reboot_required", serde_json::json!(reboot_required)))
    }

    /// Execute boolean management operation
    fn execute_boolean(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: &Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let boolean_name = params.get_string_required("boolean")?;
        Self::validate_boolean_name(&boolean_name)?;

        let state = params
            .get_string("boolean_state")
            .ok()
            .flatten()
            .or_else(|| params.get_string("state").ok().flatten());

        let desired_value = match state.as_deref() {
            Some("on") | Some("true") | Some("1") | Some("yes") => true,
            Some("off") | Some("false") | Some("0") | Some("no") => false,
            None => {
                return Err(ModuleError::MissingParameter(
                    "boolean_state or state is required for boolean operations".to_string(),
                ))
            }
            Some(s) => {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid boolean state '{}'. Valid states: on, off, true, false, 1, 0",
                    s
                )))
            }
        };

        let persistent = params.get_bool_or("persistent", true);

        let current_value = Self::get_boolean_value(connection, &boolean_name, context)?;

        if current_value.is_none() {
            return Err(ModuleError::ExecutionFailed(format!(
                "SELinux boolean '{}' not found",
                boolean_name
            )));
        }

        let current = current_value.unwrap();
        if current == desired_value {
            return Ok(ModuleOutput::ok(format!(
                "Boolean '{}' is already {}",
                boolean_name,
                if desired_value { "on" } else { "off" }
            ))
            .with_data("name", serde_json::json!(boolean_name))
            .with_data("value", serde_json::json!(desired_value))
            .with_data("persistent", serde_json::json!(persistent)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would set boolean '{}' to {}",
                boolean_name,
                if desired_value { "on" } else { "off" }
            )));
        }

        Self::set_boolean_value(
            connection,
            &boolean_name,
            desired_value,
            persistent,
            context,
        )?;

        Ok(ModuleOutput::changed(format!(
            "Set boolean '{}' to {}{}",
            boolean_name,
            if desired_value { "on" } else { "off" },
            if persistent { " (persistent)" } else { "" }
        ))
        .with_data("name", serde_json::json!(boolean_name))
        .with_data("value", serde_json::json!(desired_value))
        .with_data("previous_value", serde_json::json!(current))
        .with_data("persistent", serde_json::json!(persistent)))
    }

    /// Execute context management operation
    fn execute_context(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: &Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let target = params.get_string_required("target")?;
        Self::validate_path(&target)?;

        let setype = params.get_string("setype")?;
        let seuser = params.get_string("seuser")?;
        let serole = params.get_string("serole")?;
        let selevel = params.get_string("selevel")?;
        let recursive = params.get_bool_or("recursive", false);
        let reload = params.get_bool_or("reload", true);
        let ftype_str = params
            .get_string("ftype")?
            .unwrap_or_else(|| "a".to_string());
        let ftype = SELinuxFileType::from_str(&ftype_str)?;

        // Validate type names if provided
        if let Some(ref t) = setype {
            Self::validate_type_name(t)?;
        }
        if let Some(ref u) = seuser {
            Self::validate_user_name(u)?;
        }
        if let Some(ref r) = serole {
            Self::validate_role_name(r)?;
        }

        // Get current context
        let current_context = Self::get_file_context(connection, &target, context)?;

        // Check if we need to add fcontext rule
        let manage_fcontext = params.get_bool_or("fcontext", false);
        let fcontext_state_str = params
            .get_string("fcontext_state")?
            .unwrap_or_else(|| "present".to_string());
        let fcontext_present = fcontext_state_str == "present";

        if manage_fcontext {
            if let Some(ref t) = setype {
                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would manage fcontext for '{}' with type '{}'",
                        target, t
                    )));
                }

                let changed = Self::manage_fcontext(
                    connection,
                    &target,
                    t,
                    &ftype,
                    fcontext_present,
                    context,
                )?;

                if changed && reload {
                    Self::restore_context(connection, &target, recursive, context)?;
                }

                if changed {
                    return Ok(ModuleOutput::changed(format!(
                        "Updated fcontext for '{}' with type '{}'",
                        target, t
                    ))
                    .with_data("target", serde_json::json!(target))
                    .with_data("setype", serde_json::json!(t)));
                } else {
                    return Ok(ModuleOutput::ok(format!(
                        "Fcontext for '{}' already correct",
                        target
                    ))
                    .with_data("target", serde_json::json!(target)));
                }
            }
        }

        // Direct context change using chcon
        let needs_change = match &current_context {
            Some((u, r, t, l)) => {
                (setype.is_some() && setype.as_ref() != Some(t))
                    || (seuser.is_some() && seuser.as_ref() != Some(u))
                    || (serole.is_some() && serole.as_ref() != Some(r))
                    || (selevel.is_some() && selevel.as_ref() != Some(l))
            }
            None => true,
        };

        if !needs_change {
            let msg = format!("Context for '{}' already correct", target);
            let mut output = ModuleOutput::ok(msg).with_data("target", serde_json::json!(target));

            if let Some((u, r, t, l)) = current_context {
                output = output
                    .with_data("seuser", serde_json::json!(u))
                    .with_data("serole", serde_json::json!(r))
                    .with_data("setype", serde_json::json!(t))
                    .with_data("selevel", serde_json::json!(l));
            }

            return Ok(output);
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would change context for '{}'",
                target
            )));
        }

        Self::set_file_context(
            connection,
            &target,
            seuser.as_deref(),
            serole.as_deref(),
            setype.as_deref(),
            selevel.as_deref(),
            recursive,
            context,
        )?;

        let mut output = ModuleOutput::changed(format!("Changed context for '{}'", target))
            .with_data("target", serde_json::json!(target))
            .with_data("recursive", serde_json::json!(recursive));

        if let Some(ref t) = setype {
            output = output.with_data("setype", serde_json::json!(t));
        }
        if let Some(ref u) = seuser {
            output = output.with_data("seuser", serde_json::json!(u));
        }
        if let Some(ref r) = serole {
            output = output.with_data("serole", serde_json::json!(r));
        }
        if let Some(ref l) = selevel {
            output = output.with_data("selevel", serde_json::json!(l));
        }

        Ok(output)
    }

    /// Execute port management operation
    fn execute_port(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: &Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let ports_param = params.get_string_required("ports")?;
        let proto_str = params.get_string_required("proto")?;
        let port_type = params.get_string_required("port_type")?;
        let state_str = params
            .get_string("port_state")?
            .unwrap_or_else(|| "present".to_string());

        Self::validate_type_name(&port_type)?;
        let proto = SELinuxProtocol::from_str(&proto_str)?;
        let state = PortState::from_str(&state_str)?;

        // Parse ports (can be single, range, or comma-separated)
        let ports: Vec<String> = if ports_param.contains(',') {
            ports_param
                .split(',')
                .map(|s| s.trim().to_string())
                .collect()
        } else {
            vec![ports_param.clone()]
        };

        // Validate all ports
        for port in &ports {
            Self::validate_port(port)?;
        }

        if context.check_mode {
            let action = match state {
                PortState::Present => "add",
                PortState::Absent => "remove",
            };
            return Ok(ModuleOutput::changed(format!(
                "Would {} port(s) {} with type '{}' for {}",
                action,
                ports.join(", "),
                port_type,
                proto.as_str()
            )));
        }

        let mut changed = false;
        let mut changed_ports = Vec::new();

        for port in &ports {
            let port_changed =
                Self::manage_port(connection, port, &proto, &port_type, &state, context)?;
            if port_changed {
                changed = true;
                changed_ports.push(port.clone());
            }
        }

        if changed {
            Ok(ModuleOutput::changed(format!(
                "Managed port(s) {} with type '{}' for {}",
                changed_ports.join(", "),
                port_type,
                proto.as_str()
            ))
            .with_data("ports", serde_json::json!(changed_ports))
            .with_data("port_type", serde_json::json!(port_type))
            .with_data("proto", serde_json::json!(proto.as_str()))
            .with_data("state", serde_json::json!(state_str)))
        } else {
            Ok(ModuleOutput::ok(format!(
                "Port(s) {} already in desired state",
                ports.join(", ")
            ))
            .with_data("ports", serde_json::json!(ports))
            .with_data("port_type", serde_json::json!(port_type))
            .with_data("proto", serde_json::json!(proto.as_str())))
        }
    }

    /// Validate SELinux type name
    fn validate_type_name(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "SELinux type cannot be empty".to_string(),
            ));
        }

        if !SELINUX_TYPE_REGEX.is_match(name) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid SELinux type '{}': must match pattern like 'httpd_sys_content_t'",
                name
            )));
        }

        Ok(())
    }

    /// Validate SELinux user name
    fn validate_user_name(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "SELinux user cannot be empty".to_string(),
            ));
        }

        if !SELINUX_USER_REGEX.is_match(name) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid SELinux user '{}': must match pattern like 'system_u'",
                name
            )));
        }

        Ok(())
    }

    /// Validate SELinux role name
    fn validate_role_name(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "SELinux role cannot be empty".to_string(),
            ));
        }

        if !SELINUX_ROLE_REGEX.is_match(name) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid SELinux role '{}': must match pattern like 'object_r'",
                name
            )));
        }

        Ok(())
    }

    /// Validate SELinux boolean name
    fn validate_boolean_name(name: &str) -> ModuleResult<()> {
        if name.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "SELinux boolean name cannot be empty".to_string(),
            ));
        }

        if !SELINUX_BOOLEAN_REGEX.is_match(name) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid SELinux boolean name '{}': must contain only alphanumeric characters and underscores",
                name
            )));
        }

        Ok(())
    }

    /// Validate port specification
    fn validate_port(port: &str) -> ModuleResult<()> {
        if port.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Port cannot be empty".to_string(),
            ));
        }

        if !PORT_REGEX.is_match(port) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid port '{}': must be a number or range like '1000-2000'",
                port
            )));
        }

        // Validate port numbers are in range
        for part in port.split('-') {
            let num: u32 = part.parse().map_err(|_| {
                ModuleError::InvalidParameter(format!("Invalid port number '{}'", part))
            })?;
            if num == 0 || num > 65535 {
                return Err(ModuleError::InvalidParameter(format!(
                    "Port number {} out of range (1-65535)",
                    num
                )));
            }
        }

        Ok(())
    }

    /// Validate path for context operations
    fn validate_path(path: &str) -> ModuleResult<()> {
        if path.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "Target path cannot be empty".to_string(),
            ));
        }

        if path.contains('\0') {
            return Err(ModuleError::InvalidParameter(
                "Target path contains null byte".to_string(),
            ));
        }

        if path.contains('\n') || path.contains('\r') {
            return Err(ModuleError::InvalidParameter(
                "Target path contains newline characters".to_string(),
            ));
        }

        Ok(())
    }
}

impl Module for SELinuxModule {
    fn name(&self) -> &'static str {
        "selinux"
    }

    fn description(&self) -> &'static str {
        "Manage SELinux mode, booleans, file contexts, and port types"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn required_params(&self) -> &[&'static str] {
        &[] // Required params depend on operation type
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "SELinux module requires a connection for remote execution".to_string(),
            )
        })?;

        // Check if SELinux is available
        if !Self::is_selinux_available(connection, context)? {
            return Err(ModuleError::ExecutionFailed(
                "SELinux is not available on this system".to_string(),
            ));
        }

        // Determine operation type
        let operation = Self::determine_operation(params)?;

        match operation {
            SELinuxOperation::Mode => self.execute_mode(params, context, connection),
            SELinuxOperation::Boolean => self.execute_boolean(params, context, connection),
            SELinuxOperation::Context => self.execute_context(params, context, connection),
            SELinuxOperation::Port => self.execute_port(params, context, connection),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_selinux_mode_from_str() {
        assert_eq!(
            SELinuxMode::from_str("enforcing").unwrap(),
            SELinuxMode::Enforcing
        );
        assert_eq!(
            SELinuxMode::from_str("permissive").unwrap(),
            SELinuxMode::Permissive
        );
        assert_eq!(
            SELinuxMode::from_str("disabled").unwrap(),
            SELinuxMode::Disabled
        );
        assert_eq!(SELinuxMode::from_str("1").unwrap(), SELinuxMode::Enforcing);
        assert_eq!(SELinuxMode::from_str("0").unwrap(), SELinuxMode::Permissive);
        assert!(SELinuxMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_selinux_mode_as_str() {
        assert_eq!(SELinuxMode::Enforcing.as_str(), "enforcing");
        assert_eq!(SELinuxMode::Permissive.as_str(), "permissive");
        assert_eq!(SELinuxMode::Disabled.as_str(), "disabled");
    }

    #[test]
    fn test_selinux_mode_as_setenforce() {
        assert_eq!(SELinuxMode::Enforcing.as_setenforce(), Some("1"));
        assert_eq!(SELinuxMode::Permissive.as_setenforce(), Some("0"));
        assert_eq!(SELinuxMode::Disabled.as_setenforce(), None);
    }

    #[test]
    fn test_selinux_protocol_from_str() {
        assert_eq!(
            SELinuxProtocol::from_str("tcp").unwrap(),
            SELinuxProtocol::Tcp
        );
        assert_eq!(
            SELinuxProtocol::from_str("udp").unwrap(),
            SELinuxProtocol::Udp
        );
        assert_eq!(
            SELinuxProtocol::from_str("dccp").unwrap(),
            SELinuxProtocol::Dccp
        );
        assert_eq!(
            SELinuxProtocol::from_str("sctp").unwrap(),
            SELinuxProtocol::Sctp
        );
        assert!(SELinuxProtocol::from_str("invalid").is_err());
    }

    #[test]
    fn test_selinux_file_type_from_str() {
        assert_eq!(
            SELinuxFileType::from_str("a").unwrap(),
            SELinuxFileType::All
        );
        assert_eq!(
            SELinuxFileType::from_str("f").unwrap(),
            SELinuxFileType::File
        );
        assert_eq!(
            SELinuxFileType::from_str("d").unwrap(),
            SELinuxFileType::Directory
        );
        assert_eq!(
            SELinuxFileType::from_str("c").unwrap(),
            SELinuxFileType::CharDevice
        );
        assert_eq!(
            SELinuxFileType::from_str("b").unwrap(),
            SELinuxFileType::BlockDevice
        );
        assert_eq!(
            SELinuxFileType::from_str("s").unwrap(),
            SELinuxFileType::Socket
        );
        assert_eq!(
            SELinuxFileType::from_str("l").unwrap(),
            SELinuxFileType::SymLink
        );
        assert_eq!(
            SELinuxFileType::from_str("p").unwrap(),
            SELinuxFileType::Pipe
        );
        assert!(SELinuxFileType::from_str("x").is_err());
    }

    #[test]
    fn test_port_state_from_str() {
        assert_eq!(PortState::from_str("present").unwrap(), PortState::Present);
        assert_eq!(PortState::from_str("absent").unwrap(), PortState::Absent);
        assert!(PortState::from_str("invalid").is_err());
    }

    #[test]
    fn test_validate_type_name() {
        assert!(SELinuxModule::validate_type_name("httpd_sys_content_t").is_ok());
        assert!(SELinuxModule::validate_type_name("user_home_t").is_ok());
        assert!(SELinuxModule::validate_type_name("sshd_t").is_ok());
        assert!(SELinuxModule::validate_type_name("").is_err());
        assert!(SELinuxModule::validate_type_name("invalid").is_err());
        assert!(SELinuxModule::validate_type_name("invalid_type").is_err());
        assert!(SELinuxModule::validate_type_name("123_t").is_err());
    }

    #[test]
    fn test_validate_user_name() {
        assert!(SELinuxModule::validate_user_name("system_u").is_ok());
        assert!(SELinuxModule::validate_user_name("user_u").is_ok());
        assert!(SELinuxModule::validate_user_name("staff_u").is_ok());
        assert!(SELinuxModule::validate_user_name("").is_err());
        assert!(SELinuxModule::validate_user_name("invalid").is_err());
        assert!(SELinuxModule::validate_user_name("invalid_user").is_err());
    }

    #[test]
    fn test_validate_role_name() {
        assert!(SELinuxModule::validate_role_name("object_r").is_ok());
        assert!(SELinuxModule::validate_role_name("system_r").is_ok());
        assert!(SELinuxModule::validate_role_name("staff_r").is_ok());
        assert!(SELinuxModule::validate_role_name("").is_err());
        assert!(SELinuxModule::validate_role_name("invalid").is_err());
    }

    #[test]
    fn test_validate_boolean_name() {
        assert!(SELinuxModule::validate_boolean_name("httpd_can_network_connect").is_ok());
        assert!(SELinuxModule::validate_boolean_name("samba_enable_home_dirs").is_ok());
        assert!(SELinuxModule::validate_boolean_name("ftp_home_dir").is_ok());
        assert!(SELinuxModule::validate_boolean_name("").is_err());
        assert!(SELinuxModule::validate_boolean_name("invalid-name").is_err());
        assert!(SELinuxModule::validate_boolean_name("name with space").is_err());
    }

    #[test]
    fn test_validate_port() {
        assert!(SELinuxModule::validate_port("80").is_ok());
        assert!(SELinuxModule::validate_port("443").is_ok());
        assert!(SELinuxModule::validate_port("8000-9000").is_ok());
        assert!(SELinuxModule::validate_port("1").is_ok());
        assert!(SELinuxModule::validate_port("65535").is_ok());
        assert!(SELinuxModule::validate_port("").is_err());
        assert!(SELinuxModule::validate_port("0").is_err());
        assert!(SELinuxModule::validate_port("65536").is_err());
        assert!(SELinuxModule::validate_port("abc").is_err());
        assert!(SELinuxModule::validate_port("80-").is_err());
    }

    #[test]
    fn test_validate_path() {
        assert!(SELinuxModule::validate_path("/var/www/html").is_ok());
        assert!(SELinuxModule::validate_path("/home/user").is_ok());
        assert!(SELinuxModule::validate_path("/etc/selinux/config").is_ok());
        assert!(SELinuxModule::validate_path("").is_err());
        assert!(SELinuxModule::validate_path("/path\0with/null").is_err());
        assert!(SELinuxModule::validate_path("/path\nwith/newline").is_err());
    }

    #[test]
    fn test_determine_operation() {
        let mut params: ModuleParams = HashMap::new();

        // Mode operation
        params.insert("state".to_string(), serde_json::json!("enforcing"));
        assert_eq!(
            SELinuxModule::determine_operation(&params).unwrap(),
            SELinuxOperation::Mode
        );

        // Boolean operation
        params.clear();
        params.insert(
            "boolean".to_string(),
            serde_json::json!("httpd_can_network_connect"),
        );
        assert_eq!(
            SELinuxModule::determine_operation(&params).unwrap(),
            SELinuxOperation::Boolean
        );

        // Context operation
        params.clear();
        params.insert("target".to_string(), serde_json::json!("/var/www"));
        assert_eq!(
            SELinuxModule::determine_operation(&params).unwrap(),
            SELinuxOperation::Context
        );

        // Port operation
        params.clear();
        params.insert("ports".to_string(), serde_json::json!("8080"));
        assert_eq!(
            SELinuxModule::determine_operation(&params).unwrap(),
            SELinuxOperation::Port
        );

        // No operation specified
        params.clear();
        assert!(SELinuxModule::determine_operation(&params).is_err());
    }

    #[test]
    fn test_selinux_module_metadata() {
        let module = SELinuxModule;
        assert_eq!(module.name(), "selinux");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert!(module.required_params().is_empty());
    }

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("with-hyphen"), "with-hyphen");
        assert_eq!(shell_escape("with.dot"), "with.dot");
        assert_eq!(shell_escape("/path/to/file"), "/path/to/file");
        assert_eq!(shell_escape("user:role:type:level"), "user:role:type:level");
        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
    }
}
