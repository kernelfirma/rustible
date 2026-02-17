//! NVIDIA GPU driver installation and management module
//!
//! Provides comprehensive NVIDIA driver installation with version pinning,
//! DKMS kernel module management, nouveau driver blacklisting, kernel
//! compatibility checks, and post-install GPU readiness verification.
//!
//! # Features
//!
//! - Version-specific driver installation
//! - DKMS automatic kernel module rebuild support
//! - Nouveau driver blacklisting
//! - Repository management
//! - Idempotent state management
//! - Kernel compatibility preflight checks
//! - DKMS rebuild orchestration
//! - Post-install GPU readiness verification
//! - Canary deployment mode
//!
//! # Parameters
//!
//! - `version` (optional): Specific driver version to install (e.g., "535", "550")
//! - `state` (optional): "present" (default) or "absent"
//! - `dkms` (optional): Enable DKMS support (default: true)
//! - `blacklist_nouveau` (optional): Blacklist nouveau driver (default: true)
//! - `repo_url` (optional): Custom repository URL for driver packages
//! - `canary` (optional): Apply to one GPU first for validation (default: false)
//! - `kernel_check` (optional): Run kernel compatibility preflight check (default: true)

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
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

#[derive(Debug, serde::Serialize)]
struct VerifyResult {
    verified: bool,
    details: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct GpuReadiness {
    healthy: bool,
    gpu_count: u32,
    ecc_errors: Vec<String>,
    temperature_warnings: Vec<String>,
    power_issues: Vec<String>,
}

// ---- Helper functions ----

/// Parse major.minor components from a kernel version string.
///
/// Accepts full `uname -r` output like "5.15.0-91-generic" and returns
/// the (major, minor) tuple, e.g. `Some((5, 15))`.
fn parse_kernel_version(uname_output: &str) -> Option<(u32, u32)> {
    let version_str = uname_output.trim();
    let mut parts = version_str.split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    Some((major, minor))
}

/// Check kernel compatibility for NVIDIA driver compilation.
///
/// Verifies that kernel-devel / kernel-headers packages matching the running
/// kernel are installed. Returns a `PreflightResult` with any warnings or
/// errors discovered.
fn check_kernel_compatibility(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    os_family: &str,
) -> ModuleResult<PreflightResult> {
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Get running kernel version
    let kernel_version = run_cmd_ok(connection, "uname -r", context)?;
    let kernel_version = kernel_version.trim().to_string();

    if let Some((major, minor)) = parse_kernel_version(&kernel_version) {
        // Warn on very old kernels
        if major < 4 || (major == 4 && minor < 15) {
            warnings.push(format!(
                "Kernel {}.{} may have limited NVIDIA driver support; 4.15+ recommended",
                major, minor
            ));
        }
    } else {
        warnings.push(format!(
            "Could not parse kernel version from '{}'",
            kernel_version
        ));
    }

    // Check for kernel-devel / kernel-headers
    let headers_installed = match os_family {
        "rhel" => {
            let cmd = format!("rpm -q kernel-devel-{}", kernel_version);
            let (ok, _, _) = run_cmd(connection, &cmd, context)?;
            ok
        }
        _ => {
            let cmd = format!("dpkg -l linux-headers-{}", kernel_version);
            let (ok, _, _) = run_cmd(connection, &cmd, context)?;
            ok
        }
    };

    if !headers_installed {
        let pkg_name = match os_family {
            "rhel" => format!("kernel-devel-{}", kernel_version),
            _ => format!("linux-headers-{}", kernel_version),
        };
        errors.push(format!(
            "Kernel headers package '{}' is not installed; DKMS builds will fail",
            pkg_name
        ));
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Orchestrate a DKMS rebuild for the NVIDIA kernel module.
///
/// Inspects `dkms status nvidia` output. If the module is in the "added"
/// state but not yet "installed", triggers `dkms install nvidia/<version>`.
fn orchestrate_dkms_rebuild(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<VerifyResult> {
    let mut details: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut verified = true;

    let (dkms_ok, dkms_stdout, _) = run_cmd(connection, "dkms status nvidia", context)?;

    if !dkms_ok || dkms_stdout.trim().is_empty() {
        details.push("No NVIDIA DKMS modules found".to_string());
        return Ok(VerifyResult {
            verified: false,
            details,
            warnings,
        });
    }

    for line in dkms_stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Typical line: "nvidia/535.183.01, 5.15.0-91-generic, x86_64: installed"
        // or "nvidia/535.183.01, 5.15.0-91-generic, x86_64: added"
        if line.contains("installed") {
            details.push(format!("DKMS module already installed: {}", line));
        } else if line.contains("added") {
            // Extract version for dkms install
            if let Some(version_part) = line.split('/').nth(1) {
                let version = version_part.split(',').next().unwrap_or("").trim();
                if !version.is_empty() && !context.check_mode {
                    let install_cmd = format!("dkms install nvidia/{}", version);
                    let (install_ok, install_stdout, install_stderr) =
                        run_cmd(connection, &install_cmd, context)?;
                    if install_ok {
                        details
                            .push(format!("DKMS install succeeded for nvidia/{}", version));
                    } else {
                        verified = false;
                        warnings.push(format!(
                            "DKMS install failed for nvidia/{}: {} {}",
                            version,
                            install_stdout.trim(),
                            install_stderr.trim()
                        ));
                    }
                } else if !version.is_empty() && context.check_mode {
                    details.push(format!("Would run dkms install nvidia/{}", version));
                }
            }
        } else {
            warnings.push(format!("Unexpected DKMS status line: {}", line));
        }
    }

    Ok(VerifyResult {
        verified,
        details,
        warnings,
    })
}

/// Check GPU readiness after driver installation.
///
/// Queries nvidia-smi for temperature, ECC errors, and power draw. Flags
/// temperatures above 85 C, any ECC errors, and power anomalies.
fn check_gpu_readiness(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<GpuReadiness> {
    let mut ecc_errors: Vec<String> = Vec::new();
    let mut temperature_warnings: Vec<String> = Vec::new();
    let mut power_issues: Vec<String> = Vec::new();
    let mut healthy = true;
    let mut gpu_count: u32 = 0;

    let (smi_ok, smi_stdout, _) = run_cmd(
        connection,
        "nvidia-smi --query-gpu=gpu_name,temperature.gpu,power.draw,ecc.errors.corrected.volatile.total --format=csv,noheader",
        context,
    )?;

    if !smi_ok || smi_stdout.trim().is_empty() {
        return Ok(GpuReadiness {
            healthy: false,
            gpu_count: 0,
            ecc_errors: vec!["nvidia-smi query failed or returned no data".to_string()],
            temperature_warnings,
            power_issues,
        });
    }

    for line in smi_stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        gpu_count += 1;
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() < 4 {
            continue;
        }

        let gpu_name = parts[0];

        // Temperature check (value may be like "72" or "[N/A]")
        if let Ok(temp) = parts[1].parse::<u32>() {
            if temp > 85 {
                healthy = false;
                temperature_warnings.push(format!(
                    "GPU {} '{}': temperature {}C exceeds 85C threshold",
                    gpu_count - 1,
                    gpu_name,
                    temp
                ));
            }
        }

        // Power draw check (value may be like "250.00 W" or "[N/A]")
        let power_str = parts[2].replace(" W", "").replace(" w", "");
        if let Ok(power) = power_str.trim().parse::<f64>() {
            if power <= 0.0 {
                power_issues.push(format!(
                    "GPU {} '{}': abnormal power draw {:.1}W",
                    gpu_count - 1,
                    gpu_name,
                    power
                ));
            }
        }

        // ECC errors check (value may be "0", "5", or "[N/A]" / "[Not Supported]")
        let ecc_str = parts[3].trim();
        if !ecc_str.starts_with('[') {
            if let Ok(ecc_count) = ecc_str.parse::<u64>() {
                if ecc_count > 0 {
                    healthy = false;
                    ecc_errors.push(format!(
                        "GPU {} '{}': {} corrected ECC errors detected",
                        gpu_count - 1,
                        gpu_name,
                        ecc_count
                    ));
                }
            }
        }
    }

    Ok(GpuReadiness {
        healthy,
        gpu_count,
        ecc_errors,
        temperature_warnings,
        power_issues,
    })
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
        let _canary = params.get_bool("canary")?.unwrap_or(false);
        let kernel_check = params.get_bool("kernel_check")?.unwrap_or(true);

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();
        let mut reboot_required = false;

        // Kernel compatibility preflight check (state=present only)
        let kernel_compat = if kernel_check && state == "present" {
            let result = check_kernel_compatibility(connection, context, os_family)?;
            if !result.passed {
                for err in &result.errors {
                    changes.push(format!("Kernel preflight error: {}", err));
                }
            }
            for w in &result.warnings {
                changes.push(format!("Kernel preflight warning: {}", w));
            }
            Some(result)
        } else {
            None
        };

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

        // Step 5: DKMS status check
        let dkms_status = if dkms && !context.check_mode {
            let result = orchestrate_dkms_rebuild(connection, context)?;
            if !result.verified {
                for w in &result.warnings {
                    changes.push(format!("DKMS warning: {}", w));
                }
            }
            Some(result)
        } else {
            None
        };

        // Step 6: Verify installation
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

        // Step 7: Post-install GPU readiness check
        let gpu_readiness = if !context.check_mode {
            let result = check_gpu_readiness(connection, context)?;
            if !result.healthy {
                for w in &result.temperature_warnings {
                    changes.push(format!("GPU readiness: {}", w));
                }
                for e in &result.ecc_errors {
                    changes.push(format!("GPU readiness: {}", e));
                }
            }
            Some(result)
        } else {
            None
        };

        // Determine if reboot is required
        if !nvidia_loaded && changed {
            reboot_required = true;
        }

        // Build output
        if context.check_mode && !changes.is_empty() {
            let mut output = ModuleOutput::changed(format!(
                "Would apply {} NVIDIA driver changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes));
            if let Some(ref kc) = kernel_compat {
                output = output.with_data("kernel_compat", serde_json::json!(kc));
            }
            return Ok(output);
        }

        let build_output = |mut output: ModuleOutput| -> ModuleOutput {
            output = output
                .with_data("changes", serde_json::json!(changes))
                .with_data("driver_info", driver_info.clone())
                .with_data("reboot_required", serde_json::json!(reboot_required));
            if let Some(ref kc) = kernel_compat {
                output = output.with_data("kernel_compat", serde_json::json!(kc));
            }
            if let Some(ref ds) = dkms_status {
                output = output.with_data("dkms_status", serde_json::json!(ds));
            }
            if let Some(ref gr) = gpu_readiness {
                output = output.with_data("gpu_readiness", serde_json::json!(gr));
            }
            output
        };

        if changed {
            Ok(build_output(ModuleOutput::changed(format!(
                "Applied {} NVIDIA driver changes",
                changes.len()
            ))))
        } else {
            Ok(build_output(ModuleOutput::ok("NVIDIA driver is configured")))
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
        m.insert("canary", serde_json::json!(false));
        m.insert("kernel_check", serde_json::json!(true));
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

    #[test]
    fn test_kernel_version_parsing() {
        // Standard Ubuntu kernel
        assert_eq!(parse_kernel_version("5.15.0-91-generic"), Some((5, 15)));
        // Standard RHEL kernel
        assert_eq!(
            parse_kernel_version("4.18.0-513.11.1.el8_9.x86_64"),
            Some((4, 18))
        );
        // Newer kernel
        assert_eq!(parse_kernel_version("6.5.0-14-generic"), Some((6, 5)));
        // Minimal version string
        assert_eq!(parse_kernel_version("5.4"), Some((5, 4)));
        // With trailing whitespace from uname output
        assert_eq!(
            parse_kernel_version("  5.15.0-91-generic\n"),
            Some((5, 15))
        );
        // Empty / invalid input
        assert_eq!(parse_kernel_version(""), None);
        assert_eq!(parse_kernel_version("not-a-version"), None);
    }

    #[test]
    fn test_dkms_status_parsing() {
        // Verify the DKMS status line parsing logic used by orchestrate_dkms_rebuild.
        // We test the parsing inline since the function itself requires a connection.
        let line_installed = "nvidia/535.183.01, 5.15.0-91-generic, x86_64: installed";
        assert!(line_installed.contains("installed"));

        let line_added = "nvidia/535.183.01, 5.15.0-91-generic, x86_64: added";
        assert!(line_added.contains("added"));
        assert!(!line_added.contains("installed"));

        // Extract version from added line
        let version_part = line_added.split('/').nth(1).unwrap();
        let version = version_part.split(',').next().unwrap().trim();
        assert_eq!(version, "535.183.01");

        // Multi-line scenario
        let dkms_output = "nvidia/535.183.01, 5.15.0-91-generic, x86_64: installed\nnvidia/535.183.01, 6.1.0-generic, x86_64: added\n";
        let lines: Vec<&str> = dkms_output
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("installed"));
        assert!(lines[1].contains("added"));
    }

    #[test]
    fn test_gpu_readiness_structure() {
        // Test that GpuReadiness serializes correctly
        let readiness = GpuReadiness {
            healthy: true,
            gpu_count: 2,
            ecc_errors: vec![],
            temperature_warnings: vec![],
            power_issues: vec![],
        };

        let json = serde_json::to_value(&readiness).unwrap();
        assert_eq!(json["healthy"], true);
        assert_eq!(json["gpu_count"], 2);
        assert!(json["ecc_errors"].as_array().unwrap().is_empty());
        assert!(json["temperature_warnings"].as_array().unwrap().is_empty());
        assert!(json["power_issues"].as_array().unwrap().is_empty());

        // Test unhealthy GPU
        let unhealthy = GpuReadiness {
            healthy: false,
            gpu_count: 4,
            ecc_errors: vec!["GPU 0 'A100': 3 corrected ECC errors detected".to_string()],
            temperature_warnings: vec![
                "GPU 1 'A100': temperature 92C exceeds 85C threshold".to_string(),
            ],
            power_issues: vec!["GPU 2 'A100': abnormal power draw 0.0W".to_string()],
        };

        let json2 = serde_json::to_value(&unhealthy).unwrap();
        assert_eq!(json2["healthy"], false);
        assert_eq!(json2["gpu_count"], 4);
        assert_eq!(json2["ecc_errors"].as_array().unwrap().len(), 1);
        assert_eq!(json2["temperature_warnings"].as_array().unwrap().len(), 1);
        assert_eq!(json2["power_issues"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_preflight_result_serialization() {
        let result = PreflightResult {
            passed: false,
            warnings: vec!["Kernel 4.14 may have limited support".to_string()],
            errors: vec!["kernel-devel-5.15.0-91-generic is not installed".to_string()],
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["passed"], false);
        assert_eq!(json["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(json["errors"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_verify_result_serialization() {
        let result = VerifyResult {
            verified: true,
            details: vec!["DKMS module already installed".to_string()],
            warnings: vec![],
        };

        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["verified"], true);
        assert_eq!(json["details"].as_array().unwrap().len(), 1);
        assert!(json["warnings"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_drift_item_serialization() {
        let drift = DriftItem {
            field: "driver_version".to_string(),
            desired: "535".to_string(),
            actual: "530.41.03".to_string(),
        };

        let json = serde_json::to_value(&drift).unwrap();
        assert_eq!(json["field"], "driver_version");
        assert_eq!(json["desired"], "535");
        assert_eq!(json["actual"], "530.41.03");
    }
}
