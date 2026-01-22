//! Group module - Group management
//!
//! This module manages groups on the system.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Desired state for a group
#[derive(Debug, Clone, PartialEq)]
pub enum GroupState {
    Present,
    Absent,
}

impl GroupState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(GroupState::Present),
            "absent" => Ok(GroupState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

impl std::str::FromStr for GroupState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        GroupState::from_str(s)
    }
}

/// Information about a group
#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
}

/// Module for group management
pub struct GroupModule;

impl GroupModule {
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

        // Use tokio runtime to execute async command
        let result = Handle::current()
            .block_on(async { connection.execute(command, Some(options)).await })
            .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Check if a group exists via connection
    fn group_exists_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let command = format!("getent group {}", shell_escape(name));
        let (success, _, _) = Self::execute_command(connection, &command, context)?;
        Ok(success)
    }

    /// Get group info via connection by parsing /etc/group
    fn get_group_info_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<GroupInfo>> {
        // Use getent to get group info
        let command = format!("getent group {}", shell_escape(name));
        let (success, stdout, _) = Self::execute_command(connection, &command, context)?;

        if !success || stdout.trim().is_empty() {
            return Ok(None);
        }

        // Parse group line: name:x:gid:members
        let parts: Vec<&str> = stdout.trim().split(':').collect();
        if parts.len() < 4 {
            return Err(ModuleError::ExecutionFailed(format!(
                "Invalid group entry for group '{}'",
                name
            )));
        }

        let gid = parts[2].parse().unwrap_or(0);
        let members = if parts[3].is_empty() {
            Vec::new()
        } else {
            parts[3].split(',').map(|s| s.to_string()).collect()
        };

        Ok(Some(GroupInfo {
            name: parts[0].to_string(),
            gid,
            members,
        }))
    }

    /// Create a group via connection
    fn create_group_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        gid: Option<u32>,
        system: bool,
        local: bool,
        non_unique: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd_name = if local { "lgroupadd" } else { "groupadd" };
        let mut cmd_parts = vec![cmd_name.to_string()];

        if let Some(gid) = gid {
            cmd_parts.push("-g".to_string());
            cmd_parts.push(gid.to_string());
        }

        if system {
            cmd_parts.push("-r".to_string());
        }

        if non_unique {
            cmd_parts.push("-o".to_string());
        }

        cmd_parts.push(shell_escape(name).into_owned());

        let command = cmd_parts.join(" ");
        let (success, _, stderr) = Self::execute_command(connection, &command, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(stderr))
        }
    }

    /// Modify a group via connection
    fn modify_group_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        gid: Option<u32>,
        local: bool,
        non_unique: bool,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let current = Self::get_group_info_via_connection(connection, name, context)?
            .ok_or_else(|| ModuleError::ExecutionFailed(format!("Group '{}' not found", name)))?;

        let cmd_name = if local { "lgroupmod" } else { "groupmod" };
        let mut needs_change = false;
        let mut cmd_parts = vec![cmd_name.to_string()];

        if let Some(gid) = gid {
            if current.gid != gid {
                cmd_parts.push("-g".to_string());
                cmd_parts.push(gid.to_string());
                if non_unique {
                    cmd_parts.push("-o".to_string());
                }
                needs_change = true;
            }
        }

        if !needs_change {
            return Ok(false);
        }

        cmd_parts.push(shell_escape(name).into_owned());

        let command = cmd_parts.join(" ");
        let (success, _, stderr) = Self::execute_command(connection, &command, context)?;

        if success {
            Ok(true)
        } else {
            Err(ModuleError::ExecutionFailed(stderr))
        }
    }

    /// Delete a group via connection
    fn delete_group_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        local: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd_name = if local { "lgroupdel" } else { "groupdel" };
        let command = format!("{} {}", cmd_name, shell_escape(name));
        let (success, _, stderr) = Self::execute_command(connection, &command, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(stderr))
        }
    }
}

impl Module for GroupModule {
    fn name(&self) -> &'static str {
        "group"
    }

    fn description(&self) -> &'static str {
        "Manage groups"
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
                "Group module requires a connection for remote execution".to_string(),
            )
        })?;

        let name = params.get_string_required("name")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = GroupState::from_str(&state_str)?;

        let gid = params.get_u32("gid")?;
        let system = params.get_bool_or("system", false);
        let local = params.get_bool_or("local", false);
        let non_unique = params.get_bool_or("non_unique", false);

        let group_exists = Self::group_exists_via_connection(connection, &name, context)?;

        match state {
            GroupState::Absent => {
                if !group_exists {
                    return Ok(ModuleOutput::ok(format!("Group '{}' already absent", name)));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would remove group '{}'",
                        name
                    )));
                }

                Self::delete_group_via_connection(connection, &name, local, context)?;
                Ok(ModuleOutput::changed(format!("Removed group '{}'", name)))
            }

            GroupState::Present => {
                let mut changed = false;
                let mut messages = Vec::new();

                if !group_exists {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create group '{}'",
                            name
                        )));
                    }

                    Self::create_group_via_connection(
                        connection, &name, gid, system, local, non_unique, context,
                    )?;
                    changed = true;
                    messages.push(format!("Created group '{}'", name));
                } else {
                    // Modify existing group
                    if context.check_mode {
                        // Check if modification would be needed
                        if let Some(desired_gid) = gid {
                            if let Some(info) =
                                Self::get_group_info_via_connection(connection, &name, context)?
                            {
                                if info.gid != desired_gid {
                                    return Ok(ModuleOutput::changed(format!(
                                        "Would modify group '{}'",
                                        name
                                    )));
                                }
                            }
                        }
                        return Ok(ModuleOutput::ok(format!(
                            "Group '{}' is in desired state",
                            name
                        )));
                    }

                    let modified = Self::modify_group_via_connection(
                        connection, &name, gid, local, non_unique, context,
                    )?;

                    if modified {
                        changed = true;
                        messages.push(format!("Modified group '{}'", name));
                    }
                }

                // Get final group info
                let group_info = Self::get_group_info_via_connection(connection, &name, context)?;
                let mut data = HashMap::new();

                if let Some(info) = group_info {
                    data.insert("gid".to_string(), serde_json::json!(info.gid));
                    data.insert("members".to_string(), serde_json::json!(info.members));
                }

                let msg = if messages.is_empty() {
                    format!("Group '{}' is in desired state", name)
                } else {
                    messages.join(". ")
                };

                let mut output = if changed {
                    ModuleOutput::changed(msg)
                } else {
                    ModuleOutput::ok(msg)
                };

                for (k, v) in data {
                    output = output.with_data(k, v);
                }

                Ok(output)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_state_from_str() {
        assert_eq!(
            GroupState::from_str("present").unwrap(),
            GroupState::Present
        );
        assert_eq!(GroupState::from_str("absent").unwrap(), GroupState::Absent);
        assert!(GroupState::from_str("invalid").is_err());
    }

    #[test]
    fn test_group_module_name() {
        let module = GroupModule;
        assert_eq!(module.name(), "group");
    }

    #[test]
    fn test_group_module_classification() {
        let module = GroupModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_group_module_required_params() {
        let module = GroupModule;
        assert_eq!(module.required_params(), &["name"]);
    }
}
