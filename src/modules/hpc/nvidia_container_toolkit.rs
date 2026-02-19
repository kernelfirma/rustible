//! NVIDIA Container Toolkit module
//!
//! Installs and configures the NVIDIA Container Toolkit for GPU-accelerated
//! container workloads with Docker, containerd, CRI-O, or Podman.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `runtime` (required): Container runtime - "docker", "containerd", "crio", "podman"
//! - `cdi` (optional): Generate CDI specs (default: true)
//! - `cdi_output` (optional): CDI output path (default: "/etc/cdi/nvidia.yaml")
//! - `toolkit_config` (optional): Custom config.toml path

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
struct ContainerToolkitStatus {
    runtime: String,
    cdi_enabled: bool,
    toolkit_version: Option<String>,
    gpu_count: Option<u32>,
}

// ---- Helper functions ----

/// Validate the container runtime parameter.
fn validate_runtime(runtime: &str) -> ModuleResult<()> {
    match runtime {
        "docker" | "containerd" | "crio" | "podman" => Ok(()),
        _ => Err(ModuleError::InvalidParameter(format!(
            "runtime must be 'docker', 'containerd', 'crio', or 'podman', got: '{}'",
            runtime
        ))),
    }
}

/// Map container runtime to its systemd service name.
fn runtime_service_name(runtime: &str) -> &'static str {
    match runtime {
        "docker" => "docker.service",
        "containerd" => "containerd.service",
        "crio" => "crio.service",
        "podman" => "podman.service",
        _ => "docker.service",
    }
}

/// Parse `nvidia-container-cli info` output to extract GPU count and driver version.
///
/// Example output:
/// ```text
/// NVRM version:   535.183.01
/// CUDA version:   12.2
/// Device count:   4
/// Device 0:       NVIDIA A100-SXM4-80GB (UUID: GPU-...)
/// ```
fn parse_container_cli_info(output: &str) -> ContainerToolkitStatus {
    let mut toolkit_version = None;
    let mut gpu_count = None;

    for line in output.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("NVRM version:") {
            toolkit_version = Some(val.trim().to_string());
        } else if let Some(val) = line.strip_prefix("Device count:") {
            gpu_count = val.trim().parse::<u32>().ok();
        }
    }

    ContainerToolkitStatus {
        runtime: String::new(),
        cdi_enabled: false,
        toolkit_version,
        gpu_count,
    }
}

// ---- NVIDIA Container Toolkit Module ----

pub struct NvidiaContainerToolkitModule;

impl Module for NvidiaContainerToolkitModule {
    fn name(&self) -> &'static str {
        "nvidia_container_toolkit"
    }

    fn description(&self) -> &'static str {
        "Install and configure NVIDIA Container Toolkit for GPU containers"
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
            ModuleError::Unsupported(
                "Unsupported OS for NVIDIA Container Toolkit module".to_string(),
            )
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let runtime = params.get_string_required("runtime")?;
        let cdi = params.get_bool_or("cdi", true);
        let cdi_output = params
            .get_string("cdi_output")?
            .unwrap_or_else(|| "/etc/cdi/nvidia.yaml".to_string());
        let toolkit_config = params.get_string("toolkit_config")?;

        validate_runtime(&runtime)?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // -- state=absent --
        if state == "absent" {
            let (installed, _, _) = run_cmd(
                connection,
                "command -v nvidia-ctk >/dev/null 2>&1",
                context,
            )?;

            if !installed {
                return Ok(ModuleOutput::ok("NVIDIA Container Toolkit is not installed"));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(
                    "Would remove NVIDIA Container Toolkit",
                ));
            }

            let remove_cmd = match os_family {
                "rhel" => "dnf remove -y nvidia-container-toolkit",
                _ => "DEBIAN_FRONTEND=noninteractive apt-get remove --purge -y nvidia-container-toolkit",
            };
            run_cmd_ok(connection, remove_cmd, context)?;

            return Ok(ModuleOutput::changed("Removed NVIDIA Container Toolkit"));
        }

        // -- state=present --

        // Step 1: Set up repository
        let (repo_exists, _, _) = match os_family {
            "rhel" => run_cmd(
                connection,
                "dnf repolist | grep -q nvidia-container-toolkit",
                context,
            )?,
            _ => run_cmd(
                connection,
                "test -f /etc/apt/sources.list.d/nvidia-container-toolkit.list",
                context,
            )?,
        };

        if !repo_exists {
            if context.check_mode {
                changes.push("Would add NVIDIA Container Toolkit repository".to_string());
            } else {
                match os_family {
                    "rhel" => {
                        run_cmd_ok(
                            connection,
                            "curl -s -L https://nvidia.github.io/libnvidia-container/stable/rpm/nvidia-container-toolkit.repo | tee /etc/yum.repos.d/nvidia-container-toolkit.repo",
                            context,
                        )?;
                    }
                    _ => {
                        run_cmd_ok(
                            connection,
                            "curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey | gpg --dearmor -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg",
                            context,
                        )?;
                        run_cmd_ok(
                            connection,
                            "curl -s -L https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list | sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g' | tee /etc/apt/sources.list.d/nvidia-container-toolkit.list",
                            context,
                        )?;
                        run_cmd_ok(connection, "apt-get update", context)?;
                    }
                }
                changed = true;
                changes.push("Added NVIDIA Container Toolkit repository".to_string());
            }
        }

        // Step 2: Install toolkit
        let (toolkit_installed, _, _) = run_cmd(
            connection,
            "command -v nvidia-ctk >/dev/null 2>&1",
            context,
        )?;

        if !toolkit_installed {
            if context.check_mode {
                changes.push("Would install nvidia-container-toolkit".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y nvidia-container-toolkit",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y nvidia-container-toolkit",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed nvidia-container-toolkit".to_string());
            }
        }

        // Step 3: Configure runtime
        if !context.check_mode {
            let configure_cmd = format!(
                "nvidia-ctk runtime configure --runtime={}",
                runtime
            );
            let (ok, _, _) = run_cmd(connection, &configure_cmd, context)?;
            if ok {
                changes.push(format!("Configured {} runtime for NVIDIA", runtime));
            }

            // Apply custom config if provided
            if let Some(ref config_path) = toolkit_config {
                run_cmd_ok(
                    connection,
                    &format!(
                        "cp {} /etc/nvidia-container-runtime/config.toml",
                        config_path
                    ),
                    context,
                )?;
                changed = true;
                changes.push(format!("Applied custom toolkit config from {}", config_path));
            }

            // Restart runtime service
            let service = runtime_service_name(&runtime);
            let (svc_active, _, _) = run_cmd(
                connection,
                &format!("systemctl is-active {}", service),
                context,
            )?;
            if svc_active {
                run_cmd_ok(
                    connection,
                    &format!("systemctl restart {}", service),
                    context,
                )?;
                changes.push(format!("Restarted {}", service));
            }
        } else {
            changes.push(format!("Would configure {} runtime", runtime));
        }

        // Step 4: Generate CDI specs
        if cdi {
            if context.check_mode {
                changes.push(format!("Would generate CDI spec at {}", cdi_output));
            } else {
                run_cmd_ok(
                    connection,
                    &format!("mkdir -p {}", cdi_output.rsplit_once('/').map(|(d, _)| d).unwrap_or("/etc/cdi")),
                    context,
                )?;
                let (ok, _, _) = run_cmd(
                    connection,
                    &format!("nvidia-ctk cdi generate --output={}", cdi_output),
                    context,
                )?;
                if ok {
                    changed = true;
                    changes.push(format!("Generated CDI spec at {}", cdi_output));
                }
            }
        }

        // Step 5: Collect status info
        let status = if !context.check_mode {
            let (ok, stdout, _) = run_cmd(
                connection,
                "nvidia-container-cli info 2>/dev/null",
                context,
            )?;
            if ok {
                let mut s = parse_container_cli_info(&stdout);
                s.runtime = runtime.clone();
                s.cdi_enabled = cdi;
                s
            } else {
                ContainerToolkitStatus {
                    runtime: runtime.clone(),
                    cdi_enabled: cdi,
                    toolkit_version: None,
                    gpu_count: None,
                }
            }
        } else {
            ContainerToolkitStatus {
                runtime: runtime.clone(),
                cdi_enabled: cdi,
                toolkit_version: None,
                gpu_count: None,
            }
        };

        // Build output
        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} NVIDIA Container Toolkit changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!(
                "Applied {} NVIDIA Container Toolkit changes",
                changes.len()
            ))
        } else {
            ModuleOutput::ok("NVIDIA Container Toolkit is installed and configured")
        };

        output = output
            .with_data("changes", serde_json::json!(changes))
            .with_data("status", serde_json::json!(status));

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["runtime"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("cdi", serde_json::json!(true));
        m.insert("cdi_output", serde_json::json!("/etc/cdi/nvidia.yaml"));
        m.insert("toolkit_config", serde_json::json!(null));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = NvidiaContainerToolkitModule;
        assert_eq!(module.name(), "nvidia_container_toolkit");
        assert!(!module.description().is_empty());
        assert!(module.required_params().contains(&"runtime"));
    }

    #[test]
    fn test_validate_runtime() {
        assert!(validate_runtime("docker").is_ok());
        assert!(validate_runtime("containerd").is_ok());
        assert!(validate_runtime("crio").is_ok());
        assert!(validate_runtime("podman").is_ok());
        assert!(validate_runtime("invalid").is_err());
        assert!(validate_runtime("").is_err());
    }

    #[test]
    fn test_runtime_service_name() {
        assert_eq!(runtime_service_name("docker"), "docker.service");
        assert_eq!(runtime_service_name("containerd"), "containerd.service");
        assert_eq!(runtime_service_name("crio"), "crio.service");
        assert_eq!(runtime_service_name("podman"), "podman.service");
    }

    #[test]
    fn test_parse_container_cli_info() {
        let output = r#"NVRM version:   535.183.01
CUDA version:   12.2
Device count:   4
Device 0:       NVIDIA A100-SXM4-80GB (UUID: GPU-abcd1234)
Device 1:       NVIDIA A100-SXM4-80GB (UUID: GPU-efgh5678)
Device 2:       NVIDIA A100-SXM4-80GB (UUID: GPU-ijkl9012)
Device 3:       NVIDIA A100-SXM4-80GB (UUID: GPU-mnop3456)
"#;
        let status = parse_container_cli_info(output);
        assert_eq!(status.toolkit_version, Some("535.183.01".to_string()));
        assert_eq!(status.gpu_count, Some(4));
    }

    #[test]
    fn test_parse_container_cli_info_empty() {
        let status = parse_container_cli_info("");
        assert_eq!(status.toolkit_version, None);
        assert_eq!(status.gpu_count, None);
    }

    #[test]
    fn test_parse_container_cli_info_partial() {
        let output = "NVRM version:   550.120\n";
        let status = parse_container_cli_info(output);
        assert_eq!(status.toolkit_version, Some("550.120".to_string()));
        assert_eq!(status.gpu_count, None);
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID=rocky\nVERSION=9"), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID=arch"), None);
    }

    #[test]
    fn test_container_toolkit_status_serialization() {
        let status = ContainerToolkitStatus {
            runtime: "docker".to_string(),
            cdi_enabled: true,
            toolkit_version: Some("535.183.01".to_string()),
            gpu_count: Some(2),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["runtime"], "docker");
        assert_eq!(json["cdi_enabled"], true);
        assert_eq!(json["toolkit_version"], "535.183.01");
        assert_eq!(json["gpu_count"], 2);
    }
}
