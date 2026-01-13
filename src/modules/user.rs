//! User module - User management
//!
//! This module manages user accounts on the system.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions, TransferOptions};
use crate::utils::shell_escape;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Handle;
use uuid::Uuid;

/// Desired state for a user
#[derive(Debug, Clone, PartialEq)]
pub enum UserState {
    Present,
    Absent,
}

impl UserState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(UserState::Present),
            "absent" => Ok(UserState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

/// Information about a user
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub comment: String,
    pub home: String,
    pub shell: String,
    pub groups: Vec<String>,
}

/// Module for user management
pub struct UserModule;

impl UserModule {
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

    /// Execute a command via connection or locally
    fn execute_command(
        connection: &Arc<dyn Connection + Send + Sync>,
        command: &str,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String, String)> {
        let options = Self::get_exec_options(context);

        // Use tokio runtime to execute async command
        // Use thread::scope to avoid nested runtime issues when called from async context
        let handle = Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let connection = connection.clone();
        let command = command.to_string();
        let result = std::thread::scope(|s| {
            s.spawn(|| handle.block_on(async { connection.execute(&command, Some(options)).await }))
                .join()
                .unwrap()
        })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Check if a user exists via connection
    fn user_exists_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let command = format!("id {}", shell_escape(name));
        let (success, _, _) = Self::execute_command(connection, &command, context)?;
        Ok(success)
    }

    /// Get user info via connection by parsing /etc/passwd and groups
    fn get_user_info_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<UserInfo>> {
        // Use getent to get passwd info
        let command = format!("getent passwd {}", shell_escape(name));
        let (success, stdout, _) = Self::execute_command(connection, &command, context)?;

        if !success || stdout.trim().is_empty() {
            return Ok(None);
        }

        // Parse passwd line: name:x:uid:gid:comment:home:shell
        let parts: Vec<&str> = stdout.trim().split(':').collect();
        if parts.len() < 7 {
            return Err(ModuleError::ExecutionFailed(format!(
                "Invalid passwd entry for user '{}'",
                name
            )));
        }

        let uid = parts[2].parse().unwrap_or(0);
        let gid = parts[3].parse().unwrap_or(0);

        // Get user's groups
        let groups_cmd = format!("groups {}", shell_escape(name));
        let (groups_success, groups_stdout, _) =
            Self::execute_command(connection, &groups_cmd, context)?;

        let groups = if groups_success {
            groups_stdout
                .split(':')
                .last()
                .unwrap_or("")
                .split_whitespace()
                .map(|s| s.to_string())
                .collect()
        } else {
            Vec::new()
        };

        Ok(Some(UserInfo {
            name: parts[0].to_string(),
            uid,
            gid,
            comment: parts[4].to_string(),
            home: parts[5].to_string(),
            shell: parts[6].to_string(),
            groups,
        }))
    }

    /// Create a user via connection
    fn create_user_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        uid: Option<u32>,
        group: Option<&str>,
        groups: Option<&[String]>,
        home: Option<&str>,
        shell: Option<&str>,
        comment: Option<&str>,
        create_home: bool,
        system: bool,
        local: bool,
        expires: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd_name = if local { "luseradd" } else { "useradd" };
        let mut cmd_parts = vec![cmd_name.to_string()];

        if let Some(uid) = uid {
            cmd_parts.push("-u".to_string());
            cmd_parts.push(uid.to_string());
        }

        if let Some(group) = group {
            cmd_parts.push("-g".to_string());
            cmd_parts.push(shell_escape(group));
        }

        if let Some(groups) = groups {
            if !groups.is_empty() {
                cmd_parts.push("-G".to_string());
                cmd_parts.push(groups.join(","));
            }
        }

        if let Some(home) = home {
            cmd_parts.push("-d".to_string());
            cmd_parts.push(shell_escape(home));
        }

        if let Some(shell) = shell {
            cmd_parts.push("-s".to_string());
            cmd_parts.push(shell_escape(shell));
        }

        if let Some(comment) = comment {
            cmd_parts.push("-c".to_string());
            cmd_parts.push(format!("'{}'", comment.replace('\'', "'\\''")));
        }

        if create_home {
            cmd_parts.push("-m".to_string());
        } else {
            cmd_parts.push("-M".to_string());
        }

        if system {
            cmd_parts.push("-r".to_string());
        }

        if let Some(expires) = expires {
            cmd_parts.push("-e".to_string());
            cmd_parts.push(shell_escape(expires));
        }

        cmd_parts.push(shell_escape(name));

        let command = cmd_parts.join(" ");
        let (success, _, stderr) = Self::execute_command(connection, &command, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(stderr))
        }
    }

    /// Modify a user via connection
    fn modify_user_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        uid: Option<u32>,
        group: Option<&str>,
        groups: Option<&[String]>,
        append_groups: bool,
        home: Option<&str>,
        shell: Option<&str>,
        comment: Option<&str>,
        move_home: bool,
        local: bool,
        expires: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let current = Self::get_user_info_via_connection(connection, name, context)?
            .ok_or_else(|| ModuleError::ExecutionFailed(format!("User '{}' not found", name)))?;

        let cmd_name = if local { "lusermod" } else { "usermod" };
        let mut needs_change = false;
        let mut cmd_parts = vec![cmd_name.to_string()];

        if let Some(uid) = uid {
            if current.uid != uid {
                cmd_parts.push("-u".to_string());
                cmd_parts.push(uid.to_string());
                needs_change = true;
            }
        }

        if let Some(group) = group {
            cmd_parts.push("-g".to_string());
            cmd_parts.push(shell_escape(group));
            needs_change = true;
        }

        if let Some(groups) = groups {
            if !groups.is_empty() {
                let groups_str = groups.join(",");
                if append_groups {
                    cmd_parts.push("-a".to_string());
                    cmd_parts.push("-G".to_string());
                    cmd_parts.push(groups_str);
                } else {
                    cmd_parts.push("-G".to_string());
                    cmd_parts.push(groups_str);
                }
                needs_change = true;
            }
        }

        if let Some(home) = home {
            if current.home != home {
                cmd_parts.push("-d".to_string());
                cmd_parts.push(shell_escape(home));
                if move_home {
                    cmd_parts.push("-m".to_string());
                }
                needs_change = true;
            }
        }

        if let Some(shell) = shell {
            if current.shell != shell {
                cmd_parts.push("-s".to_string());
                cmd_parts.push(shell_escape(shell));
                needs_change = true;
            }
        }

        if let Some(comment) = comment {
            if current.comment != comment {
                cmd_parts.push("-c".to_string());
                cmd_parts.push(format!("'{}'", comment.replace('\'', "'\\''")));
                needs_change = true;
            }
        }

        if let Some(expires) = expires {
            cmd_parts.push("-e".to_string());
            cmd_parts.push(shell_escape(expires));
            needs_change = true;
        }

        if !needs_change {
            return Ok(false);
        }

        cmd_parts.push(shell_escape(name));

        let command = cmd_parts.join(" ");
        let (success, _, stderr) = Self::execute_command(connection, &command, context)?;

        if success {
            Ok(true)
        } else {
            Err(ModuleError::ExecutionFailed(stderr))
        }
    }

    /// Delete a user via connection
    fn delete_user_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        remove_home: bool,
        force: bool,
        local: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd_name = if local { "luserdel" } else { "userdel" };
        let mut cmd_parts = vec![cmd_name.to_string()];

        if remove_home {
            cmd_parts.push("-r".to_string());
        }

        if force {
            cmd_parts.push("-f".to_string());
        }

        cmd_parts.push(shell_escape(name));

        let command = cmd_parts.join(" ");
        let (success, _, stderr) = Self::execute_command(connection, &command, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(stderr))
        }
    }

    /// Set password via connection
    fn set_password_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        password: &str,
        encrypted: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Use a temporary file to avoid exposing password in process list via echo
        let temp_path = format!("/tmp/.ansible_passwd_{}", Uuid::new_v4());
        let content = format!("{}:{}", name, password);

        // Upload content to temp file with 600 permissions
        let mut transfer_opts = TransferOptions::new();
        transfer_opts = transfer_opts.with_mode(0o600);

        let handle = Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let conn_clone = connection.clone();
        let temp_path_clone = temp_path.clone();
        let content_clone = content.clone();

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(async {
                conn_clone.upload_content(content_clone.as_bytes(), Path::new(&temp_path_clone), Some(transfer_opts)).await
            }))
            .join()
            .unwrap()
        }).map_err(|e| ModuleError::ExecutionFailed(format!("Failed to upload password file: {}", e)))?;

        // Use chpasswd reading from the file
        let flag = if encrypted { "-e" } else { "" };
        let command = format!("chpasswd {} < {}", flag, shell_escape(&temp_path));

        let result = Self::execute_command(connection, &command, context);

        // Clean up temp file regardless of success/failure
        let rm_cmd = format!("rm -f {}", shell_escape(&temp_path));
        let _ = Self::execute_command(connection, &rm_cmd, context);

        match result {
            Ok((success, _, stderr)) => {
                if success {
                    Ok(())
                } else {
                    Err(ModuleError::ExecutionFailed(format!(
                        "Failed to set password: {}",
                        stderr
                    )))
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Lock or unlock user password via connection
    fn set_password_lock_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        lock: bool,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        // Check current lock status by examining shadow file
        let check_cmd = format!(
            "getent shadow {} | cut -d: -f2 | grep -q '^!'",
            shell_escape(name)
        );
        let (is_locked, _, _) = Self::execute_command(connection, &check_cmd, context)?;

        if lock == is_locked {
            // Already in desired state
            return Ok(false);
        }

        let flag = if lock { "-L" } else { "-U" };
        let command = format!("passwd {} {}", flag, shell_escape(name));

        let (success, _, stderr) = Self::execute_command(connection, &command, context)?;

        if success {
            Ok(true)
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to {} password: {}",
                if lock { "lock" } else { "unlock" },
                stderr
            )))
        }
    }

    /// Generate SSH key via connection
    fn generate_ssh_key_via_connection(
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        ssh_key_type: &str,
        ssh_key_bits: u32,
        ssh_key_file: Option<&str>,
        ssh_key_comment: Option<&str>,
        ssh_key_passphrase: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        // Get user info to find home directory
        let user_info = Self::get_user_info_via_connection(connection, name, context)?
            .ok_or_else(|| ModuleError::ExecutionFailed(format!("User '{}' not found", name)))?;

        let key_file = ssh_key_file
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}/.ssh/id_{}", user_info.home, ssh_key_type));

        // Check if key already exists
        let check_cmd = format!("test -f {}", shell_escape(&key_file));
        let (exists, _, _) = Self::execute_command(connection, &check_cmd, context)?;
        if exists {
            return Ok(false);
        }

        // Create .ssh directory if needed
        let ssh_dir = format!("{}/.ssh", user_info.home);
        let mkdir_cmd = format!(
            "mkdir -p {} && chown {}:{} {} && chmod 700 {}",
            shell_escape(&ssh_dir),
            user_info.uid,
            user_info.gid,
            shell_escape(&ssh_dir),
            shell_escape(&ssh_dir)
        );
        Self::execute_command(connection, &mkdir_cmd, context)?;

        // Generate SSH key
        let passphrase = ssh_key_passphrase.unwrap_or("");
        let comment_arg = ssh_key_comment
            .map(|c| format!("-C '{}'", c.replace('\'', "'\\''")))
            .unwrap_or_default();

        let keygen_cmd = format!(
            "ssh-keygen -t {} -b {} -f {} {} -N '{}'",
            ssh_key_type,
            ssh_key_bits,
            shell_escape(&key_file),
            comment_arg,
            passphrase.replace('\'', "'\\''")
        );

        let (success, _, stderr) = Self::execute_command(connection, &keygen_cmd, context)?;
        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to generate SSH key: {}",
                stderr
            )));
        }

        // Set ownership and permissions
        let perms_cmd = format!(
            "chown {}:{} {} {}.pub && chmod 600 {} && chmod 644 {}.pub",
            user_info.uid,
            user_info.gid,
            shell_escape(&key_file),
            shell_escape(&key_file),
            shell_escape(&key_file),
            shell_escape(&key_file)
        );
        Self::execute_command(connection, &perms_cmd, context)?;

        Ok(true)
    }
}

impl Module for UserModule {
    fn name(&self) -> &'static str {
        "user"
    }

    fn description(&self) -> &'static str {
        "Manage user accounts"
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
                "User module requires a connection for remote execution".to_string(),
            )
        })?;

        let name = params.get_string_required("name")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = UserState::from_str(&state_str)?;

        let uid = params.get_u32("uid")?;
        let group = params.get_string("group")?;
        let groups = params.get_vec_string("groups")?;
        let append_groups = params.get_bool_or("append", false);
        let home = params.get_string("home")?;
        let shell = params.get_string("shell")?;
        let comment = params.get_string("comment")?;
        let create_home = params.get_bool_or("create_home", true);
        let move_home = params.get_bool_or("move_home", false);
        let system = params.get_bool_or("system", false);
        let remove_home = params.get_bool_or("remove", false);
        let force = params.get_bool_or("force", false);
        let local = params.get_bool_or("local", false);
        let password = params.get_string("password")?;
        let password_encrypted = params.get_bool_or("password_encrypted", true);
        let password_lock = params.get_bool("password_lock")?;
        let expires = params.get_string("expires")?;
        let generate_ssh_key = params.get_bool_or("generate_ssh_key", false);
        let ssh_key_type = params
            .get_string("ssh_key_type")?
            .unwrap_or_else(|| "rsa".to_string());
        let ssh_key_bits = params.get_u32("ssh_key_bits")?.unwrap_or(4096);
        let ssh_key_file = params.get_string("ssh_key_file")?;
        let ssh_key_comment = params.get_string("ssh_key_comment")?;
        let ssh_key_passphrase = params.get_string("ssh_key_passphrase")?;

        let user_exists = Self::user_exists_via_connection(connection, &name, context)?;

        match state {
            UserState::Absent => {
                if !user_exists {
                    return Ok(ModuleOutput::ok(format!("User '{}' already absent", name)));
                }

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would remove user '{}'",
                        name
                    )));
                }

                Self::delete_user_via_connection(
                    connection,
                    &name,
                    remove_home,
                    force,
                    local,
                    context,
                )?;
                Ok(ModuleOutput::changed(format!("Removed user '{}'", name)))
            }

            UserState::Present => {
                let mut changed = false;
                let mut messages = Vec::new();

                if !user_exists {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create user '{}'",
                            name
                        )));
                    }

                    Self::create_user_via_connection(
                        connection,
                        &name,
                        uid,
                        group.as_deref(),
                        groups.as_deref(),
                        home.as_deref(),
                        shell.as_deref(),
                        comment.as_deref(),
                        create_home,
                        system,
                        local,
                        expires.as_deref(),
                        context,
                    )?;

                    changed = true;
                    messages.push(format!("Created user '{}'", name));
                } else {
                    // Modify existing user
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would modify user '{}'",
                            name
                        )));
                    }

                    let modified = Self::modify_user_via_connection(
                        connection,
                        &name,
                        uid,
                        group.as_deref(),
                        groups.as_deref(),
                        append_groups,
                        home.as_deref(),
                        shell.as_deref(),
                        comment.as_deref(),
                        move_home,
                        local,
                        expires.as_deref(),
                        context,
                    )?;

                    if modified {
                        changed = true;
                        messages.push(format!("Modified user '{}'", name));
                    }
                }

                // Set password if provided
                if let Some(ref pwd) = password {
                    if context.check_mode {
                        messages.push("Would set password".to_string());
                        changed = true;
                    } else {
                        Self::set_password_via_connection(
                            connection,
                            &name,
                            pwd,
                            password_encrypted,
                            context,
                        )?;
                        messages.push("Set password".to_string());
                        changed = true;
                    }
                }

                // Lock or unlock password if specified
                if let Some(lock) = password_lock {
                    if context.check_mode {
                        let action = if lock { "lock" } else { "unlock" };
                        messages.push(format!("Would {} password", action));
                        changed = true;
                    } else {
                        let lock_changed = Self::set_password_lock_via_connection(
                            connection, &name, lock, context,
                        )?;
                        if lock_changed {
                            let action = if lock { "Locked" } else { "Unlocked" };
                            messages.push(format!("{} password", action));
                            changed = true;
                        }
                    }
                }

                // Generate SSH key if requested
                if generate_ssh_key {
                    if context.check_mode {
                        messages.push("Would generate SSH key".to_string());
                        changed = true;
                    } else {
                        let key_generated = Self::generate_ssh_key_via_connection(
                            connection,
                            &name,
                            &ssh_key_type,
                            ssh_key_bits,
                            ssh_key_file.as_deref(),
                            ssh_key_comment.as_deref(),
                            ssh_key_passphrase.as_deref(),
                            context,
                        )?;

                        if key_generated {
                            messages.push("Generated SSH key".to_string());
                            changed = true;
                        }
                    }
                }

                // Get final user info
                let user_info = Self::get_user_info_via_connection(connection, &name, context)?;
                let mut data = HashMap::new();

                if let Some(info) = user_info {
                    data.insert("uid".to_string(), serde_json::json!(info.uid));
                    data.insert("gid".to_string(), serde_json::json!(info.gid));
                    data.insert("home".to_string(), serde_json::json!(info.home));
                    data.insert("shell".to_string(), serde_json::json!(info.shell));
                    data.insert("groups".to_string(), serde_json::json!(info.groups));
                }

                let msg = if messages.is_empty() {
                    format!("User '{}' is in desired state", name)
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
    fn test_user_state_from_str() {
        assert_eq!(UserState::from_str("present").unwrap(), UserState::Present);
        assert_eq!(UserState::from_str("absent").unwrap(), UserState::Absent);
        assert!(UserState::from_str("invalid").is_err());
    }

    #[test]
    fn test_user_module_name() {
        let module = UserModule;
        assert_eq!(module.name(), "user");
    }

    #[test]
    fn test_user_module_classification() {
        let module = UserModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_user_module_required_params() {
        let module = UserModule;
        assert_eq!(module.required_params(), &["name"]);
    }
}
