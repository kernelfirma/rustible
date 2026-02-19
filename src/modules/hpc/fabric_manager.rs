//! NVIDIA Fabric Manager module
//!
//! Installs and configures NVIDIA Fabric Manager for NVSwitch-based multi-GPU
//! topologies (DGX, HGX). Ensures version matches the installed driver.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `driver_version` (optional): Driver version to match (auto-detected if omitted)
//! - `fabric_mode` (optional): "full_gpu_fabric" (default) or "shared_nvswitch"
//! - `fault_tolerance` (optional): Enable FM fault tolerance (default: true)
//! - `log_level` (optional): Logging verbosity 0-5 (default: 3)

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

// ---- Helper structs ----

#[derive(Debug, serde::Serialize)]
struct FabricManagerStatus {
    version: String,
    fabric_mode: String,
    fault_tolerance: bool,
    service_active: bool,
}

// ---- Helper functions ----

/// Parse driver version from `nvidia-smi` output.
///
/// Accepts the raw output of `nvidia-smi --query-gpu=driver_version --format=csv,noheader`.
fn parse_driver_version(nvidia_smi_output: &str) -> Option<String> {
    let version = nvidia_smi_output.trim();
    if version.is_empty() {
        return None;
    }
    // Take only the first line in case of multi-GPU output
    Some(
        version
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
    )
}

/// Validate fabric mode parameter.
fn validate_fabric_mode(mode: &str) -> ModuleResult<()> {
    match mode {
        "full_gpu_fabric" | "shared_nvswitch" => Ok(()),
        _ => Err(ModuleError::InvalidParameter(format!(
            "fabric_mode must be 'full_gpu_fabric' or 'shared_nvswitch', got: '{}'",
            mode
        ))),
    }
}

/// Generate Fabric Manager configuration file content.
fn generate_fm_config(fabric_mode: &str, fault_tolerance: bool, log_level: u32) -> String {
    let fm_mode = match fabric_mode {
        "shared_nvswitch" => 1,
        _ => 0, // full_gpu_fabric
    };

    format!(
        "# Fabric Manager configuration - managed by rustible\n\
         FM_MODE={}\n\
         FM_STAY_RESIDENT_ON_FAILURES=1\n\
         FM_FAULT_TOLERANCE={}\n\
         LOG_LEVEL={}\n\
         LOG_FILE_NAME=/var/log/fabricmanager.log\n\
         LOG_APPEND_TO_LOG=1\n\
         DAEMONIZE=1\n",
        fm_mode,
        if fault_tolerance { 1 } else { 0 },
        log_level,
    )
}

// ---- Fabric Manager Module ----

pub struct FabricManagerModule;

impl Module for FabricManagerModule {
    fn name(&self) -> &'static str {
        "fabric_manager"
    }

    fn description(&self) -> &'static str {
        "Install and configure NVIDIA Fabric Manager for NVSwitch topologies"
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
            ModuleError::Unsupported("Unsupported OS for Fabric Manager module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let driver_version_param = params.get_string("driver_version")?;
        let fabric_mode = params
            .get_string("fabric_mode")?
            .unwrap_or_else(|| "full_gpu_fabric".to_string());
        let fault_tolerance = params.get_bool_or("fault_tolerance", true);
        let log_level = params.get_u32("log_level")?.unwrap_or(3);

        validate_fabric_mode(&fabric_mode)?;

        if log_level > 5 {
            return Err(ModuleError::InvalidParameter(
                "log_level must be between 0 and 5".to_string(),
            ));
        }

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // -- state=absent --
        if state == "absent" {
            let (installed, _, _) = run_cmd(
                connection,
                "systemctl list-unit-files nvidia-fabricmanager.service >/dev/null 2>&1",
                context,
            )?;

            if !installed {
                return Ok(ModuleOutput::ok("Fabric Manager is not installed"));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed("Would remove Fabric Manager"));
            }

            let _ = run_cmd(
                connection,
                "systemctl stop nvidia-fabricmanager.service",
                context,
            );
            let _ = run_cmd(
                connection,
                "systemctl disable nvidia-fabricmanager.service",
                context,
            );

            let remove_cmd = match os_family {
                "rhel" => "dnf remove -y 'nvidia-fabricmanager*'",
                _ => "DEBIAN_FRONTEND=noninteractive apt-get remove --purge -y 'nvidia-fabricmanager*'",
            };
            run_cmd_ok(connection, remove_cmd, context)?;

            return Ok(ModuleOutput::changed("Removed Fabric Manager"));
        }

        // -- state=present --

        // Step 1: Detect driver version
        let driver_version = if let Some(v) = driver_version_param {
            v
        } else {
            let (ok, stdout, _) = run_cmd(
                connection,
                "nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null",
                context,
            )?;
            if !ok {
                return Err(ModuleError::ExecutionFailed(
                    "Cannot detect driver version via nvidia-smi; provide driver_version parameter"
                        .to_string(),
                ));
            }
            parse_driver_version(&stdout).ok_or_else(|| {
                ModuleError::ExecutionFailed(
                    "Failed to parse driver version from nvidia-smi output".to_string(),
                )
            })?
        };

        // Step 2: Install Fabric Manager matching driver version
        let pkg_name = match os_family {
            "rhel" => format!("nvidia-fabricmanager-{}", driver_version),
            _ => format!("nvidia-fabricmanager-{}", driver_version),
        };

        let check_cmd = match os_family {
            "rhel" => format!("rpm -q {} >/dev/null 2>&1", pkg_name),
            _ => format!("dpkg -s {} >/dev/null 2>&1", pkg_name),
        };
        let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push(format!("Would install {}", pkg_name));
            } else {
                let install_cmd = match os_family {
                    "rhel" => format!("dnf install -y {}", pkg_name),
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                        pkg_name
                    ),
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push(format!("Installed {}", pkg_name));
            }
        }

        // Step 3: Write configuration
        let config_path = "/etc/nvidia-fabricmanager/fabricmanager.cfg";
        let config_content = generate_fm_config(&fabric_mode, fault_tolerance, log_level);

        if !context.check_mode {
            run_cmd_ok(
                connection,
                "mkdir -p /etc/nvidia-fabricmanager",
                context,
            )?;

            // Check if config differs
            let (exists, current_content, _) = run_cmd(
                connection,
                &format!("cat {} 2>/dev/null", config_path),
                context,
            )?;

            if !exists || current_content != config_content {
                let escaped = config_content.replace('\'', "'\\''");
                run_cmd_ok(
                    connection,
                    &format!("printf '%s' '{}' > {}", escaped, config_path),
                    context,
                )?;
                changed = true;
                changes.push("Updated fabricmanager.cfg".to_string());
            }
        } else {
            changes.push("Would write fabricmanager.cfg".to_string());
        }

        // Step 4: Enable and start service
        if !context.check_mode {
            let (svc_active, _, _) = run_cmd(
                connection,
                "systemctl is-active nvidia-fabricmanager.service",
                context,
            )?;
            let (svc_enabled, _, _) = run_cmd(
                connection,
                "systemctl is-enabled nvidia-fabricmanager.service",
                context,
            )?;

            if !svc_active || !svc_enabled {
                run_cmd_ok(
                    connection,
                    "systemctl enable --now nvidia-fabricmanager.service",
                    context,
                )?;
                changed = true;
                changes.push("Enabled and started nvidia-fabricmanager.service".to_string());
            } else if changed {
                // Restart if config changed
                run_cmd_ok(
                    connection,
                    "systemctl restart nvidia-fabricmanager.service",
                    context,
                )?;
                changes.push("Restarted nvidia-fabricmanager.service".to_string());
            }
        } else if !installed {
            changes.push("Would enable and start nvidia-fabricmanager.service".to_string());
        }

        // Build output
        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Fabric Manager changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let status = FabricManagerStatus {
            version: driver_version.clone(),
            fabric_mode: fabric_mode.clone(),
            fault_tolerance,
            service_active: !context.check_mode,
        };

        let mut output = if changed {
            ModuleOutput::changed(format!(
                "Applied {} Fabric Manager changes",
                changes.len()
            ))
        } else {
            ModuleOutput::ok("Fabric Manager is installed and configured")
        };

        output = output
            .with_data("changes", serde_json::json!(changes))
            .with_data("driver_version", serde_json::json!(driver_version))
            .with_data("status", serde_json::json!(status));

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("driver_version", serde_json::json!(null));
        m.insert("fabric_mode", serde_json::json!("full_gpu_fabric"));
        m.insert("fault_tolerance", serde_json::json!(true));
        m.insert("log_level", serde_json::json!(3));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = FabricManagerModule;
        assert_eq!(module.name(), "fabric_manager");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params().len(), 0);
    }

    #[test]
    fn test_parse_driver_version() {
        assert_eq!(
            parse_driver_version("535.183.01"),
            Some("535.183.01".to_string())
        );
        assert_eq!(
            parse_driver_version("550.120\n535.183.01\n"),
            Some("550.120".to_string())
        );
        assert_eq!(parse_driver_version(""), None);
        assert_eq!(
            parse_driver_version("  535.183.01  \n"),
            Some("535.183.01".to_string())
        );
    }

    #[test]
    fn test_validate_fabric_mode() {
        assert!(validate_fabric_mode("full_gpu_fabric").is_ok());
        assert!(validate_fabric_mode("shared_nvswitch").is_ok());
        assert!(validate_fabric_mode("invalid").is_err());
        assert!(validate_fabric_mode("").is_err());
    }

    #[test]
    fn test_generate_fm_config() {
        let config = generate_fm_config("full_gpu_fabric", true, 3);
        assert!(config.contains("FM_MODE=0"));
        assert!(config.contains("FM_FAULT_TOLERANCE=1"));
        assert!(config.contains("LOG_LEVEL=3"));
        assert!(config.contains("DAEMONIZE=1"));

        let config2 = generate_fm_config("shared_nvswitch", false, 5);
        assert!(config2.contains("FM_MODE=1"));
        assert!(config2.contains("FM_FAULT_TOLERANCE=0"));
        assert!(config2.contains("LOG_LEVEL=5"));
    }

    #[test]
    fn test_generate_fm_config_default_fields() {
        let config = generate_fm_config("full_gpu_fabric", true, 3);
        assert!(config.contains("FM_STAY_RESIDENT_ON_FAILURES=1"));
        assert!(config.contains("LOG_FILE_NAME=/var/log/fabricmanager.log"));
        assert!(config.contains("LOG_APPEND_TO_LOG=1"));
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID_LIKE=\"rhel centos fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID=freebsd"), None);
    }

    #[test]
    fn test_fabric_manager_status_serialization() {
        let status = FabricManagerStatus {
            version: "535.183.01".to_string(),
            fabric_mode: "full_gpu_fabric".to_string(),
            fault_tolerance: true,
            service_active: true,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["version"], "535.183.01");
        assert_eq!(json["fabric_mode"], "full_gpu_fabric");
        assert_eq!(json["fault_tolerance"], true);
        assert_eq!(json["service_active"], true);
    }
}
