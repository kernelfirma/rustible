//! GPU management module
//!
//! Manages NVIDIA GPU driver installation, persistence mode, and
//! optional Slurm GRES configuration generation.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `driver_version` (optional): string - NVIDIA driver version (e.g., "535.129.03")
//! - `persistence_mode` (optional): bool (default: false) - enable persistence mode
//! - `compute_mode` (optional): string - default, exclusive_thread, exclusive_process, prohibited
//! - `ecc_mode` (optional): bool - enable/disable ECC (requires reboot)
//! - `power_limit` (optional): u32 - power limit in watts
//! - `gpu_id` (optional): string - GPU index or UUID for single-GPU operations
//! - `gres_config` (optional): bool (default: false) - generate GRES entries for Slurm

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

fn driver_branch(version: &str) -> Option<String> {
    let trimmed = version.trim();
    if trimmed.is_empty() {
        return None;
    }
    let branch = trimmed.split('.').next().unwrap_or("");
    if branch.is_empty() {
        None
    } else {
        Some(branch.to_string())
    }
}

fn normalize_compute_mode(mode: &str) -> ModuleResult<String> {
    let normalized = mode.trim().to_lowercase();
    if normalized.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "compute_mode cannot be empty".to_string(),
        ));
    }

    if let Ok(value) = normalized.parse::<u32>() {
        if value <= 3 {
            return Ok(value.to_string());
        }
    }

    match normalized.as_str() {
        "default" => Ok("0".to_string()),
        "exclusive_thread" => Ok("1".to_string()),
        "exclusive_process" => Ok("2".to_string()),
        "prohibited" => Ok("3".to_string()),
        _ => Err(ModuleError::InvalidParameter(format!(
            "compute_mode must be default, exclusive_thread, exclusive_process, prohibited, or 0-3 (got: {})",
            mode
        ))),
    }
}

fn gpu_selector_arg(gpu_id: &Option<String>) -> String {
    gpu_id
        .as_ref()
        .map(|id| format!(" -i {}", id))
        .unwrap_or_default()
}

fn parse_power_limit(value: &str) -> Option<f64> {
    let token = value.split_whitespace().next().unwrap_or("");
    token.parse::<f64>().ok()
}

fn parse_enabled_flag(value: &str) -> Option<bool> {
    let normalized = value.trim().to_lowercase();
    match normalized.as_str() {
        "enabled" | "on" | "yes" => Some(true),
        "disabled" | "off" | "no" => Some(false),
        _ => None,
    }
}

pub struct NvidiaGpuModule;

impl NvidiaGpuModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        os_family: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let check_cmd = match os_family {
            "rhel" => "rpm -q nvidia-driver >/dev/null 2>&1",
            _ => "dpkg -s nvidia-driver-535 >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            return Ok(ModuleOutput::ok("NVIDIA GPU drivers are not installed"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would remove NVIDIA GPU drivers"));
        }

        // Stop persistence daemon if running
        let _ = run_cmd(
            connection,
            "systemctl stop nvidia-persistenced.service",
            context,
        );
        let _ = run_cmd(
            connection,
            "systemctl disable nvidia-persistenced.service",
            context,
        );

        // Remove packages
        let remove_cmd = match os_family {
            "rhel" => "dnf remove -y nvidia-driver nvidia-driver-cuda",
            _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y nvidia-driver-535",
        };
        run_cmd_ok(connection, remove_cmd, context)?;

        Ok(ModuleOutput::changed("Removed NVIDIA GPU drivers"))
    }

    /// Query nvidia-smi for GPU inventory, returning parsed GPU data.
    fn query_gpu_inventory(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<Vec<serde_json::Value>> {
        let output = run_cmd_ok(
            connection,
            "nvidia-smi --query-gpu=index,name,uuid,pci.bus_id,memory.total --format=csv,noheader,nounits",
            context,
        )?;

        let mut gpus = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(5, ',').collect();
            if parts.len() >= 5 {
                gpus.push(serde_json::json!({
                    "index": parts[0].trim(),
                    "name": parts[1].trim(),
                    "uuid": parts[2].trim(),
                    "pci_bus_id": parts[3].trim(),
                    "memory_total_mib": parts[4].trim(),
                }));
            }
        }
        Ok(gpus)
    }

    fn query_single_value(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        query: &str,
        gpu_id: &Option<String>,
    ) -> ModuleResult<Vec<String>> {
        let selector = gpu_selector_arg(gpu_id);
        let output = run_cmd_ok(
            connection,
            &format!(
                "nvidia-smi{} --query-gpu={} --format=csv,noheader",
                selector, query
            ),
            context,
        )?;
        Ok(output
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect())
    }

    /// Generate Slurm GRES config lines from GPU inventory.
    fn generate_gres_lines(&self, gpus: &[serde_json::Value]) -> Vec<String> {
        gpus.iter()
            .map(|gpu| {
                let name = gpu["name"].as_str().unwrap_or("gpu");
                let index = gpu["index"].as_str().unwrap_or("0");
                // Format: Name=gpu Type=<model> File=/dev/nvidia<index>
                format!(
                    "Name=gpu Type={} File=/dev/nvidia{}",
                    name.replace(' ', "_"),
                    index,
                )
            })
            .collect()
    }
}

impl Module for NvidiaGpuModule {
    fn name(&self) -> &'static str {
        "nvidia_gpu"
    }

    fn description(&self) -> &'static str {
        "Manage NVIDIA GPU driver installation, persistence mode, and Slurm GRES configuration"
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
        let driver_version = params.get_string("driver_version")?;
        let persistence_mode = params.get_bool("persistence_mode")?;
        let compute_mode = params.get_string("compute_mode")?;
        let ecc_mode = params.get_bool("ecc_mode")?;
        let power_limit = params.get_u32("power_limit")?;
        let gpu_id = params.get_string("gpu_id")?;
        let gres_config = params.get_bool_or("gres_config", false);

        // Detect OS family
        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported(
                "Unsupported OS. NVIDIA GPU module supports RHEL-family and Debian-family distributions."
                    .to_string(),
            )
        })?;

        if state == "absent" {
            return self.handle_absent(connection, os_family, context);
        }

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // --- Install NVIDIA driver packages ---
        let desired_branch = driver_version
            .as_deref()
            .and_then(driver_branch)
            .unwrap_or_else(|| "535".to_string());
        let check_cmd = match os_family {
            "rhel" => {
                if driver_version.is_some() {
                    format!("rpm -q nvidia-driver-{}xx >/dev/null 2>&1", desired_branch)
                } else {
                    "rpm -q nvidia-driver nvidia-driver-cuda >/dev/null 2>&1".to_string()
                }
            }
            _ => format!("dpkg -s nvidia-driver-{} >/dev/null 2>&1", desired_branch),
        };
        let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push(format!(
                    "Would install NVIDIA driver packages (branch {})",
                    desired_branch
                ));
            } else {
                let install_cmd = match os_family {
                    "rhel" => {
                        if driver_version.is_some() {
                            format!("dnf install -y nvidia-driver-{}xx", desired_branch)
                        } else {
                            "dnf install -y nvidia-driver nvidia-driver-cuda".to_string()
                        }
                    }
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y nvidia-driver-{}",
                        desired_branch
                    ),
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push(format!(
                    "Installed NVIDIA driver packages (branch {})",
                    desired_branch
                ));
            }
        }

        // --- Verify nvidia-smi works and collect GPU inventory ---
        let gpus = if !context.check_mode {
            match self.query_gpu_inventory(connection, context) {
                Ok(gpus) => gpus,
                Err(e) => {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "nvidia-smi verification failed after driver install: {}. A reboot may be required.",
                        e
                    )));
                }
            }
        } else {
            Vec::new()
        };

        // --- Persistence mode ---
        if let Some(persistence_mode) = persistence_mode {
            let selector = gpu_selector_arg(&gpu_id);
            let current = self
                .query_single_value(connection, context, "persistence_mode", &gpu_id)
                .unwrap_or_default();
            let current_enabled = current.first().and_then(|value| parse_enabled_flag(value));

            if current_enabled != Some(persistence_mode) {
                if context.check_mode {
                    changes.push(format!(
                        "Would set persistence_mode={}{}",
                        persistence_mode, selector
                    ));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!(
                            "nvidia-smi{} -pm {}",
                            selector,
                            if persistence_mode { 1 } else { 0 }
                        ),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!(
                        "Set persistence_mode={}{}",
                        persistence_mode, selector
                    ));
                }
            }

            if persistence_mode {
                let (svc_active, _, _) = run_cmd(
                    connection,
                    "systemctl is-active nvidia-persistenced.service",
                    context,
                )?;
                let (svc_enabled, _, _) = run_cmd(
                    connection,
                    "systemctl is-enabled nvidia-persistenced.service",
                    context,
                )?;

                if !svc_active || !svc_enabled {
                    if context.check_mode {
                        changes
                            .push("Would enable and start nvidia-persistenced.service".to_string());
                    } else {
                        run_cmd_ok(
                            connection,
                            "systemctl enable --now nvidia-persistenced.service",
                            context,
                        )?;
                        changed = true;
                        changes.push("Enabled and started nvidia-persistenced.service".to_string());
                    }
                }
            } else {
                let (svc_active, _, _) = run_cmd(
                    connection,
                    "systemctl is-active nvidia-persistenced.service",
                    context,
                )?;
                if svc_active {
                    if context.check_mode {
                        changes.push("Would stop nvidia-persistenced.service".to_string());
                    } else {
                        run_cmd_ok(
                            connection,
                            "systemctl disable --now nvidia-persistenced.service",
                            context,
                        )?;
                        changed = true;
                        changes.push("Stopped nvidia-persistenced.service".to_string());
                    }
                }
            }
        }

        // --- Compute mode ---
        if let Some(mode) = compute_mode.as_deref() {
            let selector = gpu_selector_arg(&gpu_id);
            let desired_mode = normalize_compute_mode(mode)?;
            let desired_label = mode.trim().to_lowercase();
            let current = self
                .query_single_value(connection, context, "compute_mode", &gpu_id)
                .unwrap_or_default();
            let current_mode = current.first().map(|value| value.trim().to_lowercase());

            if current_mode.as_deref() != Some(desired_label.as_str()) {
                if context.check_mode {
                    changes.push(format!("Would set compute_mode={}{}", mode, selector));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("nvidia-smi{} -c {}", selector, desired_mode),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Set compute_mode={}{}", mode, selector));
                }
            }
        }

        // --- ECC mode ---
        if let Some(ecc_mode) = ecc_mode {
            let selector = gpu_selector_arg(&gpu_id);
            let current = self
                .query_single_value(connection, context, "ecc.mode.current", &gpu_id)
                .unwrap_or_default();
            let current_enabled = current.first().and_then(|value| parse_enabled_flag(value));

            if current_enabled != Some(ecc_mode) {
                if context.check_mode {
                    changes.push(format!("Would set ecc_mode={}{}", ecc_mode, selector));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("nvidia-smi{} -e {}", selector, if ecc_mode { 1 } else { 0 }),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Set ecc_mode={}{}", ecc_mode, selector));
                }
            }
        }

        // --- Power limit ---
        if let Some(power_limit) = power_limit {
            let selector = gpu_selector_arg(&gpu_id);
            let current = self
                .query_single_value(connection, context, "power.limit", &gpu_id)
                .unwrap_or_default();
            let current_limit = current.first().and_then(|value| parse_power_limit(value));

            let needs_update = current_limit
                .map(|limit| (limit - power_limit as f64).abs() > 0.1)
                .unwrap_or(true);

            if needs_update {
                if context.check_mode {
                    changes.push(format!("Would set power_limit={}{}", power_limit, selector));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("nvidia-smi{} -pl {}", selector, power_limit),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Set power_limit={}{}", power_limit, selector));
                }
            }
        }

        // --- GRES config generation ---
        let mut gres_lines: Vec<String> = Vec::new();
        if gres_config {
            if context.check_mode {
                changes.push("Would generate Slurm GRES config from GPU inventory".to_string());
            } else {
                gres_lines = self.generate_gres_lines(&gpus);
                if !gres_lines.is_empty() {
                    changes.push(format!(
                        "Generated {} GRES config entries",
                        gres_lines.len()
                    ));
                }
            }
        }

        // --- Build output ---
        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} NVIDIA GPU changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("os_family", serde_json::json!(os_family)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!("Applied {} NVIDIA GPU changes", changes.len()))
        } else {
            ModuleOutput::ok("NVIDIA GPU drivers are installed and configured")
        };

        output = output
            .with_data("changes", serde_json::json!(changes))
            .with_data("os_family", serde_json::json!(os_family))
            .with_data("gpu_count", serde_json::json!(gpus.len()))
            .with_data("gpus", serde_json::json!(gpus));

        if gres_config && !gres_lines.is_empty() {
            output = output.with_data("gres_config", serde_json::json!(gres_lines));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("driver_version", serde_json::json!(null));
        m.insert("persistence_mode", serde_json::json!(null));
        m.insert("compute_mode", serde_json::json!(null));
        m.insert("ecc_mode", serde_json::json!(null));
        m.insert("power_limit", serde_json::json!(null));
        m.insert("gpu_id", serde_json::json!(null));
        m.insert("gres_config", serde_json::json!(false));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::{driver_branch, normalize_compute_mode};

    #[test]
    fn test_driver_branch_parses_versions() {
        assert_eq!(driver_branch("535.129.03"), Some("535".to_string()));
        assert_eq!(driver_branch("550"), Some("550".to_string()));
        assert_eq!(driver_branch(""), None);
    }

    #[test]
    fn test_normalize_compute_mode() {
        assert_eq!(normalize_compute_mode("default").unwrap(), "0");
        assert_eq!(normalize_compute_mode("exclusive_thread").unwrap(), "1");
        assert_eq!(normalize_compute_mode("exclusive_process").unwrap(), "2");
        assert_eq!(normalize_compute_mode("prohibited").unwrap(), "3");
        assert_eq!(normalize_compute_mode("2").unwrap(), "2");
    }
}
