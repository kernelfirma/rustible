//! Dedicated LNet-aware Lustre mount management
//!
//! Manages Lustre filesystem mounts with LNet NID configuration,
//! mount options, and fstab persistence. This module focuses specifically
//! on mount lifecycle management, complementing `lustre_client` which
//! handles package installation and basic mounts.
//!
//! # Parameters
//!
//! - `nid` (required): LNet NID address (e.g., "10.0.0.1@tcp")
//! - `fs_name` (required): Lustre filesystem name
//! - `mount_point` (required): Target mount point path
//! - `mount_options` (optional): Mount options (default: "defaults")
//! - `fstab` (optional): Whether to manage fstab entry (default: true)
//! - `state` (optional): "mounted" (default), "unmounted", "absent"

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

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

pub struct LustreMountModule;

impl Module for LustreMountModule {
    fn name(&self) -> &'static str {
        "lustre_mount"
    }

    fn description(&self) -> &'static str {
        "Manage LNet-aware Lustre filesystem mounts with fstab persistence"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::HostExclusive
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

        let nid = params.get_string_required("nid")?;
        let fs_name = params.get_string_required("fs_name")?;
        let mount_point = params.get_string_required("mount_point")?;
        let mount_options = params
            .get_string("mount_options")?
            .unwrap_or_else(|| "defaults".to_string());
        let manage_fstab = params.get_bool_or("fstab", true);
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "mounted".to_string());

        let lustre_source = format!("{}:/{}", nid, fs_name);

        match state.as_str() {
            "absent" => self.handle_absent(
                connection,
                context,
                &lustre_source,
                &nid,
                &fs_name,
                &mount_point,
            ),
            "unmounted" => self.handle_unmounted(
                connection,
                context,
                &lustre_source,
                &nid,
                &fs_name,
                &mount_point,
                &mount_options,
                manage_fstab,
            ),
            "mounted" => self.handle_mounted(
                connection,
                context,
                &lustre_source,
                &nid,
                &fs_name,
                &mount_point,
                &mount_options,
                manage_fstab,
            ),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'mounted', 'unmounted', or 'absent'",
                state
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["nid", "fs_name", "mount_point"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("mount_options", serde_json::json!("defaults"));
        m.insert("fstab", serde_json::json!(true));
        m.insert("state", serde_json::json!("mounted"));
        m
    }
}

impl LustreMountModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        _lustre_source: &str,
        nid: &str,
        fs_name: &str,
        mount_point: &str,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Unmount if mounted
        let (is_mounted, _, _) = run_cmd(
            connection,
            &format!("mountpoint -q '{}'", mount_point),
            context,
        )?;

        if is_mounted {
            if context.check_mode {
                changes.push(format!("Would unmount {}", mount_point));
            } else {
                run_cmd_ok(connection, &format!("umount '{}'", mount_point), context)?;
                changed = true;
                changes.push(format!("Unmounted {}", mount_point));
            }
        }

        // Remove fstab entry
        let (in_fstab, _, _) = run_cmd(
            connection,
            &format!("grep -qF '{}:/{} ' /etc/fstab", nid, fs_name),
            context,
        )?;

        if in_fstab {
            if context.check_mode {
                changes.push("Would remove fstab entry".to_string());
            } else {
                run_cmd_ok(
                    connection,
                    &format!("sed -i '\\|{}:/{}|d' /etc/fstab", nid, fs_name),
                    context,
                )?;
                changed = true;
                changes.push("Removed fstab entry".to_string());
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(
                ModuleOutput::changed(format!("Would remove Lustre mount {}", mount_point))
                    .with_data("changes", serde_json::json!(changes)),
            );
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Removed Lustre mount {}", mount_point))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok("Lustre mount is already absent"))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_unmounted(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        _lustre_source: &str,
        nid: &str,
        fs_name: &str,
        mount_point: &str,
        mount_options: &str,
        manage_fstab: bool,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Unmount if mounted
        let (is_mounted, _, _) = run_cmd(
            connection,
            &format!("mountpoint -q '{}'", mount_point),
            context,
        )?;

        if is_mounted {
            if context.check_mode {
                changes.push(format!("Would unmount {}", mount_point));
            } else {
                run_cmd_ok(connection, &format!("umount '{}'", mount_point), context)?;
                changed = true;
                changes.push(format!("Unmounted {}", mount_point));
            }
        }

        // Ensure fstab entry exists (but don't mount)
        if manage_fstab {
            self.ensure_fstab(
                connection,
                context,
                nid,
                fs_name,
                mount_point,
                mount_options,
                &mut changed,
                &mut changes,
            )?;
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Lustre mount changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} Lustre mount changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok(
                "Lustre mount is in desired state (unmounted)",
            ))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_mounted(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        lustre_source: &str,
        nid: &str,
        fs_name: &str,
        mount_point: &str,
        mount_options: &str,
        manage_fstab: bool,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Ensure mount point directory exists
        let (dir_exists, _, _) =
            run_cmd(connection, &format!("test -d '{}'", mount_point), context)?;

        if !dir_exists {
            if context.check_mode {
                changes.push(format!("Would create mount point {}", mount_point));
            } else {
                run_cmd_ok(connection, &format!("mkdir -p '{}'", mount_point), context)?;
                changed = true;
                changes.push(format!("Created mount point {}", mount_point));
            }
        }

        // Ensure lustre kernel module is loaded
        let (lustre_loaded, _, _) = run_cmd(connection, "lsmod | grep -q lustre", context)?;

        if !lustre_loaded {
            if context.check_mode {
                changes.push("Would load lustre kernel module".to_string());
            } else {
                run_cmd_ok(connection, "modprobe lustre", context)?;
                changed = true;
                changes.push("Loaded lustre kernel module".to_string());
            }
        }

        // Manage fstab entry
        if manage_fstab {
            self.ensure_fstab(
                connection,
                context,
                nid,
                fs_name,
                mount_point,
                mount_options,
                &mut changed,
                &mut changes,
            )?;
        }

        // Mount if not already mounted
        let (is_mounted, _, _) = run_cmd(
            connection,
            &format!("mountpoint -q '{}'", mount_point),
            context,
        )?;

        if !is_mounted {
            if context.check_mode {
                changes.push(format!("Would mount {} at {}", lustre_source, mount_point));
            } else {
                let mount_cmd = format!(
                    "mount -t lustre -o '{}' '{}' '{}'",
                    mount_options, lustre_source, mount_point
                );
                run_cmd_ok(connection, &mount_cmd, context)?;
                changed = true;
                changes.push(format!("Mounted {} at {}", lustre_source, mount_point));
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Lustre mount changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} Lustre mount changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("mount_point", serde_json::json!(mount_point))
                    .with_data("source", serde_json::json!(lustre_source)),
            )
        } else {
            Ok(ModuleOutput::ok("Lustre mount is in desired state")
                .with_data("mount_point", serde_json::json!(mount_point))
                .with_data("source", serde_json::json!(lustre_source)))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ensure_fstab(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        nid: &str,
        fs_name: &str,
        mount_point: &str,
        mount_options: &str,
        changed: &mut bool,
        changes: &mut Vec<String>,
    ) -> ModuleResult<()> {
        let fstab_entry = format!(
            "{}:/{} {} lustre {} 0 0",
            nid, fs_name, mount_point, mount_options
        );

        let (in_fstab, _, _) = run_cmd(
            connection,
            &format!("grep -qF '{}:/{} ' /etc/fstab", nid, fs_name),
            context,
        )?;

        if !in_fstab {
            if context.check_mode {
                changes.push(format!("Would add fstab entry for {}", mount_point));
            } else {
                run_cmd_ok(
                    connection,
                    &format!("echo '{}' >> /etc/fstab", fstab_entry),
                    context,
                )?;
                *changed = true;
                changes.push(format!("Added fstab entry for {}", mount_point));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_name_and_description() {
        let module = LustreMountModule;
        assert_eq!(module.name(), "lustre_mount");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = LustreMountModule;
        let required = module.required_params();
        assert!(required.contains(&"nid"));
        assert!(required.contains(&"fs_name"));
        assert!(required.contains(&"mount_point"));
    }

    #[test]
    fn test_optional_params() {
        let module = LustreMountModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("mount_options"));
        assert!(optional.contains_key("fstab"));
        assert!(optional.contains_key("state"));
    }

    #[test]
    fn test_parallelization_hint() {
        let module = LustreMountModule;
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }
}
