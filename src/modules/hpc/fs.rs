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
                    changes.push("Would remove Lustre fstab entry".to_string());
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("sed -i '\\|{}:/{}|d' /etc/fstab", nid, fs_name),
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
                        _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y lustre-client-utils"
                            .to_string(),
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
                return Ok(ModuleOutput::changed(format!(
                    "Removed Lustre client from {}",
                    mount_point
                ))
                .with_data("changes", serde_json::json!(changes)));
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

        // Step 3: Create mount point directory
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
                run_cmd_ok(connection, &format!("mount '{}'", mount_point), context)?;
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
                ModuleOutput::changed(format!("Applied {} Lustre client changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("mount_point", serde_json::json!(mount_point))
                    .with_data("health", health_status),
            )
        } else {
            Ok(ModuleOutput::ok("Lustre client is configured")
                .with_data("mount_point", serde_json::json!(mount_point))
                .with_data("health", health_status))
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

#[derive(Debug, serde::Serialize)]
struct BeegfsPreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct BeegfsDriftItem {
    field: String,
    desired: String,
    actual: String,
}

#[derive(Debug, serde::Serialize)]
struct BeegfsVerifyResult {
    verified: bool,
    details: Vec<String>,
    warnings: Vec<String>,
}

/// Check BeeGFS client kernel module compatibility against the running kernel.
///
/// Runs `modinfo beegfs` to extract the module version and `uname -r` for the
/// kernel version, then compares them to flag potential incompatibilities.
fn check_beegfs_kernel_compat(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<BeegfsPreflightResult> {
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Get kernel version
    let kernel_ver = match run_cmd(connection, "uname -r", context) {
        Ok((true, stdout, _)) => stdout.trim().to_string(),
        Ok((false, _, stderr)) => {
            errors.push(format!("Failed to get kernel version: {}", stderr.trim()));
            return Ok(BeegfsPreflightResult {
                passed: false,
                warnings,
                errors,
            });
        }
        Err(e) => {
            errors.push(format!("Failed to get kernel version: {}", e));
            return Ok(BeegfsPreflightResult {
                passed: false,
                warnings,
                errors,
            });
        }
    };

    // Get BeeGFS module info
    let (mod_ok, mod_stdout, mod_stderr) =
        run_cmd(connection, "modinfo beegfs 2>/dev/null", context)?;

    if !mod_ok {
        // Module not loaded or not available -- this is a warning, not fatal
        warnings.push(format!(
            "BeeGFS kernel module not found (modinfo failed): {}",
            mod_stderr.trim()
        ));
        return Ok(BeegfsPreflightResult {
            passed: true,
            warnings,
            errors,
        });
    }

    // Extract version from modinfo output (look for "version:" line)
    let beegfs_ver = mod_stdout
        .lines()
        .find(|l| l.starts_with("version:") || l.starts_with("vermagic:"))
        .map(|l| l.split_whitespace().nth(1).unwrap_or("").to_string())
        .unwrap_or_default();

    if beegfs_ver.is_empty() {
        warnings.push("Could not determine BeeGFS module version from modinfo".to_string());
        return Ok(BeegfsPreflightResult {
            passed: true,
            warnings,
            errors,
        });
    }

    // Check vermagic line for kernel version match
    let vermagic_line = mod_stdout
        .lines()
        .find(|l| l.starts_with("vermagic:"))
        .unwrap_or("");

    if !vermagic_line.is_empty() && !vermagic_line.contains(&kernel_ver) {
        warnings.push(format!(
            "BeeGFS module vermagic '{}' does not match running kernel '{}'",
            vermagic_line.trim(),
            kernel_ver
        ));
    }

    let passed = errors.is_empty();
    Ok(BeegfsPreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Parse a beegfs-client.conf file (key=value format, # comments) and compare
/// against desired configuration values.  Returns a list of drift items where
/// the current value differs from the desired value.
fn reconcile_beegfs_config(
    conf_content: &str,
    desired: &HashMap<String, String>,
) -> Vec<BeegfsDriftItem> {
    // Parse existing config into a map
    let mut current: HashMap<String, String> = HashMap::new();
    for line in conf_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim().to_string();
            let value = trimmed[eq_pos + 1..].trim().to_string();
            if !key.is_empty() {
                current.insert(key, value);
            }
        }
    }

    let mut drift: Vec<BeegfsDriftItem> = Vec::new();
    for (key, desired_val) in desired {
        let actual_val = current.get(key).cloned().unwrap_or_default();
        if actual_val != *desired_val {
            drift.push(BeegfsDriftItem {
                field: key.clone(),
                desired: desired_val.clone(),
                actual: actual_val,
            });
        }
    }

    // Sort by field name for deterministic output
    drift.sort_by(|a, b| a.field.cmp(&b.field));
    drift
}

/// Collect BeeGFS health information from `beegfs-ctl --listtargets` and
/// `beegfs-net`, returning structured JSON data.
fn beegfs_health_output(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<serde_json::Value> {
    // Collect storage target states
    let (targets_ok, targets_stdout, _) = run_cmd(
        connection,
        "beegfs-ctl --listtargets --nodetype=storage --state 2>&1",
        context,
    )?;

    // Collect network status
    let (net_ok, net_stdout, _) = run_cmd(connection, "beegfs-net 2>&1", context)?;

    // Parse target lines -- typical format:
    //   TargetID   NodeID   State
    //   1          node01   Good
    let mut targets: Vec<serde_json::Value> = Vec::new();
    if targets_ok {
        let mut in_data = false;
        for line in targets_stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Skip header line(s) -- detect start of data by checking if first
            // token is numeric
            if !in_data {
                if trimmed
                    .split_whitespace()
                    .next()
                    .map(|t| t.chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or(false)
                {
                    in_data = true;
                } else {
                    continue;
                }
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 3 {
                targets.push(serde_json::json!({
                    "target_id": parts[0],
                    "node_id": parts[1],
                    "state": parts[2],
                }));
            }
        }
    }

    let all_good = targets
        .iter()
        .all(|t| t.get("state").and_then(|s| s.as_str()) == Some("Good"));

    Ok(serde_json::json!({
        "healthy": targets_ok && net_ok && all_good,
        "targets": targets,
        "targets_raw": if targets_ok { targets_stdout.trim() } else { "" },
        "network_raw": if net_ok { net_stdout.trim() } else { "" },
    }))
}

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
        let repo_url = params.get_string("repo_url")?;
        let conn_test = params.get_bool_or("conn_test", false);

        // Parse optional tuning as a map of sysctl-style tuning overrides
        let tuning: Option<HashMap<String, String>> = match params.get("tuning") {
            Some(serde_json::Value::Object(map)) => {
                let mut t = HashMap::new();
                for (k, v) in map {
                    let val_str = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string().trim_matches('"').to_string(),
                    };
                    t.insert(k.clone(), val_str);
                }
                Some(t)
            }
            _ => None,
        };

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
            // Step 1: Stop beegfs-client service first (before unmount)
            // Order matters: stop client service -> unmount -> stop helperd -> remove -> clean
            for svc in &["beegfs-client.service", "beegfs-helperd.service"] {
                let (active, _, _) =
                    run_cmd(connection, &format!("systemctl is-active {}", svc), context)?;

                if active {
                    if context.check_mode {
                        changes.push(format!("Would stop and disable {}", svc));
                    } else {
                        let _ = run_cmd(connection, &format!("systemctl stop {}", svc), context);
                        let _ = run_cmd(connection, &format!("systemctl disable {}", svc), context);
                        changed = true;
                        changes.push(format!("Stopped and disabled {}", svc));
                    }
                }
            }

            // Step 2: Unmount all BeeGFS filesystems (not just the specified mount_point)
            let (_, mount_out, _) =
                run_cmd(connection, "mount -t beegfs 2>/dev/null || true", context)?;
            let beegfs_mounts: Vec<String> = mount_out
                .lines()
                .filter_map(|l| {
                    // Format: beegfs_nodev on /mnt/beegfs type beegfs ...
                    let parts: Vec<&str> = l.split_whitespace().collect();
                    if parts.len() >= 3 {
                        Some(parts[2].to_string())
                    } else {
                        None
                    }
                })
                .collect();

            // Always attempt to unmount the specified mount point too
            let mut all_mounts = beegfs_mounts;
            if !all_mounts.contains(&mount_point) {
                // Check if specified mount is mounted (could be via fstab as different type)
                let (is_mounted, _, _) = run_cmd(
                    connection,
                    &format!("mountpoint -q '{}'", mount_point),
                    context,
                )?;
                if is_mounted {
                    all_mounts.push(mount_point.clone());
                }
            }

            for mnt in &all_mounts {
                if context.check_mode {
                    changes.push(format!("Would unmount {}", mnt));
                } else {
                    let _ = run_cmd(
                        connection,
                        &format!("umount '{}' 2>/dev/null", mnt),
                        context,
                    );
                    changed = true;
                    changes.push(format!("Unmounted {}", mnt));
                }
            }

            // Step 3: Remove packages
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

            // Step 4: Clean BeeGFS configuration files from /etc/beegfs/
            let (conf_exists, _, _) = run_cmd(connection, "test -d /etc/beegfs", context)?;

            if conf_exists {
                if context.check_mode {
                    changes.push("Would remove /etc/beegfs/ configuration directory".to_string());
                } else {
                    run_cmd_ok(connection, "rm -rf /etc/beegfs/", context)?;
                    changed = true;
                    changes.push("Removed /etc/beegfs/ configuration directory".to_string());
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
                return Ok(ModuleOutput::changed(format!(
                    "Removed BeeGFS client from {}",
                    mount_point
                ))
                .with_data("changes", serde_json::json!(changes)));
            }

            return Ok(ModuleOutput::ok("BeeGFS client is not present"));
        }

        // -- state=present --

        // Preflight: kernel compatibility check (non-fatal, adds warnings)
        let preflight = check_beegfs_kernel_compat(connection, context)?;
        if !preflight.warnings.is_empty() || !preflight.errors.is_empty() {
            changes.push(format!(
                "Kernel compat check: passed={}, warnings={}, errors={}",
                preflight.passed,
                preflight.warnings.len(),
                preflight.errors.len()
            ));
        }

        // Step 0: Setup BeeGFS repository if repo_url is provided
        if let Some(ref url) = repo_url {
            if !context.check_mode {
                let repo_cmd = match os_family {
                    "rhel" => format!(
                        "cat > /etc/yum.repos.d/beegfs.repo << 'REPOEOF'\n[beegfs]\nname=BeeGFS\nbaseurl={}\nenabled=1\ngpgcheck=0\nREPOEOF",
                        url
                    ),
                    _ => format!(
                        "echo 'deb [trusted=yes] {} ./' > /etc/apt/sources.list.d/beegfs.list && apt-get update -qq",
                        url
                    ),
                };
                let (repo_ok, _, _) = run_cmd(connection, &repo_cmd, context)?;
                if repo_ok {
                    changed = true;
                    changes.push(format!("Configured BeeGFS repository: {}", url));
                }
            } else {
                changes.push(format!("Would configure BeeGFS repository: {}", url));
            }
        }

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

            // Build full desired config map for drift detection
            let mut desired_conf: HashMap<String, String> = HashMap::new();
            desired_conf.insert("sysMgmtdHost".to_string(), mgmtd_host.clone());
            if let Some(ref conf_map) = client_conf {
                for (k, v) in conf_map {
                    desired_conf.insert(k.clone(), v.clone());
                }
            }

            // Use reconcile_beegfs_config to detect drift
            let drift = reconcile_beegfs_config(&current_conf, &desired_conf);

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
                        &format!("echo '{}' >> /etc/beegfs/beegfs-client.conf", mgmtd_line),
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

            // Report any remaining drift (informational)
            if !drift.is_empty() {
                for item in &drift {
                    changes.push(format!(
                        "Config drift: {} (desired='{}', actual='{}')",
                        item.field, item.desired, item.actual
                    ));
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
                    changes.push(format!(
                        "Would set {} = {} in beegfs-client.conf",
                        key, value
                    ));
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
                    &format!("/opt/beegfs/sbin/beegfs-setup-client -m '{}'", mount_point),
                    context,
                )?;
                changed = true;
                changes.push(format!("Ran beegfs-setup-client for {}", mount_point));
            }
        } else {
            changes.push(format!("Would run beegfs-setup-client -m {}", mount_point));
        }

        // Step 4: Enable and start beegfs-helperd and beegfs-client services
        for svc in &["beegfs-helperd.service", "beegfs-client.service"] {
            let (active, _, _) =
                run_cmd(connection, &format!("systemctl is-active {}", svc), context)?;
            let (enabled, _, _) = run_cmd(
                connection,
                &format!("systemctl is-enabled {}", svc),
                context,
            )?;

            if !enabled {
                if context.check_mode {
                    changes.push(format!("Would enable {}", svc));
                } else {
                    run_cmd_ok(connection, &format!("systemctl enable {}", svc), context)?;
                    changed = true;
                    changes.push(format!("Enabled {}", svc));
                }
            }

            if !active {
                if context.check_mode {
                    changes.push(format!("Would start {}", svc));
                } else {
                    run_cmd_ok(connection, &format!("systemctl start {}", svc), context)?;
                    changed = true;
                    changes.push(format!("Started {}", svc));
                }
            }
        }

        // Step 5: Create mount point directory and mount
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

        // Step 6: Apply tuning parameters
        if let Some(ref tune_map) = tuning {
            for (key, value) in tune_map {
                if !context.check_mode {
                    let proc_path = format!(
                        "/proc/fs/beegfs/{}/tune_{}",
                        mount_point.trim_start_matches('/').replace('/', "_"),
                        key
                    );
                    let (_, current, _) = run_cmd(
                        connection,
                        &format!("cat '{}' 2>/dev/null || echo '__MISSING__'", proc_path),
                        context,
                    )?;
                    if current.trim() != value {
                        let (ok, _, _) = run_cmd(
                            connection,
                            &format!("echo '{}' > '{}' 2>/dev/null", value, proc_path),
                            context,
                        )?;
                        if ok {
                            changed = true;
                            changes.push(format!("Set tuning {} = {}", key, value));
                        }
                    }
                } else {
                    changes.push(format!("Would set tuning {} = {}", key, value));
                }
            }
        }

        // Step 7: Connection test
        if conn_test {
            if !context.check_mode {
                let (test_ok, test_stdout, _) = run_cmd(
                    connection,
                    &format!(
                        "beegfs-ctl --conntest --mount='{}' 2>&1 | tail -5",
                        mount_point
                    ),
                    context,
                )?;
                if !test_ok {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "BeeGFS connection test failed for {}: {}",
                        mount_point,
                        test_stdout.trim()
                    )));
                }
            } else {
                changes.push(format!("Would run connection test on {}", mount_point));
            }
        }

        // Step 8: Structured health check (non-fatal)
        let mut health_status = serde_json::json!("unknown");
        if !context.check_mode && is_mounted {
            match beegfs_health_output(connection, context) {
                Ok(health_data) => {
                    health_status = health_data;
                }
                Err(_) => {
                    health_status = serde_json::json!({
                        "healthy": false,
                        "error": "Failed to collect health data",
                    });
                }
            }
        }

        // Include preflight results in output
        let preflight_data = serde_json::json!(preflight);

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} BeeGFS client changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("preflight", preflight_data.clone()));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} BeeGFS client changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("mount_point", serde_json::json!(mount_point))
                    .with_data("health", health_status)
                    .with_data("preflight", preflight_data),
            )
        } else {
            Ok(ModuleOutput::ok("BeeGFS client is configured")
                .with_data("mount_point", serde_json::json!(mount_point))
                .with_data("health", health_status)
                .with_data("preflight", preflight_data))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["mount_point", "mgmtd_host"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("client_conf", serde_json::json!({}));
        m.insert("repo_url", serde_json::json!(null));
        m.insert("tuning", serde_json::json!({}));
        m.insert("conn_test", serde_json::json!(false));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lustre_module_name_and_description() {
        let module = LustreClientModule;
        assert_eq!(module.name(), "lustre_client");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_lustre_required_params() {
        let module = LustreClientModule;
        let required = module.required_params();
        assert!(required.contains(&"nid"));
        assert!(required.contains(&"fs_name"));
        assert!(required.contains(&"mount_point"));
    }

    #[test]
    fn test_lustre_optional_params() {
        let module = LustreClientModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
    }

    #[test]
    fn test_lustre_parallelization_hint() {
        let module = LustreClientModule;
        assert!(matches!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        ));
    }

    #[test]
    fn test_beegfs_module_name_and_description() {
        let module = BeegfsClientModule;
        assert_eq!(module.name(), "beegfs_client");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_beegfs_required_params() {
        let module = BeegfsClientModule;
        let required = module.required_params();
        assert!(required.contains(&"mount_point"));
        assert!(required.contains(&"mgmtd_host"));
    }

    #[test]
    fn test_beegfs_optional_params() {
        let module = BeegfsClientModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("client_conf"));
        assert!(optional.contains_key("repo_url"));
        assert!(optional.contains_key("tuning"));
        assert!(optional.contains_key("conn_test"));
    }

    #[test]
    fn test_beegfs_parallelization_hint() {
        let module = BeegfsClientModule;
        assert!(matches!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        ));
    }

    #[test]
    fn test_beegfs_config_parsing() {
        // Test reconcile_beegfs_config with typical beegfs-client.conf content
        let conf = "\
# This is a comment
sysMgmtdHost = mgmt01
connMaxInternodeNum = 12
tuneNumWorkers = 8

# Another comment
connNetFilterFile =
";
        let mut desired = HashMap::new();
        desired.insert("sysMgmtdHost".to_string(), "mgmt02".to_string());
        desired.insert("tuneNumWorkers".to_string(), "8".to_string());
        desired.insert("connMaxInternodeNum".to_string(), "24".to_string());
        desired.insert("sysACLsEnabled".to_string(), "true".to_string());

        let drift = reconcile_beegfs_config(conf, &desired);

        // sysMgmtdHost differs (mgmt01 vs mgmt02)
        assert!(drift
            .iter()
            .any(|d| d.field == "sysMgmtdHost" && d.desired == "mgmt02" && d.actual == "mgmt01"));

        // connMaxInternodeNum differs (12 vs 24)
        assert!(drift
            .iter()
            .any(|d| d.field == "connMaxInternodeNum" && d.desired == "24" && d.actual == "12"));

        // sysACLsEnabled is missing from config (actual should be empty)
        assert!(drift
            .iter()
            .any(|d| d.field == "sysACLsEnabled" && d.actual.is_empty()));

        // tuneNumWorkers matches, should NOT be in drift
        assert!(!drift.iter().any(|d| d.field == "tuneNumWorkers"));

        // Empty config should report all desired keys as drift
        let drift_empty = reconcile_beegfs_config("", &{
            let mut m = HashMap::new();
            m.insert("sysMgmtdHost".to_string(), "mgmt01".to_string());
            m
        });
        assert_eq!(drift_empty.len(), 1);
        assert_eq!(drift_empty[0].field, "sysMgmtdHost");
        assert_eq!(drift_empty[0].actual, "");

        // Config with only comments should also report drift
        let drift_comments =
            reconcile_beegfs_config("# sysMgmtdHost = oldhost\n# tuneNumWorkers = 4\n", &{
                let mut m = HashMap::new();
                m.insert("sysMgmtdHost".to_string(), "mgmt01".to_string());
                m
            });
        assert_eq!(drift_comments.len(), 1);
        assert_eq!(drift_comments[0].actual, "");
    }

    #[test]
    fn test_beegfs_health_structure() {
        // Test that BeegfsPreflightResult serializes to valid JSON with expected fields
        let result = BeegfsPreflightResult {
            passed: true,
            warnings: vec!["test warning".to_string()],
            errors: vec![],
        };
        let json = serde_json::json!(result);
        assert_eq!(json["passed"], true);
        assert_eq!(json["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(json["errors"].as_array().unwrap().len(), 0);

        // Test BeegfsDriftItem serialization
        let drift_item = BeegfsDriftItem {
            field: "sysMgmtdHost".to_string(),
            desired: "mgmt02".to_string(),
            actual: "mgmt01".to_string(),
        };
        let drift_json = serde_json::json!(drift_item);
        assert_eq!(drift_json["field"], "sysMgmtdHost");
        assert_eq!(drift_json["desired"], "mgmt02");
        assert_eq!(drift_json["actual"], "mgmt01");

        // Test BeegfsVerifyResult serialization
        let verify = BeegfsVerifyResult {
            verified: true,
            details: vec!["all targets healthy".to_string()],
            warnings: vec![],
        };
        let verify_json = serde_json::json!(verify);
        assert_eq!(verify_json["verified"], true);
        assert_eq!(verify_json["details"].as_array().unwrap().len(), 1);
        assert_eq!(verify_json["warnings"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_beegfs_kernel_compat_format() {
        // Test that BeegfsPreflightResult can represent various states

        // Case 1: Clean pass
        let clean = BeegfsPreflightResult {
            passed: true,
            warnings: vec![],
            errors: vec![],
        };
        assert!(clean.passed);
        assert!(clean.warnings.is_empty());
        assert!(clean.errors.is_empty());

        // Case 2: Pass with warnings (typical for version mismatch)
        let with_warnings = BeegfsPreflightResult {
            passed: true,
            warnings: vec![
                "BeeGFS module vermagic '5.15.0-1 SMP mod_unload' does not match running kernel '5.15.0-2'"
                    .to_string(),
            ],
            errors: vec![],
        };
        assert!(with_warnings.passed);
        assert_eq!(with_warnings.warnings.len(), 1);
        assert!(with_warnings.warnings[0].contains("vermagic"));

        // Case 3: Failed with errors
        let failed = BeegfsPreflightResult {
            passed: false,
            warnings: vec![],
            errors: vec!["Failed to get kernel version: command not found".to_string()],
        };
        assert!(!failed.passed);
        assert_eq!(failed.errors.len(), 1);

        // Verify all serialize to JSON correctly
        let json_clean = serde_json::json!(clean);
        let json_warn = serde_json::json!(with_warnings);
        let json_fail = serde_json::json!(failed);
        assert_eq!(json_clean["passed"], true);
        assert_eq!(json_warn["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(json_fail["passed"], false);
    }
}
