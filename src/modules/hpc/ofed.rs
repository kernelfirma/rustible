//! OFED / RDMA / InfiniBand stack module
//!
//! Manages RDMA userland packages, kernel module loading, network sysctl
//! tuning, and IP over InfiniBand configuration.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `packages` (optional): additional RDMA packages to install
//! - `kernel_modules` (optional): list of kernel modules to load (default: ["ib_uverbs", "rdma_ucm", "ib_umad"])
//! - `sysctl` (optional): map of network sysctl overrides for RDMA tuning
//! - `ipoib` (optional): bool (default: false) - enable IP over InfiniBand

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

const DEFAULT_KERNEL_MODULES: &[&str] = &["ib_uverbs", "rdma_ucm", "ib_umad"];

pub struct RdmaStackModule;

impl RdmaStackModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        os_family: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let check_cmd = match os_family {
            "rhel" => "rpm -q rdma-core >/dev/null 2>&1",
            _ => "dpkg -s rdma-core >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            return Ok(ModuleOutput::ok("RDMA stack is not installed"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would remove RDMA stack"));
        }

        // Unload kernel modules (best-effort, ignore failures for in-use modules)
        for module in DEFAULT_KERNEL_MODULES.iter().rev() {
            let _ = run_cmd(
                connection,
                &format!("modprobe -r {} 2>/dev/null || true", module),
                context,
            );
        }
        let _ = run_cmd(
            connection,
            "modprobe -r ib_ipoib 2>/dev/null || true",
            context,
        );

        // Remove persistence config
        let _ = run_cmd(
            connection,
            "rm -f /etc/modules-load.d/rdma.conf",
            context,
        );

        // Remove packages
        let remove_cmd = match os_family {
            "rhel" => "dnf remove -y rdma-core libibverbs-utils",
            _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y rdma-core ibverbs-utils",
        };
        run_cmd_ok(connection, remove_cmd, context)?;

        Ok(ModuleOutput::changed("Removed RDMA stack"))
    }
}

impl Module for RdmaStackModule {
    fn name(&self) -> &'static str {
        "rdma_stack"
    }

    fn description(&self) -> &'static str {
        "Manage RDMA / InfiniBand / OFED userland stack, kernel modules, and network tuning"
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

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let extra_packages = params.get_vec_string("packages")?;
        let kernel_modules = params.get_vec_string("kernel_modules")?.unwrap_or_else(|| {
            DEFAULT_KERNEL_MODULES
                .iter()
                .map(|s| s.to_string())
                .collect()
        });
        let ipoib = params.get_bool_or("ipoib", false);

        // Detect OS family
        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported(
                "Unsupported OS. RDMA stack module supports RHEL-family and Debian-family distributions."
                    .to_string(),
            )
        })?;

        if state == "absent" {
            return self.handle_absent(connection, os_family, context);
        }

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // --- Install base RDMA packages ---
        let base_packages: Vec<&str> = match os_family {
            "rhel" => vec!["rdma-core", "libibverbs-utils"],
            _ => vec!["rdma-core", "ibverbs-utils"],
        };

        let check_cmd = match os_family {
            "rhel" => format!("rpm -q {} >/dev/null 2>&1", base_packages.join(" ")),
            _ => format!("dpkg -s {} >/dev/null 2>&1", base_packages.join(" ")),
        };
        let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install base RDMA packages".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => format!("dnf install -y {}", base_packages.join(" ")),
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                        base_packages.join(" ")
                    ),
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push("Installed base RDMA packages".to_string());
            }
        }

        // --- Install extra packages ---
        if let Some(ref extras) = extra_packages {
            if !extras.is_empty() {
                let extra_check = match os_family {
                    "rhel" => format!("rpm -q {} >/dev/null 2>&1", extras.join(" ")),
                    _ => format!("dpkg -s {} >/dev/null 2>&1", extras.join(" ")),
                };
                let (extras_installed, _, _) = run_cmd(connection, &extra_check, context)?;

                if !extras_installed {
                    if context.check_mode {
                        changes.push(format!(
                            "Would install additional RDMA packages: {}",
                            extras.join(", ")
                        ));
                    } else {
                        let install_cmd = match os_family {
                            "rhel" => format!("dnf install -y {}", extras.join(" ")),
                            _ => format!(
                                "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                                extras.join(" ")
                            ),
                        };
                        run_cmd_ok(connection, &install_cmd, context)?;
                        changed = true;
                        changes.push(format!(
                            "Installed additional RDMA packages: {}",
                            extras.join(", ")
                        ));
                    }
                }
            }
        }

        // --- Load kernel modules ---
        let mut all_modules = kernel_modules.clone();
        if ipoib && !all_modules.iter().any(|m| m == "ib_ipoib") {
            all_modules.push("ib_ipoib".to_string());
        }

        for module in &all_modules {
            let (loaded, _, _) = run_cmd(
                connection,
                &format!("lsmod | grep -qw '{}'", module),
                context,
            )?;
            if !loaded {
                if context.check_mode {
                    changes.push(format!("Would load kernel module {}", module));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("modprobe {}", module),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Loaded kernel module {}", module));
                }
            }
        }

        // --- Persist kernel modules in /etc/modules-load.d/rdma.conf ---
        let desired_conf = all_modules.join("\n");
        let (_, existing_conf, _) = run_cmd(
            connection,
            "cat /etc/modules-load.d/rdma.conf 2>/dev/null || true",
            context,
        )?;

        if existing_conf.trim() != desired_conf.trim() {
            if context.check_mode {
                changes.push("Would write /etc/modules-load.d/rdma.conf".to_string());
            } else {
                run_cmd_ok(
                    connection,
                    &format!(
                        "printf '%s\\n' '{}' > /etc/modules-load.d/rdma.conf",
                        desired_conf.replace('\'', "'\\''")
                    ),
                    context,
                )?;
                changed = true;
                changes.push("Wrote /etc/modules-load.d/rdma.conf".to_string());
            }
        }

        // --- Apply network sysctls for RDMA tuning ---
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
                            // Persist in sysctl.d
                            run_cmd_ok(
                                connection,
                                &format!(
                                    "grep -q '^{}=' /etc/sysctl.d/99-rdma.conf 2>/dev/null && \
                                     sed -i 's|^{}=.*|{}={}|' /etc/sysctl.d/99-rdma.conf || \
                                     echo '{}={}' >> /etc/sysctl.d/99-rdma.conf",
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

        // --- Build output ---
        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} RDMA stack changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("os_family", serde_json::json!(os_family)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} RDMA stack changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("os_family", serde_json::json!(os_family))
                    .with_data("kernel_modules", serde_json::json!(all_modules)),
            )
        } else {
            Ok(
                ModuleOutput::ok("RDMA stack is installed and configured")
                    .with_data("os_family", serde_json::json!(os_family))
                    .with_data("kernel_modules", serde_json::json!(all_modules)),
            )
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("packages", serde_json::json!(null));
        m.insert(
            "kernel_modules",
            serde_json::json!(["ib_uverbs", "rdma_ucm", "ib_umad"]),
        );
        m.insert("sysctl", serde_json::json!(null));
        m.insert("ipoib", serde_json::json!(false));
        m
    }
}
