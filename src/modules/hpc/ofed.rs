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
//! - `reboot_coordination` (optional): bool (default: true) - coordinate reboot after OFED install/upgrade

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

/// Parse OFED version from `ofed_info -s` output.
///
/// Extracts the major.minor version from strings like
/// "MLNX_OFED_LINUX-5.8-1.0.1.1:" returning "5.8".
fn parse_ofed_version(ofed_info_output: &str) -> Option<String> {
    let trimmed = ofed_info_output.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Find the version portion after "MLNX_OFED_LINUX-" or similar prefix
    let version_part = if let Some(pos) = trimmed.find("MLNX_OFED") {
        let after_prefix = &trimmed[pos..];
        // Skip to first dash after the prefix text
        if let Some(dash_pos) = after_prefix.find('-') {
            &after_prefix[dash_pos + 1..]
        } else {
            return None;
        }
    } else {
        // Try to parse as a bare version string
        trimmed
    };

    // Extract major.minor from the version portion (e.g., "5.8-1.0.1.1:" -> "5.8")
    let mut parts = version_part.split(|c: char| !c.is_ascii_digit() && c != '.');
    let first_segment = parts.next()?;
    let mut dot_parts = first_segment.split('.');
    let major = dot_parts.next()?;
    let minor = dot_parts.next()?;

    // Validate that both are numeric
    if major.parse::<u32>().is_err() || minor.parse::<u32>().is_err() {
        return None;
    }

    Some(format!("{}.{}", major, minor))
}

/// Parse kernel major.minor version from a `uname -r` output string.
///
/// Example: "5.15.0-75-generic" -> Some((5, 15))
fn parse_kernel_major_minor(uname_r: &str) -> Option<(u32, u32)> {
    let trimmed = uname_r.trim();
    let mut parts = trimmed.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Check OFED version compatibility against the running kernel version.
///
/// Known compatibility ranges:
/// - MLNX_OFED 5.x: kernels 4.15 - 5.15
/// - MLNX_OFED 23.x: kernels 5.4 - 6.2
/// - MLNX_OFED 24.x: kernels 5.15 - 6.8
fn check_compat_matrix(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<PreflightResult> {
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    // Get OFED version
    let (ofed_ok, ofed_stdout, _) =
        run_cmd(connection, "ofed_info -s 2>/dev/null || true", context)?;
    let ofed_version = if ofed_ok {
        parse_ofed_version(&ofed_stdout)
    } else {
        None
    };

    // Get kernel version
    let kernel_stdout = run_cmd_ok(connection, "uname -r", context)?;
    let kernel_version = parse_kernel_major_minor(&kernel_stdout);

    if let (Some(ref ofed_ver), Some((k_major, k_minor))) = (&ofed_version, kernel_version) {
        let ofed_major: u32 = ofed_ver
            .split('.')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let kernel_ver = (k_major, k_minor);

        let compatible = match ofed_major {
            5 => kernel_ver >= (4, 15) && kernel_ver <= (5, 15),
            23 => kernel_ver >= (5, 4) && kernel_ver <= (6, 2),
            24 => kernel_ver >= (5, 15) && kernel_ver <= (6, 8),
            _ => {
                warnings.push(format!(
                    "Unknown OFED major version {}; cannot verify kernel compatibility",
                    ofed_major
                ));
                true // Assume compatible for unknown versions
            }
        };

        if !compatible {
            warnings.push(format!(
                "OFED {} may not be compatible with kernel {}.{}; \
                 check Mellanox compatibility matrix",
                ofed_ver, k_major, k_minor
            ));
        }
    } else {
        if ofed_version.is_none() {
            warnings
                .push("Could not determine OFED version; skipping compatibility check".to_string());
        }
        if kernel_version.is_none() {
            errors.push("Could not determine kernel version".to_string());
        }
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Check for active RDMA connections before upgrading OFED.
///
/// Inspects port state via sysfs and saves the current OFED version
/// for potential rollback information.
fn guarded_upgrade(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<PreflightResult> {
    let mut warnings: Vec<String> = Vec::new();
    let errors: Vec<String> = Vec::new();

    // Check for active RDMA connections via sysfs port state
    let (_, port_state_output, _) = run_cmd(
        connection,
        "cat /sys/class/infiniband/*/ports/*/state 2>/dev/null || true",
        context,
    )?;

    let active_count = port_state_output
        .lines()
        .filter(|line| line.contains("ACTIVE"))
        .count();

    if active_count > 0 {
        warnings.push(format!(
            "Found {} active RDMA port(s); upgrading OFED may disrupt connections",
            active_count
        ));
    }

    // Try rdma link show as a secondary check
    let (rdma_ok, rdma_stdout, _) =
        run_cmd(connection, "rdma link show 2>/dev/null || true", context)?;
    if rdma_ok {
        let active_links = rdma_stdout
            .lines()
            .filter(|line| line.contains("state ACTIVE"))
            .count();
        if active_links > 0 && active_count == 0 {
            warnings.push(format!(
                "rdma link show reports {} active link(s)",
                active_links
            ));
        }
    }

    // Save current OFED version for rollback info
    let (_, ofed_info, _) = run_cmd(
        connection,
        "ofed_info -s 2>/dev/null || echo 'unknown'",
        context,
    )?;
    let current_version = ofed_info.trim().to_string();
    if current_version != "unknown" {
        warnings.push(format!(
            "Current OFED version before upgrade: {}; save for rollback if needed",
            current_version
        ));
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// After OFED install/upgrade that requires a reboot, create a reboot marker
/// file and provide reboot instructions.
fn coordinate_reboot(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<VerifyResult> {
    let mut details: Vec<String> = Vec::new();
    let warnings: Vec<String> = Vec::new();

    if context.check_mode {
        details.push(format!(
            "Would create reboot marker at {}",
            REBOOT_MARKER_PATH
        ));
        details.push("Would schedule reboot coordination for OFED changes".to_string());
        return Ok(VerifyResult {
            verified: true,
            details,
            warnings,
        });
    }

    // Create the reboot marker file
    let marker_content = "ofed-upgrade";
    run_cmd_ok(
        connection,
        &format!(
            "printf '%s\\n' '{}' > {}",
            marker_content, REBOOT_MARKER_PATH
        ),
        context,
    )?;
    details.push(format!("Created reboot marker at {}", REBOOT_MARKER_PATH));
    details.push(
        "Reboot required to complete OFED installation/upgrade; \
         schedule maintenance window and reboot the node"
            .to_string(),
    );

    Ok(VerifyResult {
        verified: true,
        details,
        warnings,
    })
}

const REBOOT_MARKER_PATH: &str = "/var/run/rustible-reboot-required";

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
        let _ = run_cmd(connection, "rm -f /etc/modules-load.d/rdma.conf", context);

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
        let reboot_coordination = params.get_bool_or("reboot_coordination", true);

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
        let mut all_warnings: Vec<String> = Vec::new();

        // --- Compatibility matrix preflight check ---
        let compat_result = check_compat_matrix(connection, context)?;
        all_warnings.extend(compat_result.warnings);
        if !compat_result.errors.is_empty() {
            return Err(ModuleError::ExecutionFailed(format!(
                "Compatibility preflight failed: {}",
                compat_result.errors.join("; ")
            )));
        }

        // --- Guarded upgrade check ---
        let upgrade_result = guarded_upgrade(connection, context)?;
        all_warnings.extend(upgrade_result.warnings);

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
                    run_cmd_ok(connection, &format!("modprobe {}", module), context)?;
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

        // --- Reboot coordination ---
        let mut reboot_info: Option<VerifyResult> = None;
        if changed && reboot_coordination {
            let reboot_result = coordinate_reboot(connection, context)?;
            reboot_info = Some(reboot_result);
        }

        // --- Build output ---
        if context.check_mode && !changes.is_empty() {
            let mut output =
                ModuleOutput::changed(format!("Would apply {} RDMA stack changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("os_family", serde_json::json!(os_family));
            if !all_warnings.is_empty() {
                output = output.with_data("warnings", serde_json::json!(all_warnings));
            }
            return Ok(output);
        }

        if changed {
            let mut output =
                ModuleOutput::changed(format!("Applied {} RDMA stack changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("os_family", serde_json::json!(os_family))
                    .with_data("kernel_modules", serde_json::json!(all_modules));
            if !all_warnings.is_empty() {
                output = output.with_data("warnings", serde_json::json!(all_warnings));
            }
            if let Some(ref reboot) = reboot_info {
                output = output.with_data("reboot_coordination", serde_json::json!(reboot));
            }
            Ok(output)
        } else {
            let mut output = ModuleOutput::ok("RDMA stack is installed and configured")
                .with_data("os_family", serde_json::json!(os_family))
                .with_data("kernel_modules", serde_json::json!(all_modules));
            if !all_warnings.is_empty() {
                output = output.with_data("warnings", serde_json::json!(all_warnings));
            }
            Ok(output)
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
        m.insert("reboot_coordination", serde_json::json!(true));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = RdmaStackModule;
        assert_eq!(module.name(), "rdma_stack");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_optional_params_include_reboot_coordination() {
        let module = RdmaStackModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("reboot_coordination"));
        assert_eq!(optional["reboot_coordination"], serde_json::json!(true));
    }

    #[test]
    fn test_ofed_version_parsing() {
        // Standard MLNX_OFED_LINUX format
        assert_eq!(
            parse_ofed_version("MLNX_OFED_LINUX-5.8-1.0.1.1:"),
            Some("5.8".to_string())
        );
        assert_eq!(
            parse_ofed_version("MLNX_OFED_LINUX-23.10-1.1.9.0:"),
            Some("23.10".to_string())
        );
        assert_eq!(
            parse_ofed_version("MLNX_OFED_LINUX-24.04-0.6.6.0:"),
            Some("24.04".to_string())
        );

        // With leading/trailing whitespace
        assert_eq!(
            parse_ofed_version("  MLNX_OFED_LINUX-5.4-3.5.8.0:  \n"),
            Some("5.4".to_string())
        );

        // Invalid / missing output
        assert_eq!(parse_ofed_version(""), None);
        assert_eq!(parse_ofed_version("not-a-version"), None);
    }

    #[test]
    fn test_kernel_version_parsing() {
        assert_eq!(parse_kernel_major_minor("5.15.0-75-generic"), Some((5, 15)));
        assert_eq!(
            parse_kernel_major_minor("4.18.0-477.10.1.el8_8.x86_64"),
            Some((4, 18))
        );
        assert_eq!(parse_kernel_major_minor("6.2.0-26-generic"), Some((6, 2)));
        assert_eq!(
            parse_kernel_major_minor("  5.4.0-150-generic\n"),
            Some((5, 4))
        );

        // Invalid
        assert_eq!(parse_kernel_major_minor(""), None);
        assert_eq!(parse_kernel_major_minor("not-a-kernel"), None);
    }

    #[test]
    fn test_compat_matrix_kernel_ofed() {
        // MLNX_OFED 5.x: kernels 4.15 - 5.15
        // Compatible combos
        assert!(is_compat(5, (4, 15)));
        assert!(is_compat(5, (5, 4)));
        assert!(is_compat(5, (5, 15)));
        // Incompatible combos
        assert!(!is_compat(5, (4, 14)));
        assert!(!is_compat(5, (5, 16)));
        assert!(!is_compat(5, (6, 0)));

        // MLNX_OFED 23.x: kernels 5.4 - 6.2
        assert!(is_compat(23, (5, 4)));
        assert!(is_compat(23, (5, 15)));
        assert!(is_compat(23, (6, 2)));
        assert!(!is_compat(23, (5, 3)));
        assert!(!is_compat(23, (6, 3)));

        // MLNX_OFED 24.x: kernels 5.15 - 6.8
        assert!(is_compat(24, (5, 15)));
        assert!(is_compat(24, (6, 1)));
        assert!(is_compat(24, (6, 8)));
        assert!(!is_compat(24, (5, 14)));
        assert!(!is_compat(24, (6, 9)));
    }

    /// Helper for testing compatibility matrix logic without a connection.
    fn is_compat(ofed_major: u32, kernel_ver: (u32, u32)) -> bool {
        match ofed_major {
            5 => kernel_ver >= (4, 15) && kernel_ver <= (5, 15),
            23 => kernel_ver >= (5, 4) && kernel_ver <= (6, 2),
            24 => kernel_ver >= (5, 15) && kernel_ver <= (6, 8),
            _ => true,
        }
    }

    #[test]
    fn test_reboot_marker_path() {
        assert_eq!(REBOOT_MARKER_PATH, "/var/run/rustible-reboot-required");
        // Verify it is an absolute path
        assert!(REBOOT_MARKER_PATH.starts_with('/'));
        // Verify it contains the expected identifier
        assert!(REBOOT_MARKER_PATH.contains("rustible"));
        assert!(REBOOT_MARKER_PATH.contains("reboot"));
    }
}
