//! Parallel filesystem client modules
//!
//! Manages Lustre and BeeGFS client installation and mount configuration.
//!
//! # Modules
//!
//! - `lustre_client`: Install Lustre client packages, load kernel module, manage fstab and mounts
//! - `beegfs_client`: Install BeeGFS client packages, configure management daemon, manage services and mounts

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
    ParallelizationHint,
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
    for line in os_release.lines() {
        if line.starts_with("ID_LIKE=") || line.starts_with("ID=") {
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
                return Some("rhel");
            } else if val.contains("debian") || val.contains("ubuntu") {
                return Some("debian");
            }
        }
    }
    None
}

// ---- Lustre Client Module ----

pub struct LustreClientModule;

impl Module for LustreClientModule {
    fn name(&self) -> &'static str {
        "lustre_client"
    }

    fn description(&self) -> &'static str {
        "Manage Lustre filesystem client installation and mounts"
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
            ModuleError::Unsupported("Unsupported OS for Lustre client module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let mount_point = params.get_string_required("mount_point")?;
        let nid = params.get_string_required("nid")?;
        let fs_name = params.get_string_required("fs_name")?;
        let mount_options = params
            .get_string("mount_options")?
            .unwrap_or_else(|| "defaults".to_string());

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        let fstab_entry = format!(
            "{}:/{} {} lustre {} 0 0",
            nid, fs_name, mount_point, mount_options
        );

        // -- state=absent --
        if state == "absent" {
            // Unmount if currently mounted
            let (is_mounted, _, _) = run_cmd(
                connection,
                &format!("mountpoint -q '{}'", mount_point),
                context,
            )?;

            if is_mounted {
                if context.check_mode {
                    changes.push(format!("Would unmount {}", mount_point));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("umount '{}'", mount_point),
                        context,
                    )?;
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
                    changes.push("Would remove Lustre fstab entry".to_string());
                } else {
                    run_cmd_ok(
                        connection,
                        &format!(
                            "sed -i '\\|{}:/{}|d' /etc/fstab",
                            nid, fs_name
                        ),
                        context,
                    )?;
                    changed = true;
                    changes.push("Removed Lustre fstab entry".to_string());
                }
            }

            // Optionally remove packages
            let check_cmd = match os_family {
                "rhel" => "rpm -q lustre-client >/dev/null 2>&1".to_string(),
                _ => "dpkg -s lustre-client-utils >/dev/null 2>&1".to_string(),
            };
            let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

            if installed {
                if context.check_mode {
                    changes.push("Would remove Lustre client packages".to_string());
                } else {
                    let remove_cmd = match os_family {
                        "rhel" => "dnf remove -y lustre-client".to_string(),
                        _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y lustre-client-utils".to_string(),
                    };
                    run_cmd_ok(connection, &remove_cmd, context)?;
                    changed = true;
                    changes.push("Removed Lustre client packages".to_string());
                }
            }

            if context.check_mode && !changes.is_empty() {
                return Ok(ModuleOutput::changed(format!(
                    "Would apply {} Lustre removal changes",
                    changes.len()
                ))
                .with_data("changes", serde_json::json!(changes)));
            }

            if changed {
                return Ok(
                    ModuleOutput::changed(format!("Removed Lustre client from {}", mount_point))
                        .with_data("changes", serde_json::json!(changes)),
                );
            }

            return Ok(ModuleOutput::ok("Lustre client is not present"));
        }

        // -- state=present --

        // Step 1: Install lustre-client packages
        let check_cmd = match os_family {
            "rhel" => "rpm -q lustre-client >/dev/null 2>&1".to_string(),
            _ => "dpkg -s lustre-client-utils >/dev/null 2>&1".to_string(),
        };
        let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install Lustre client packages".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y lustre-client".to_string(),
                    _ => {
                        // On Debian, we need the kernel-specific modules package
                        let kernel_ver = run_cmd_ok(connection, "uname -r", context)?;
                        let kernel_ver = kernel_ver.trim();
                        format!(
                            "DEBIAN_FRONTEND=noninteractive apt-get install -y lustre-client-modules-{} lustre-client-utils",
                            kernel_ver
                        )
                    }
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push("Installed Lustre client packages".to_string());
            }
        }

        // Step 2: Load lustre kernel module
        let (lustre_loaded, _, _) =
            run_cmd(connection, "lsmod | grep -q lustre", context)?;

        if !lustre_loaded {
            if context.check_mode {
                changes.push("Would load lustre kernel module".to_string());
            } else {
                run_cmd_ok(connection, "modprobe lustre", context)?;
                changed = true;
                changes.push("Loaded lustre kernel module".to_string());
            }
        }

        // Step 3: Create mount point directory
        let (dir_exists, _, _) = run_cmd(
            connection,
            &format!("test -d '{}'", mount_point),
            context,
        )?;

        if !dir_exists {
            if context.check_mode {
                changes.push(format!("Would create mount point {}", mount_point));
            } else {
                run_cmd_ok(
                    connection,
                    &format!("mkdir -p '{}'", mount_point),
                    context,
                )?;
                changed = true;
                changes.push(format!("Created mount point {}", mount_point));
            }
        }

        // Step 4: Manage fstab entry
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
                changed = true;
                changes.push(format!("Added fstab entry for {}", mount_point));
            }
        }

        // Step 5: Mount if not already mounted
        let (is_mounted, _, _) = run_cmd(
            connection,
            &format!("mountpoint -q '{}'", mount_point),
            context,
        )?;

        if !is_mounted {
            if context.check_mode {
                changes.push(format!("Would mount {}", mount_point));
            } else {
                run_cmd_ok(
                    connection,
                    &format!("mount '{}'", mount_point),
                    context,
                )?;
                changed = true;
                changes.push(format!("Mounted {}", mount_point));
            }
        }

        // Step 6: Health check (non-fatal)
        let mut health_status = serde_json::json!("unknown");
        if !context.check_mode && is_mounted {
            let (health_ok, health_stdout, _) = run_cmd(
                connection,
                &format!("lfs df '{}' 2>&1", mount_point),
                context,
            )?;
            if health_ok {
                health_status = serde_json::json!({
                    "healthy": true,
                    "output": health_stdout.trim(),
                });
            } else {
                health_status = serde_json::json!({
                    "healthy": false,
                    "output": health_stdout.trim(),
                });
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Lustre client changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!(
                    "Applied {} Lustre client changes",
                    changes.len()
                ))
                .with_data("changes", serde_json::json!(changes))
                .with_data("mount_point", serde_json::json!(mount_point))
                .with_data("health", health_status),
            )
        } else {
            Ok(
                ModuleOutput::ok("Lustre client is configured")
                    .with_data("mount_point", serde_json::json!(mount_point))
                    .with_data("health", health_status),
            )
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["mount_point", "nid", "fs_name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("mount_options", serde_json::json!("defaults"));
        m
    }
}

// ---- BeeGFS Client Module ----

pub struct BeegfsClientModule;

impl Module for BeegfsClientModule {
    fn name(&self) -> &'static str {
        "beegfs_client"
    }

    fn description(&self) -> &'static str {
        "Manage BeeGFS filesystem client installation and mounts"
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
            ModuleError::Unsupported("Unsupported OS for BeeGFS client module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let mount_point = params.get_string_required("mount_point")?;
        let mgmtd_host = params.get_string_required("mgmtd_host")?;

        // Parse optional client_conf as a map of key/value overrides
        let client_conf: Option<HashMap<String, String>> = match params.get("client_conf") {
            Some(serde_json::Value::Object(map)) => {
                let mut conf = HashMap::new();
                for (k, v) in map {
                    let val_str = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string().trim_matches('"').to_string(),
                    };
                    conf.insert(k.clone(), val_str);
                }
                Some(conf)
            }
            _ => None,
        };

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        let beegfs_packages = "beegfs-client beegfs-helperd beegfs-utils";

        // -- state=absent --
        if state == "absent" {
            // Unmount if currently mounted
            let (is_mounted, _, _) = run_cmd(
                connection,
                &format!("mountpoint -q '{}'", mount_point),
                context,
            )?;

            if is_mounted {
                if context.check_mode {
                    changes.push(format!("Would unmount {}", mount_point));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("umount '{}'", mount_point),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Unmounted {}", mount_point));
                }
            }

            // Stop and disable services
            for svc in &["beegfs-client.service", "beegfs-helperd.service"] {
                let (active, _, _) = run_cmd(
                    connection,
                    &format!("systemctl is-active {}", svc),
                    context,
                )?;

                if active {
                    if context.check_mode {
                        changes.push(format!("Would stop {}", svc));
                    } else {
                        let _ = run_cmd(
                            connection,
                            &format!("systemctl stop {}", svc),
                            context,
                        );
                        let _ = run_cmd(
                            connection,
                            &format!("systemctl disable {}", svc),
                            context,
                        );
                        changed = true;
                        changes.push(format!("Stopped and disabled {}", svc));
                    }
                }
            }

            // Remove packages
            let check_cmd = match os_family {
                "rhel" => "rpm -q beegfs-client >/dev/null 2>&1".to_string(),
                _ => "dpkg -s beegfs-client >/dev/null 2>&1".to_string(),
            };
            let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

            if installed {
                if context.check_mode {
                    changes.push("Would remove BeeGFS packages".to_string());
                } else {
                    let remove_cmd = match os_family {
                        "rhel" => format!("dnf remove -y {}", beegfs_packages),
                        _ => format!(
                            "DEBIAN_FRONTEND=noninteractive apt-get remove -y {}",
                            beegfs_packages
                        ),
                    };
                    run_cmd_ok(connection, &remove_cmd, context)?;
                    changed = true;
                    changes.push("Removed BeeGFS packages".to_string());
                }
            }

            if context.check_mode && !changes.is_empty() {
                return Ok(ModuleOutput::changed(format!(
                    "Would apply {} BeeGFS removal changes",
                    changes.len()
                ))
                .with_data("changes", serde_json::json!(changes)));
            }

            if changed {
                return Ok(
                    ModuleOutput::changed(format!("Removed BeeGFS client from {}", mount_point))
                        .with_data("changes", serde_json::json!(changes)),
                );
            }

            return Ok(ModuleOutput::ok("BeeGFS client is not present"));
        }

        // -- state=present --

        // Step 1: Install BeeGFS packages
        let check_cmd = match os_family {
            "rhel" => "rpm -q beegfs-client >/dev/null 2>&1".to_string(),
            _ => "dpkg -s beegfs-client >/dev/null 2>&1".to_string(),
        };
        let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install BeeGFS packages".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => format!("dnf install -y {}", beegfs_packages),
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                        beegfs_packages
                    ),
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push("Installed BeeGFS packages".to_string());
            }
        }

        // Step 2: Configure /etc/beegfs/beegfs-client.conf
        if !context.check_mode {
            // Read current config
            let (_, current_conf, _) = run_cmd(
                connection,
                "cat /etc/beegfs/beegfs-client.conf 2>/dev/null || true",
                context,
            )?;

            // Set sysMgmtdHost
            let mgmtd_line = format!("sysMgmtdHost = {}", mgmtd_host);
            let conf_has_mgmtd = current_conf
                .lines()
                .any(|l| l.trim().starts_with("sysMgmtdHost") && l.contains(&mgmtd_host));

            if !conf_has_mgmtd {
                // Use sed to replace the sysMgmtdHost line, or append if missing
                let (has_key, _, _) = run_cmd(
                    connection,
                    "grep -q '^sysMgmtdHost' /etc/beegfs/beegfs-client.conf 2>/dev/null",
                    context,
                )?;

                if has_key {
                    run_cmd_ok(
                        connection,
                        &format!(
                            "sed -i 's|^sysMgmtdHost.*|{}|' /etc/beegfs/beegfs-client.conf",
                            mgmtd_line
                        ),
                        context,
                    )?;
                } else {
                    run_cmd_ok(
                        connection,
                        &format!(
                            "echo '{}' >> /etc/beegfs/beegfs-client.conf",
                            mgmtd_line
                        ),
                        context,
                    )?;
                }
                changed = true;
                changes.push(format!("Set sysMgmtdHost = {}", mgmtd_host));
            }

            // Apply any additional client_conf overrides
            if let Some(ref conf_map) = client_conf {
                for (key, value) in conf_map {
                    let desired_line = format!("{} = {}", key, value);
                    let already_set = current_conf.lines().any(|l| {
                        let trimmed = l.trim();
                        trimmed.starts_with(key.as_str())
                            && trimmed.contains('=')
                            && trimmed
                                .split('=')
                                .nth(1)
                                .map(|v| v.trim() == value.as_str())
                                .unwrap_or(false)
                    });

                    if !already_set {
                        let (has_key, _, _) = run_cmd(
                            connection,
                            &format!(
                                "grep -q '^{}' /etc/beegfs/beegfs-client.conf 2>/dev/null",
                                key
                            ),
                            context,
                        )?;

                        if has_key {
                            run_cmd_ok(
                                connection,
                                &format!(
                                    "sed -i 's|^{}.*|{}|' /etc/beegfs/beegfs-client.conf",
                                    key, desired_line
                                ),
                                context,
                            )?;
                        } else {
                            run_cmd_ok(
                                connection,
                                &format!(
                                    "echo '{}' >> /etc/beegfs/beegfs-client.conf",
                                    desired_line
                                ),
                                context,
                            )?;
                        }
                        changed = true;
                        changes.push(format!("Set {} = {}", key, value));
                    }
                }
            }
        } else {
            // check_mode: report config changes without applying
            changes.push(format!(
                "Would set sysMgmtdHost = {} in beegfs-client.conf",
                mgmtd_host
            ));
            if let Some(ref conf_map) = client_conf {
                for (key, value) in conf_map {
                    changes.push(format!("Would set {} = {} in beegfs-client.conf", key, value));
                }
            }
        }

        // Step 3: Build kernel module via beegfs-setup-client
        if !context.check_mode {
            let (setup_done, _, _) = run_cmd(
                connection,
                "test -f /etc/beegfs/beegfs-mounts.conf",
                context,
            )?;

            // Check if mount point is already configured in beegfs-mounts.conf
            let already_configured = if setup_done {
                let (found, _, _) = run_cmd(
                    connection,
                    &format!(
                        "grep -qF '{}' /etc/beegfs/beegfs-mounts.conf 2>/dev/null",
                        mount_point
                    ),
                    context,
                )?;
                found
            } else {
                false
            };

            if !already_configured {
                run_cmd_ok(
                    connection,
                    &format!(
                        "/opt/beegfs/sbin/beegfs-setup-client -m '{}'",
                        mount_point
                    ),
                    context,
                )?;
                changed = true;
                changes.push(format!("Ran beegfs-setup-client for {}", mount_point));
            }
        } else {
            changes.push(format!(
                "Would run beegfs-setup-client -m {}",
                mount_point
            ));
        }

        // Step 4: Enable and start beegfs-helperd and beegfs-client services
        for svc in &["beegfs-helperd.service", "beegfs-client.service"] {
            let (active, _, _) = run_cmd(
                connection,
                &format!("systemctl is-active {}", svc),
                context,
            )?;
            let (enabled, _, _) = run_cmd(
                connection,
                &format!("systemctl is-enabled {}", svc),
                context,
            )?;

            if !enabled {
                if context.check_mode {
                    changes.push(format!("Would enable {}", svc));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("systemctl enable {}", svc),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Enabled {}", svc));
                }
            }

            if !active {
                if context.check_mode {
                    changes.push(format!("Would start {}", svc));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("systemctl start {}", svc),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Started {}", svc));
                }
            }
        }

        // Step 5: Create mount point directory and mount
        let (dir_exists, _, _) = run_cmd(
            connection,
            &format!("test -d '{}'", mount_point),
            context,
        )?;

        if !dir_exists {
            if context.check_mode {
                changes.push(format!("Would create mount point {}", mount_point));
            } else {
                run_cmd_ok(
                    connection,
                    &format!("mkdir -p '{}'", mount_point),
                    context,
                )?;
                changed = true;
                changes.push(format!("Created mount point {}", mount_point));
            }
        }

        let (is_mounted, _, _) = run_cmd(
            connection,
            &format!("mountpoint -q '{}'", mount_point),
            context,
        )?;

        if !is_mounted {
            if context.check_mode {
                changes.push(format!("Would mount {}", mount_point));
            } else {
                run_cmd_ok(
                    connection,
                    &format!("mount '{}'", mount_point),
                    context,
                )?;
                changed = true;
                changes.push(format!("Mounted {}", mount_point));
            }
        }

        // Step 6: Health check (non-fatal)
        let mut health_status = serde_json::json!("unknown");
        if !context.check_mode && is_mounted {
            let (health_ok, health_stdout, _) = run_cmd(
                connection,
                "beegfs-check-servers 2>&1",
                context,
            )?;
            if health_ok {
                health_status = serde_json::json!({
                    "healthy": true,
                    "output": health_stdout.trim(),
                });
            } else {
                health_status = serde_json::json!({
                    "healthy": false,
                    "output": health_stdout.trim(),
                });
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} BeeGFS client changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!(
                    "Applied {} BeeGFS client changes",
                    changes.len()
                ))
                .with_data("changes", serde_json::json!(changes))
                .with_data("mount_point", serde_json::json!(mount_point))
                .with_data("health", health_status),
            )
        } else {
            Ok(
                ModuleOutput::ok("BeeGFS client is configured")
                    .with_data("mount_point", serde_json::json!(mount_point))
                    .with_data("health", health_status),
            )
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["mount_point", "mgmtd_host"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("client_conf", serde_json::json!({}));
        m
    }
}
