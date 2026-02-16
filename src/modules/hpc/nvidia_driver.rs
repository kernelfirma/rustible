//! NVIDIA GPU driver installation and management module
//!
//! Provides comprehensive NVIDIA driver installation with version pinning,
//! DKMS kernel module management, and nouveau driver blacklisting.
//!
//! # Features
//!
//! - Version-specific driver installation
//! - DKMS automatic kernel module rebuild support
//! - Nouveau driver blacklisting
//! - Repository management
//! - Idempotent state management
//!
//! # Parameters
//!
//! - `version` (optional): Specific driver version to install (e.g., "535", "550")
//! - `state` (optional): "present" (default) or "absent"
//! - `dkms` (optional): Enable DKMS support (default: true)
//! - `blacklist_nouveau` (optional): Blacklist nouveau driver (default: true)
//! - `repo_url` (optional): Custom repository URL for driver packages

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

/// Parse driver version from nvidia-smi output
fn parse_driver_version(nvidia_smi_output: &str) -> Option<String> {
    // nvidia-smi --query-gpu=driver_version --format=csv,noheader
    // Output: "535.183.01" or similar
    let version = nvidia_smi_output.trim();
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}

/// Check if driver version matches desired version (prefix matching)
fn version_matches(installed: &str, desired: &str) -> bool {
    // Support prefix matching: "535" matches "535.183.01"
    installed.starts_with(desired)
}

// ---- NVIDIA Driver Module ----

pub struct NvidiaDriverModule;

impl Module for NvidiaDriverModule {
    fn name(&self) -> &'static str {
        "nvidia_driver"
    }

    fn description(&self) -> &'static str {
        "Manage NVIDIA GPU driver installation with version pinning and DKMS support"
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
            ModuleError::Unsupported("Unsupported OS for NVIDIA driver module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let version = params.get_string("version")?;
        let dkms = params.get_bool("dkms")?.unwrap_or(true);
        let blacklist_nouveau = params.get_bool("blacklist_nouveau")?.unwrap_or(true);
        let repo_url = params.get_string("repo_url")?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Check current driver installation
        let (nvidia_installed, current_version_stdout, _) = run_cmd(
            connection,
            "nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null",
            context,
        )?;

        let current_version = if nvidia_installed {
            parse_driver_version(&current_version_stdout)
        } else {
            None
        };

        // -- state=absent --
        if state == "absent" {
            // Remove NVIDIA driver packages
            if nvidia_installed {
                if context.check_mode {
                    changes.push("Would remove NVIDIA driver packages".to_string());
                } else {
                    let remove_cmd = match os_family {
                        "rhel" => "dnf remove -y 'nvidia-*' 'cuda-*' 'dkms-nvidia*'".to_string(),
                        _ => "DEBIAN_FRONTEND=noninteractive apt-get remove --purge -y 'nvidia-*' 'cuda-*' 'libnvidia-*'"
                            .to_string(),
                    };
                    run_cmd_ok(connection, &remove_cmd, context)?;
                    changed = true;
                    changes.push("Removed NVIDIA driver packages".to_string());
                }
            }

            // Unblacklist nouveau if it was blacklisted
            let (blacklist_exists, _, _) = run_cmd(
                connection,
                "test -f /etc/modprobe.d/blacklist-nouveau.conf",
                context,
            )?;

            if blacklist_exists {
                if context.check_mode {
                    changes.push("Would remove nouveau blacklist".to_string());
                } else {
                    run_cmd_ok(
                        connection,
                        "rm -f /etc/modprobe.d/blacklist-nouveau.conf",
                        context,
                    )?;
                    // Rebuild initramfs
                    match os_family {
                        "rhel" => {
                            run_cmd_ok(connection, "dracut -f", context)?;
                        }
                        _ => {
                            run_cmd_ok(connection, "update-initramfs -u", context)?;
                        }
                    }
                    changed = true;
                    changes.push("Removed nouveau blacklist and rebuilt initramfs".to_string());
                }
            }

            if context.check_mode && !changes.is_empty() {
                return Ok(ModuleOutput::changed(format!(
                    "Would apply {} NVIDIA driver removal changes",
                    changes.len()
                ))
                .with_data("changes", serde_json::json!(changes)));
            }

            if changed {
                return Ok(ModuleOutput::changed("Removed NVIDIA driver")
                    .with_data("changes", serde_json::json!(changes)));
            }

            return Ok(ModuleOutput::ok("NVIDIA driver is not present"));
        }

        // -- state=present --

        // Step 1: Add repository if needed (custom or default)
        if let Some(ref custom_repo_url) = repo_url {
            if context.check_mode {
                changes.push(format!(
                    "Would add custom NVIDIA repository: {}",
                    custom_repo_url
                ));
            } else {
                match os_family {
                    "rhel" => {
                        // Add custom YUM/DNF repository
                        let repo_content = format!(
                            "[nvidia-driver]\nname=NVIDIA Driver\nbaseurl={}\nenabled=1\ngpgcheck=0\n",
                            custom_repo_url
                        );
                        run_cmd_ok(
                            connection,
                            &format!(
                                "echo '{}' > /etc/yum.repos.d/nvidia-driver.repo",
                                repo_content
                            ),
                            context,
                        )?;
                        changed = true;
                        changes.push(format!(
                            "Added custom NVIDIA repository: {}",
                            custom_repo_url
                        ));
                    }
                    _ => {
                        // Add custom APT repository
                        run_cmd_ok(
                            connection,
                            &format!(
                                "echo 'deb {} /' > /etc/apt/sources.list.d/nvidia-driver.list",
                                custom_repo_url
                            ),
                            context,
                        )?;
                        run_cmd_ok(connection, "apt-get update", context)?;
                        changed = true;
                        changes.push(format!(
                            "Added custom NVIDIA repository: {}",
                            custom_repo_url
                        ));
                    }
                }
            }
        } else {
            // Add official NVIDIA repository if not present
            let repo_check_cmd = match os_family {
                "rhel" => "dnf repolist | grep -q cuda",
                _ => "test -f /etc/apt/sources.list.d/cuda*.list",
            };

            let (repo_exists, _, _) = run_cmd(connection, repo_check_cmd, context)?;

            if !repo_exists && !context.check_mode {
                match os_family {
                    "rhel" => {
                        // Install CUDA repository package
                        let (_, os_version_stdout, _) =
                            run_cmd(connection, "rpm -E %{rhel}", context)?;
                        let rhel_version = os_version_stdout.trim();
                        let repo_url_rhel = format!(
                            "https://developer.download.nvidia.com/compute/cuda/repos/rhel{}/x86_64/cuda-rhel{}.repo",
                            rhel_version, rhel_version
                        );
                        run_cmd_ok(
                            connection,
                            &format!("dnf config-manager --add-repo {}", repo_url_rhel),
                            context,
                        )?;
                        changed = true;
                        changes.push("Added NVIDIA CUDA repository".to_string());
                    }
                    _ => {
                        // Install CUDA repository for Ubuntu/Debian
                        run_cmd_ok(
                            connection,
                            "wget -O /tmp/cuda-keyring.deb https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2204/x86_64/cuda-keyring_1.1-1_all.deb",
                            context,
                        )?;
                        run_cmd_ok(connection, "dpkg -i /tmp/cuda-keyring.deb", context)?;
                        run_cmd_ok(connection, "apt-get update", context)?;
                        changed = true;
                        changes.push("Added NVIDIA CUDA repository".to_string());
                    }
                }
            } else if !repo_exists && context.check_mode {
                changes.push("Would add NVIDIA CUDA repository".to_string());
            }
        }

        // Step 2: Check if driver needs installation or update
        let needs_install = if let Some(ref current) = current_version {
            if let Some(ref desired) = version {
                !version_matches(current, desired)
            } else {
                false // Driver is installed, no specific version required
            }
        } else {
            true // Driver not installed
        };

        if needs_install {
            if context.check_mode {
                if let Some(ref desired) = version {
                    changes.push(format!("Would install NVIDIA driver version {}", desired));
                } else {
                    changes.push("Would install latest NVIDIA driver".to_string());
                }
            } else {
                // Determine package name based on version and DKMS preference
                let package_name = if let Some(ref ver) = version {
                    match os_family {
                        "rhel" => {
                            if dkms {
                                format!("nvidia-driver-{}xx-dkms", ver)
                            } else {
                                format!("nvidia-driver-{}xx", ver)
                            }
                        }
                        _ => {
                            if dkms {
                                format!("nvidia-driver-{}-dkms", ver)
                            } else {
                                format!("nvidia-driver-{}", ver)
                            }
                        }
                    }
                } else {
                    match os_family {
                        "rhel" => {
                            if dkms {
                                "nvidia-driver-dkms".to_string()
                            } else {
                                "nvidia-driver".to_string()
                            }
                        }
                        _ => {
                            if dkms {
                                "nvidia-driver-dkms".to_string()
                            } else {
                                "nvidia-driver".to_string()
                            }
                        }
                    }
                };

                let install_cmd = match os_family {
                    "rhel" => format!("dnf install -y {}", package_name),
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                        package_name
                    ),
                };

                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push(format!("Installed NVIDIA driver package: {}", package_name));
            }
        }

        // Step 3: Blacklist nouveau if requested
        if blacklist_nouveau {
            let (blacklist_exists, _, _) = run_cmd(
                connection,
                "test -f /etc/modprobe.d/blacklist-nouveau.conf && grep -q 'blacklist nouveau' /etc/modprobe.d/blacklist-nouveau.conf",
                context,
            )?;

            if !blacklist_exists {
                if context.check_mode {
                    changes.push("Would blacklist nouveau driver".to_string());
                } else {
                    let blacklist_content = "blacklist nouveau\noptions nouveau modeset=0\n";
                    run_cmd_ok(
                        connection,
                        &format!(
                            "echo '{}' > /etc/modprobe.d/blacklist-nouveau.conf",
                            blacklist_content
                        ),
                        context,
                    )?;

                    // Rebuild initramfs
                    match os_family {
                        "rhel" => {
                            run_cmd_ok(connection, "dracut -f", context)?;
                        }
                        _ => {
                            run_cmd_ok(connection, "update-initramfs -u", context)?;
                        }
                    }

                    changed = true;
                    changes.push("Blacklisted nouveau driver and rebuilt initramfs".to_string());
                }
            }
        }

        // Step 4: Load NVIDIA kernel module if not loaded
        let (nvidia_loaded, _, _) = run_cmd(connection, "lsmod | grep -q '^nvidia '", context)?;

        if !nvidia_loaded && !context.check_mode {
            let (load_ok, _, _) = run_cmd(connection, "modprobe nvidia", context)?;
            if load_ok {
                changed = true;
                changes.push("Loaded nvidia kernel module".to_string());
            } else {
                // Module loading might fail if nouveau is still loaded; note it but don't fail
                changes.push(
                    "Note: nvidia module not loaded (may require reboot if nouveau was active)"
                        .to_string(),
                );
            }
        } else if !nvidia_loaded && context.check_mode {
            changes.push("Would load nvidia kernel module".to_string());
        }

        // Step 5: Verify installation
        let mut driver_info = serde_json::json!({});
        if !context.check_mode {
            let (verify_ok, verify_stdout, _) = run_cmd(
                connection,
                "nvidia-smi --query-gpu=name,driver_version,compute_cap --format=csv,noheader",
                context,
            )?;

            if verify_ok {
                let lines: Vec<&str> = verify_stdout.trim().lines().collect();
                if let Some(first_line) = lines.first() {
                    let parts: Vec<&str> = first_line.split(',').map(|s| s.trim()).collect();
                    if parts.len() >= 3 {
                        driver_info = serde_json::json!({
                            "gpu_name": parts[0],
                            "driver_version": parts[1],
                            "compute_capability": parts[2],
                            "gpu_count": lines.len(),
                        });
                    }
                }
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} NVIDIA driver changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} NVIDIA driver changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("driver_info", driver_info),
            )
        } else {
            Ok(ModuleOutput::ok("NVIDIA driver is configured")
                .with_data("driver_info", driver_info))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("version", serde_json::json!(null));
        m.insert("dkms", serde_json::json!(true));
        m.insert("blacklist_nouveau", serde_json::json!(true));
        m.insert("repo_url", serde_json::json!(null));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = NvidiaDriverModule;
        assert_eq!(module.name(), "nvidia_driver");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params().len(), 0);
    }

    #[test]
    fn test_parse_driver_version() {
        assert_eq!(
            parse_driver_version("535.183.01"),
            Some("535.183.01".to_string())
        );
        assert_eq!(parse_driver_version("550.120"), Some("550.120".to_string()));
        assert_eq!(parse_driver_version(""), None);
        assert_eq!(
            parse_driver_version("  535.183.01  \n"),
            Some("535.183.01".to_string())
        );
    }

    #[test]
    fn test_version_matches() {
        assert!(version_matches("535.183.01", "535"));
        assert!(version_matches("535.183.01", "535.183"));
        assert!(version_matches("535.183.01", "535.183.01"));
        assert!(!version_matches("535.183.01", "550"));
        assert!(!version_matches("535.183.01", "536"));
    }

    #[test]
    fn test_detect_os_family_rhel() {
        let os_release = r#"
NAME="Rocky Linux"
VERSION="8.9 (Green Obsidian)"
ID="rocky"
ID_LIKE="rhel centos fedora"
"#;
        assert_eq!(detect_os_family(os_release), Some("rhel"));
    }

    #[test]
    fn test_detect_os_family_debian() {
        let os_release = r#"
NAME="Ubuntu"
VERSION="22.04.3 LTS (Jammy Jellyfish)"
ID=ubuntu
ID_LIKE=debian
"#;
        assert_eq!(detect_os_family(os_release), Some("debian"));
    }

    #[test]
    fn test_detect_os_family_unsupported() {
        let os_release = r#"
NAME="FreeBSD"
ID=freebsd
"#;
        assert_eq!(detect_os_family(os_release), None);
    }
}
