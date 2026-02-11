//! GPU management module
//!
//! Manages NVIDIA GPU driver installation, persistence mode, and
//! optional Slurm GRES configuration generation.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `persistence_mode` (optional): bool (default: false) - enable nvidia-persistenced
//! - `gres_config` (optional): bool (default: false) - generate GRES entries for Slurm

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

    /// Generate Slurm GRES config lines from GPU inventory.
    fn generate_gres_lines(
        &self,
        gpus: &[serde_json::Value],
    ) -> Vec<String> {
        gpus.iter()
            .filter_map(|gpu| {
                let name = gpu["name"].as_str().unwrap_or("gpu");
                let index = gpu["index"].as_str().unwrap_or("0");
                // Format: Name=gpu Type=<model> File=/dev/nvidia<index>
                Some(format!(
                    "Name=gpu Type={} File=/dev/nvidia{}",
                    name.replace(' ', "_"),
                    index,
                ))
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
        let persistence_mode = params.get_bool_or("persistence_mode", false);
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
        let check_cmd = match os_family {
            "rhel" => "rpm -q nvidia-driver nvidia-driver-cuda >/dev/null 2>&1",
            _ => "dpkg -s nvidia-driver-535 >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install NVIDIA driver packages".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y nvidia-driver nvidia-driver-cuda",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y nvidia-driver-535",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed NVIDIA driver packages".to_string());
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
                    changes.push(
                        "Would enable and start nvidia-persistenced.service".to_string(),
                    );
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
        m.insert("persistence_mode", serde_json::json!(false));
        m.insert("gres_config", serde_json::json!(false));
        m
    }
}
