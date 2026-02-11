//! HPC common baseline module
//!
//! Validates and applies HPC cluster baseline configuration including
//! system limits, sysctl parameters, required packages, directories,
//! and tuned profiles.
//!
//! # Parameters
//!
//! - `limits` (optional): List of `/etc/security/limits.d/` entries
//! - `sysctl` (optional): Map of sysctl key/value pairs
//! - `packages` (optional): List of baseline packages to install
//! - `directories` (optional): List of directories to ensure exist
//! - `tuned_profile` (optional): Tuned profile name to activate

use std::collections::HashMap;
use std::sync::Arc;

use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt, ParallelizationHint,
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

/// Detects the OS family from /etc/os-release content.
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

pub struct HpcBaselineModule;

impl Module for HpcBaselineModule {
    fn name(&self) -> &'static str {
        "hpc_baseline"
    }

    fn description(&self) -> &'static str {
        "Validate and apply HPC cluster baseline configuration (limits, sysctl, packages, directories, tuned)"
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
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        // Detect OS family
        let os_release_out = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_release_out).ok_or_else(|| {
            ModuleError::Unsupported(
                "Unsupported OS. HPC baseline supports RHEL-family and Debian-family distributions."
                    .to_string(),
            )
        })?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // --- Limits ---
        if let Some(limits) = params.get_vec_string("limits")? {
            if !limits.is_empty() {
                let content = limits.join("\n");
                if context.check_mode {
                    changes.push(format!("Would write {} limit entries", limits.len()));
                } else {
                    let (_, existing, _) = run_cmd(
                        connection,
                        "cat /etc/security/limits.d/99-hpc.conf 2>/dev/null || true",
                        context,
                    )?;
                    if existing.trim() != content.trim() {
                        run_cmd_ok(
                            connection,
                            &format!(
                                "printf '%s\\n' '{}' > /etc/security/limits.d/99-hpc.conf",
                                content.replace('\'', "'\\''")
                            ),
                            context,
                        )?;
                        changed = true;
                        changes.push(format!("Wrote {} limit entries", limits.len()));
                    }
                }
            }
        }

        // --- Sysctl ---
        if let Some(sysctl_val) = params.get("sysctl") {
            if let Some(sysctl_map) = sysctl_val.as_object() {
                for (key, value) in sysctl_map {
                    let desired = value.as_str().unwrap_or(&value.to_string()).to_string();
                    let (_, current, _) = run_cmd(
                        connection,
                        &format!("sysctl -n {} 2>/dev/null || true", key),
                        context,
                    )?;
                    if current.trim() != desired.trim() {
                        if context.check_mode {
                            changes.push(format!("Would set sysctl {}={}", key, desired));
                        } else {
                            run_cmd_ok(
                                connection,
                                &format!("sysctl -w {}={}", key, desired),
                                context,
                            )?;
                            // Persist
                            run_cmd_ok(
                                connection,
                                &format!(
                                    "grep -q '^{}=' /etc/sysctl.d/99-hpc.conf 2>/dev/null && \
                                     sed -i 's|^{}=.*|{}={}|' /etc/sysctl.d/99-hpc.conf || \
                                     echo '{}={}' >> /etc/sysctl.d/99-hpc.conf",
                                    key, key, key, desired, key, desired
                                ),
                                context,
                            )?;
                            changed = true;
                            changes.push(format!("Set sysctl {}={}", key, desired));
                        }
                    }
                }
            }
        }

        // --- Packages ---
        if let Some(packages) = params.get_vec_string("packages")? {
            if !packages.is_empty() {
                let pkg_cmd = match os_family {
                    "rhel" => format!("rpm -q {} >/dev/null 2>&1", packages.join(" ")),
                    _ => format!("dpkg -s {} >/dev/null 2>&1", packages.join(" ")),
                };
                let (installed, _, _) = run_cmd(connection, &pkg_cmd, context)?;
                if !installed {
                    if context.check_mode {
                        changes.push(format!("Would install packages: {}", packages.join(", ")));
                    } else {
                        let install_cmd = match os_family {
                            "rhel" => format!("dnf install -y {}", packages.join(" ")),
                            _ => format!(
                                "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                                packages.join(" ")
                            ),
                        };
                        run_cmd_ok(connection, &install_cmd, context)?;
                        changed = true;
                        changes.push(format!("Installed packages: {}", packages.join(", ")));
                    }
                }
            }
        }

        // --- Directories ---
        if let Some(directories) = params.get_vec_string("directories")? {
            for dir in &directories {
                let (exists, _, _) = run_cmd(
                    connection,
                    &format!("test -d '{}'", dir),
                    context,
                )?;
                if !exists {
                    if context.check_mode {
                        changes.push(format!("Would create directory {}", dir));
                    } else {
                        run_cmd_ok(connection, &format!("mkdir -p '{}'", dir), context)?;
                        changed = true;
                        changes.push(format!("Created directory {}", dir));
                    }
                }
            }
        }

        // --- Tuned profile ---
        if let Some(profile) = params.get_string("tuned_profile")? {
            let (_, current_profile, _) = run_cmd(
                connection,
                "tuned-adm active 2>/dev/null | awk '{print $NF}' || true",
                context,
            )?;
            if current_profile.trim() != profile {
                if context.check_mode {
                    changes.push(format!("Would activate tuned profile '{}'", profile));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("tuned-adm profile '{}'", profile),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Activated tuned profile '{}'", profile));
                }
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} baseline changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("os_family", serde_json::json!(os_family)));
        }

        if changed {
            Ok(ModuleOutput::changed(format!(
                "Applied {} baseline changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("os_family", serde_json::json!(os_family)))
        } else {
            Ok(ModuleOutput::ok("HPC baseline configuration is up to date")
                .with_data("os_family", serde_json::json!(os_family)))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("limits", serde_json::json!([]));
        m.insert("sysctl", serde_json::json!({}));
        m.insert("packages", serde_json::json!([]));
        m.insert("directories", serde_json::json!([]));
        m.insert("tuned_profile", serde_json::json!(null));
        m
    }
}
