//! NVIDIA Data Center GPU Manager (DCGM) module
//!
//! Installs and configures DCGM for GPU health monitoring, diagnostics,
//! and optional Prometheus exporter integration.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `operation_mode` (optional): "standalone" (default) or "embedded"
//! - `health_watches` (optional): bool - enable all health monitoring (default: false)
//! - `diag_level` (optional): u32 - 1=quick, 2=medium, 3=long
//! - `exporter` (optional): bool - install dcgm-exporter (default: false)
//! - `exporter_port` (optional): u32 - Prometheus metrics port (default: 9400)
//! - `exporter_counters` (optional): string - custom counters CSV path

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
struct DcgmGpuInfo {
    gpu_id: String,
    name: String,
    pci_bus_id: String,
}

#[derive(Debug, serde::Serialize)]
struct DcgmDiagResult {
    passed: bool,
    level: u32,
    details: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct DcgmExporterConfig {
    port: u32,
    counters_file: Option<String>,
}

// ---- Helper functions ----

/// Validate the DCGM operation mode parameter.
fn validate_operation_mode(mode: &str) -> ModuleResult<()> {
    match mode {
        "standalone" | "embedded" => Ok(()),
        _ => Err(ModuleError::InvalidParameter(format!(
            "operation_mode must be 'standalone' or 'embedded', got: '{}'",
            mode
        ))),
    }
}

/// Parse `dcgmi discovery -l` output to extract GPU information.
///
/// Example output:
/// ```text
/// 1 GPU found.
///   GPU ID: 0 | Name: NVIDIA A100 | PCI Bus ID: 00000000:3B:00.0
/// ```
fn parse_dcgmi_discovery(output: &str) -> Vec<DcgmGpuInfo> {
    let mut gpus = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if !line.contains("GPU ID:") {
            continue;
        }
        let mut gpu_id = String::new();
        let mut name = String::new();
        let mut pci_bus_id = String::new();

        for part in line.split('|') {
            let part = part.trim();
            if let Some(val) = part.strip_prefix("GPU ID:") {
                gpu_id = val.trim().to_string();
            } else if let Some(val) = part.strip_prefix("Name:") {
                name = val.trim().to_string();
            } else if let Some(val) = part.strip_prefix("PCI Bus ID:") {
                pci_bus_id = val.trim().to_string();
            }
        }

        if !gpu_id.is_empty() {
            gpus.push(DcgmGpuInfo {
                gpu_id,
                name,
                pci_bus_id,
            });
        }
    }
    gpus
}

/// Parse `dcgmi diag -r <level>` output to determine pass/fail.
///
/// Example output:
/// ```text
/// +---------------------------+------------------------------------------------+
/// | Diagnostic                | Result                                         |
/// +===========================+================================================+
/// | Deployment                | Pass                                           |
/// | PCIe                      | Pass                                           |
/// | Memory                    | Fail - ECC error detected                      |
/// +---------------------------+------------------------------------------------+
/// ```
fn parse_dcgmi_diag(output: &str, level: u32) -> DcgmDiagResult {
    let mut passed = true;
    let mut details = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.contains("Diagnostic") || line.contains("Result") {
            continue;
        }
        // Parse table rows: "| TestName | Result |"
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 3 {
            let test_name = parts[1].trim();
            let result = parts[2].trim();
            if test_name.is_empty()
                || test_name.starts_with('-')
                || test_name.starts_with('=')
            {
                continue;
            }
            if result.to_lowercase().contains("fail") {
                passed = false;
            }
            details.push(format!("{}: {}", test_name, result));
        }
    }

    DcgmDiagResult {
        passed,
        level,
        details,
    }
}

// ---- DCGM Module ----

pub struct DcgmModule;

impl Module for DcgmModule {
    fn name(&self) -> &'static str {
        "dcgm"
    }

    fn description(&self) -> &'static str {
        "Install and configure NVIDIA Data Center GPU Manager (DCGM)"
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
            ModuleError::Unsupported("Unsupported OS for DCGM module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let operation_mode = params
            .get_string("operation_mode")?
            .unwrap_or_else(|| "standalone".to_string());
        let health_watches = params.get_bool_or("health_watches", false);
        let diag_level = params.get_u32("diag_level")?;
        let exporter = params.get_bool_or("exporter", false);
        let exporter_port = params
            .get_u32("exporter_port")?
            .unwrap_or(9400);
        let exporter_counters = params.get_string("exporter_counters")?;

        validate_operation_mode(&operation_mode)?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // -- state=absent --
        if state == "absent" {
            let (installed, _, _) = run_cmd(
                connection,
                "command -v dcgmi >/dev/null 2>&1",
                context,
            )?;

            if !installed {
                return Ok(ModuleOutput::ok("DCGM is not installed"));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed("Would remove DCGM packages"));
            }

            let _ = run_cmd(
                connection,
                "systemctl stop nvidia-dcgm.service",
                context,
            );
            let _ = run_cmd(
                connection,
                "systemctl disable nvidia-dcgm.service",
                context,
            );

            let remove_cmd = match os_family {
                "rhel" => "dnf remove -y datacenter-gpu-manager dcgm-exporter",
                _ => "DEBIAN_FRONTEND=noninteractive apt-get remove --purge -y datacenter-gpu-manager dcgm-exporter",
            };
            run_cmd_ok(connection, remove_cmd, context)?;

            return Ok(ModuleOutput::changed("Removed DCGM packages"));
        }

        // -- state=present --

        // Step 1: Install DCGM
        let (dcgm_installed, _, _) = run_cmd(
            connection,
            "command -v dcgmi >/dev/null 2>&1",
            context,
        )?;

        if !dcgm_installed {
            if context.check_mode {
                changes.push("Would install datacenter-gpu-manager".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y datacenter-gpu-manager",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y datacenter-gpu-manager",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed datacenter-gpu-manager".to_string());
            }
        }

        // Step 2: Enable and start service
        if !context.check_mode {
            let (svc_active, _, _) = run_cmd(
                connection,
                "systemctl is-active nvidia-dcgm.service",
                context,
            )?;
            let (svc_enabled, _, _) = run_cmd(
                connection,
                "systemctl is-enabled nvidia-dcgm.service",
                context,
            )?;

            if !svc_active || !svc_enabled {
                run_cmd_ok(
                    connection,
                    "systemctl enable --now nvidia-dcgm.service",
                    context,
                )?;
                changed = true;
                changes.push("Enabled and started nvidia-dcgm.service".to_string());
            }
        } else if !dcgm_installed {
            changes.push("Would enable and start nvidia-dcgm.service".to_string());
        }

        // Step 3: Set operation mode (standalone vs embedded)
        if operation_mode == "embedded" && !context.check_mode {
            // In embedded mode DCGM is loaded in-process; the standalone
            // hostengine service is not needed
            let (svc_active, _, _) = run_cmd(
                connection,
                "systemctl is-active nvidia-dcgm.service",
                context,
            )?;
            if svc_active {
                run_cmd_ok(
                    connection,
                    "systemctl stop nvidia-dcgm.service",
                    context,
                )?;
                changed = true;
                changes.push("Stopped nvidia-dcgm.service for embedded mode".to_string());
            }
        }

        // Step 4: Enable health watches
        if health_watches && !context.check_mode {
            let (ok, _, _) = run_cmd(connection, "dcgmi health -s a", context)?;
            if ok {
                changes.push("Enabled all DCGM health watches".to_string());
            }
        } else if health_watches && context.check_mode {
            changes.push("Would enable all DCGM health watches".to_string());
        }

        // Step 5: Run diagnostics if requested
        let diag_result = if let Some(level) = diag_level {
            if level < 1 || level > 3 {
                return Err(ModuleError::InvalidParameter(
                    "diag_level must be 1, 2, or 3".to_string(),
                ));
            }
            if context.check_mode {
                changes.push(format!("Would run DCGM diagnostics level {}", level));
                None
            } else {
                let diag_stdout = run_cmd_ok(
                    connection,
                    &format!("dcgmi diag -r {}", level),
                    context,
                )?;
                let result = parse_dcgmi_diag(&diag_stdout, level);
                if !result.passed {
                    changes.push(format!(
                        "DCGM diagnostics level {} reported failures",
                        level
                    ));
                } else {
                    changes.push(format!("DCGM diagnostics level {} passed", level));
                }
                Some(result)
            }
        } else {
            None
        };

        // Step 6: Install and configure dcgm-exporter
        if exporter {
            let (exporter_installed, _, _) = run_cmd(
                connection,
                "command -v dcgm-exporter >/dev/null 2>&1",
                context,
            )?;

            if !exporter_installed {
                if context.check_mode {
                    changes.push("Would install dcgm-exporter".to_string());
                } else {
                    let install_cmd = match os_family {
                        "rhel" => "dnf install -y dcgm-exporter",
                        _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y dcgm-exporter",
                    };
                    run_cmd_ok(connection, install_cmd, context)?;
                    changed = true;
                    changes.push("Installed dcgm-exporter".to_string());
                }
            }

            // Configure exporter port via environment override
            if !context.check_mode {
                let env_line = format!(
                    "DCGM_EXPORTER_LISTEN=:{}",
                    exporter_port
                );
                let env_file = "/etc/default/dcgm-exporter";
                let mut env_content = env_line.clone();
                if let Some(ref counters) = exporter_counters {
                    env_content.push_str(&format!(
                        "\nDCGM_EXPORTER_COLLECTORS={}",
                        counters
                    ));
                }
                run_cmd_ok(
                    connection,
                    &format!("echo '{}' > {}", env_content, env_file),
                    context,
                )?;

                let (exp_active, _, _) = run_cmd(
                    connection,
                    "systemctl is-active dcgm-exporter.service",
                    context,
                )?;
                if !exp_active {
                    run_cmd_ok(
                        connection,
                        "systemctl enable --now dcgm-exporter.service",
                        context,
                    )?;
                } else {
                    run_cmd_ok(
                        connection,
                        "systemctl restart dcgm-exporter.service",
                        context,
                    )?;
                }
                changed = true;
                changes.push(format!(
                    "Configured dcgm-exporter on port {}",
                    exporter_port
                ));
            }
        }

        // Step 7: Discovery
        let discovery = if !context.check_mode {
            let (ok, stdout, _) = run_cmd(connection, "dcgmi discovery -l", context)?;
            if ok {
                parse_dcgmi_discovery(&stdout)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Build output
        let exporter_config = if exporter {
            Some(DcgmExporterConfig {
                port: exporter_port,
                counters_file: exporter_counters,
            })
        } else {
            None
        };

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} DCGM changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!("Applied {} DCGM changes", changes.len()))
        } else {
            ModuleOutput::ok("DCGM is installed and configured")
        };

        output = output
            .with_data("changes", serde_json::json!(changes))
            .with_data("operation_mode", serde_json::json!(operation_mode))
            .with_data("gpus", serde_json::json!(discovery));

        if let Some(ref diag) = diag_result {
            output = output.with_data("diagnostics", serde_json::json!(diag));
        }
        if let Some(ref exp) = exporter_config {
            output = output.with_data("exporter", serde_json::json!(exp));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("operation_mode", serde_json::json!("standalone"));
        m.insert("health_watches", serde_json::json!(false));
        m.insert("diag_level", serde_json::json!(null));
        m.insert("exporter", serde_json::json!(false));
        m.insert("exporter_port", serde_json::json!(9400));
        m.insert("exporter_counters", serde_json::json!(null));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = DcgmModule;
        assert_eq!(module.name(), "dcgm");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params().len(), 0);
    }

    #[test]
    fn test_validate_operation_mode() {
        assert!(validate_operation_mode("standalone").is_ok());
        assert!(validate_operation_mode("embedded").is_ok());
        assert!(validate_operation_mode("invalid").is_err());
        assert!(validate_operation_mode("").is_err());
    }

    #[test]
    fn test_parse_dcgmi_discovery() {
        let output = r#"1 GPU found.
  GPU ID: 0 | Name: NVIDIA A100-SXM4-80GB | PCI Bus ID: 00000000:3B:00.0
"#;
        let gpus = parse_dcgmi_discovery(output);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].gpu_id, "0");
        assert_eq!(gpus[0].name, "NVIDIA A100-SXM4-80GB");
        assert_eq!(gpus[0].pci_bus_id, "00000000:3B:00.0");
    }

    #[test]
    fn test_parse_dcgmi_discovery_multi() {
        let output = r#"4 GPUs found.
  GPU ID: 0 | Name: NVIDIA A100 | PCI Bus ID: 00000000:3B:00.0
  GPU ID: 1 | Name: NVIDIA A100 | PCI Bus ID: 00000000:86:00.0
  GPU ID: 2 | Name: NVIDIA A100 | PCI Bus ID: 00000000:AF:00.0
  GPU ID: 3 | Name: NVIDIA A100 | PCI Bus ID: 00000000:D8:00.0
"#;
        let gpus = parse_dcgmi_discovery(output);
        assert_eq!(gpus.len(), 4);
        assert_eq!(gpus[3].gpu_id, "3");
    }

    #[test]
    fn test_parse_dcgmi_discovery_empty() {
        let gpus = parse_dcgmi_discovery("");
        assert!(gpus.is_empty());

        let gpus = parse_dcgmi_discovery("0 GPUs found.\n");
        assert!(gpus.is_empty());
    }

    #[test]
    fn test_parse_dcgmi_diag_pass() {
        let output = r#"+---------------------------+------------------------------------------------+
| Diagnostic                | Result                                         |
+===========================+================================================+
| Deployment                | Pass                                           |
| PCIe                      | Pass                                           |
| Memory                    | Pass                                           |
+---------------------------+------------------------------------------------+"#;
        let result = parse_dcgmi_diag(output, 1);
        assert!(result.passed);
        assert_eq!(result.level, 1);
        assert_eq!(result.details.len(), 3);
        assert!(result.details[0].contains("Deployment"));
        assert!(result.details[0].contains("Pass"));
    }

    #[test]
    fn test_parse_dcgmi_diag_fail() {
        let output = r#"+---------------------------+------------------------------------------------+
| Diagnostic                | Result                                         |
+===========================+================================================+
| Deployment                | Pass                                           |
| Memory                    | Fail - ECC error detected                      |
+---------------------------+------------------------------------------------+"#;
        let result = parse_dcgmi_diag(output, 2);
        assert!(!result.passed);
        assert_eq!(result.level, 2);
        assert_eq!(result.details.len(), 2);
        assert!(result.details[1].contains("Fail"));
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID_LIKE=\"rhel centos fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID=freebsd"), None);
    }

    #[test]
    fn test_dcgm_diag_result_serialization() {
        let result = DcgmDiagResult {
            passed: true,
            level: 1,
            details: vec!["Deployment: Pass".to_string()],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["passed"], true);
        assert_eq!(json["level"], 1);
    }

    #[test]
    fn test_dcgm_exporter_config_serialization() {
        let config = DcgmExporterConfig {
            port: 9400,
            counters_file: Some("/etc/dcgm-exporter/counters.csv".to_string()),
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["port"], 9400);
        assert_eq!(
            json["counters_file"],
            "/etc/dcgm-exporter/counters.csv"
        );
    }
}
