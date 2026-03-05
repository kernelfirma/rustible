//! NFS shared storage modules
//!
//! Provides NFS server and client management for HPC clusters.
//!
//! # Modules
//!
//! - `nfs_server`: Manage NFS exports via `/etc/exports`
//! - `nfs_client`: Manage NFS client mounts via fstab

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

fn detect_os_family(os_release: &str) -> Option<&'static str> {
    let id_line = os_release
        .lines()
        .find(|l| l.starts_with("ID_LIKE=") || l.starts_with("ID="));
    match id_line {
        Some(line) => {
            let val = line
                .split('=')
                .nth(1)
                .unwrap_or("")
                .trim_matches('"')
                .to_lowercase();
            if val.contains("rhel")
                || val.contains("fedora")
                || val.contains("centos")
                || val == "rocky"
                || val == "almalinux"
            {
                Some("rhel")
            } else if val.contains("debian") || val.contains("ubuntu") {
                Some("debian")
            } else {
                None
            }
        }
        None => None,
    }
}

// ---- NFS Server Module ----

pub struct NfsServerModule;

impl Module for NfsServerModule {
    fn name(&self) -> &'static str {
        "nfs_server"
    }

    fn description(&self) -> &'static str {
        "Manage NFS server exports for HPC shared storage"
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

        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported("Unsupported OS for NFS server module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Determine package and service names
        let nfs_pkg = match os_family {
            "rhel" => "nfs-utils",
            _ => "nfs-kernel-server",
        };
        let nfs_svc = match os_family {
            "rhel" => "nfs-server.service",
            _ => "nfs-kernel-server.service",
        };

        // Check if installed
        let check_cmd = match os_family {
            "rhel" => format!("rpm -q {} >/dev/null 2>&1", nfs_pkg),
            _ => format!("dpkg -s {} >/dev/null 2>&1", nfs_pkg),
        };
        let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

        if state == "absent" {
            if !installed {
                return Ok(ModuleOutput::ok("NFS server is not installed"));
            }
            if context.check_mode {
                return Ok(ModuleOutput::changed("Would remove NFS server"));
            }
            let _ = run_cmd(connection, &format!("systemctl stop {}", nfs_svc), context);
            let _ = run_cmd(
                connection,
                &format!("systemctl disable {}", nfs_svc),
                context,
            );
            let remove_cmd = match os_family {
                "rhel" => format!("dnf remove -y {}", nfs_pkg),
                _ => format!(
                    "DEBIAN_FRONTEND=noninteractive apt-get remove -y {}",
                    nfs_pkg
                ),
            };
            run_cmd_ok(connection, &remove_cmd, context)?;
            return Ok(ModuleOutput::changed("Removed NFS server"));
        }

        // Install NFS server packages
        if !installed {
            if context.check_mode {
                changes.push(format!("Would install {}", nfs_pkg));
            } else {
                let install_cmd = match os_family {
                    "rhel" => format!("dnf install -y {}", nfs_pkg),
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                        nfs_pkg
                    ),
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push(format!("Installed {}", nfs_pkg));
            }
        }

        // Manage exports
        if let Some(exports) = params.get_vec_string("exports")? {
            if !exports.is_empty() {
                let desired = exports.join("\n") + "\n";
                let (_, current, _) =
                    run_cmd(connection, "cat /etc/exports 2>/dev/null || true", context)?;

                if current.trim() != desired.trim() {
                    if context.check_mode {
                        changes.push(format!(
                            "Would update /etc/exports with {} entries",
                            exports.len()
                        ));
                    } else {
                        run_cmd_ok(
                            connection,
                            &format!(
                                "printf '%s\\n' '{}' > /etc/exports",
                                desired.trim().replace('\'', "'\\''")
                            ),
                            context,
                        )?;
                        run_cmd_ok(connection, "exportfs -ra", context)?;
                        changed = true;
                        changes.push(format!(
                            "Updated /etc/exports with {} entries",
                            exports.len()
                        ));
                    }
                }
            }
        }

        // Enable NFS service
        let (active, _, _) = run_cmd(
            connection,
            &format!("systemctl is-active {}", nfs_svc),
            context,
        )?;
        if !active {
            if context.check_mode {
                changes.push(format!("Would enable and start {}", nfs_svc));
            } else {
                run_cmd_ok(
                    connection,
                    &format!("systemctl enable --now {}", nfs_svc),
                    context,
                )?;
                changed = true;
                changes.push(format!("Enabled and started {}", nfs_svc));
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} NFS server changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} NFS server changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok("NFS server is configured"))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("exports", serde_json::json!([]));
        m
    }
}

// ---- NFS Client Module ----

pub struct NfsClientModule;

impl Module for NfsClientModule {
    fn name(&self) -> &'static str {
        "nfs_client"
    }

    fn description(&self) -> &'static str {
        "Manage NFS client mounts for HPC shared storage"
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

        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported("Unsupported OS for NFS client module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "mounted".to_string());
        let server = params.get_string_required("server")?;
        let export_path = params.get_string_required("export")?;
        let mount_point = params.get_string_required("mount_point")?;
        let mount_options = params
            .get_string("mount_options")?
            .unwrap_or_else(|| "defaults,hard,intr".to_string());

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Install NFS client packages
        let nfs_pkg = match os_family {
            "rhel" => "nfs-utils",
            _ => "nfs-common",
        };
        let check_cmd = match os_family {
            "rhel" => format!("rpm -q {} >/dev/null 2>&1", nfs_pkg),
            _ => format!("dpkg -s {} >/dev/null 2>&1", nfs_pkg),
        };
        let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push(format!("Would install {}", nfs_pkg));
            } else {
                let install_cmd = match os_family {
                    "rhel" => format!("dnf install -y {}", nfs_pkg),
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                        nfs_pkg
                    ),
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push(format!("Installed {}", nfs_pkg));
            }
        }

        let _fstab_entry = format!(
            "{}:{} {} nfs {} 0 0",
            server, export_path, mount_point, mount_options
        );

        match state.as_str() {
            "mounted" => {
                // Ensure mount point directory exists
                let (dir_exists, _, _) =
                    run_cmd(connection, &format!("test -d '{}'", mount_point), context)?;
                if !dir_exists {
                    if !context.check_mode {
                        run_cmd_ok(connection, &format!("mkdir -p '{}'", mount_point), context)?;
                    }
                    changed = true;
                    changes.push(format!("Created mount point {}", mount_point));
                }

                // Check if fstab entry exists
                let (in_fstab, _, _) = run_cmd(
                    connection,
                    &format!("grep -qF '{}:{}' /etc/fstab", server, export_path),
                    context,
                )?;

                if !in_fstab {
                    if context.check_mode {
                        changes.push(format!("Would add fstab entry for {}", mount_point));
                    } else {
                        run_cmd_ok(
                            connection,
                            &format!("echo '{}' >> /etc/fstab", _fstab_entry),
                            context,
                        )?;
                        changed = true;
                        changes.push(format!("Added fstab entry for {}", mount_point));
                    }
                }

                // Mount if not already mounted
                let (is_mounted, _, _) = run_cmd(
                    connection,
                    &format!("mountpoint -q '{}'", mount_point),
                    context,
                )?;

                if !is_mounted {
                    if context.check_mode {
                        changes.push(format!("Would mount {}", mount_point));
                    } else {
                        run_cmd_ok(connection, &format!("mount '{}'", mount_point), context)?;
                        changed = true;
                        changes.push(format!("Mounted {}", mount_point));
                    }
                }
            }
            "unmounted" | "absent" => {
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

                if state == "absent" {
                    // Remove fstab entry
                    let (in_fstab, _, _) = run_cmd(
                        connection,
                        &format!("grep -qF '{}:{}' /etc/fstab", server, export_path),
                        context,
                    )?;

                    if in_fstab {
                        if context.check_mode {
                            changes.push("Would remove fstab entry".to_string());
                        } else {
                            run_cmd_ok(
                                connection,
                                &format!("sed -i '\\|{}:{}|d' /etc/fstab", server, export_path),
                                context,
                            )?;
                            changed = true;
                            changes.push("Removed fstab entry".to_string());
                        }
                    }
                }
            }
            _ => {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid state '{}'. Must be 'mounted', 'unmounted', or 'absent'",
                    state
                )));
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} NFS client changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} NFS client changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok("NFS client mount is configured"))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["server", "export", "mount_point"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("mounted"));
        m.insert("mount_options", serde_json::json!("defaults,hard,intr"));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_os_family, NfsClientModule, NfsServerModule};
    use crate::modules::Module;

    #[test]
    fn test_detect_os_family_for_nfs() {
        assert_eq!(detect_os_family("ID=ubuntu"), Some("debian"));
        assert_eq!(detect_os_family("ID_LIKE=\"rhel fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=freebsd"), None);
    }

    #[test]
    fn test_nfs_server_module_metadata() {
        let module = NfsServerModule;
        assert_eq!(module.name(), "nfs_server");
        assert!(!module.description().is_empty());
        assert!(module.required_params().is_empty());
        assert_eq!(
            module.optional_params().get("state"),
            Some(&serde_json::json!("present"))
        );
    }

    #[test]
    fn test_nfs_client_module_metadata_and_defaults() {
        let module = NfsClientModule;
        assert_eq!(module.name(), "nfs_client");
        assert_eq!(module.required_params(), ["server", "export", "mount_point"]);
        let optional = module.optional_params();
        assert_eq!(optional.get("state"), Some(&serde_json::json!("mounted")));
        assert_eq!(
            optional.get("mount_options"),
            Some(&serde_json::json!("defaults,hard,intr"))
        );
    }
}
