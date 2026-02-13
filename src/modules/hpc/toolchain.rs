//! HPC toolchain module
//!
//! Installs curated sets of HPC development and diagnostic tools.
//! Supports named toolchain sets that map to OS-specific packages.
//!
//! # Parameters
//!
//! - `sets` (required): List of toolchain set names to install
//!   - "build_essentials": gcc, g++, make, cmake, autoconf, automake, libtool
//!   - "perf_tools": perf, strace, ltrace, sysstat, htop
//!   - "debug_tools": gdb, valgrind, elfutils
//!   - "rdma_userland": rdma-core, libibverbs, librdmacm

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

/// Returns the list of packages for a given toolchain set and OS family.
fn toolchain_packages(set_name: &str, os_family: &str) -> Option<Vec<&'static str>> {
    match (set_name, os_family) {
        ("build_essentials", "rhel") => Some(vec![
            "gcc", "gcc-c++", "make", "cmake", "autoconf", "automake", "libtool",
        ]),
        ("build_essentials", "debian") => Some(vec![
            "build-essential",
            "cmake",
            "autoconf",
            "automake",
            "libtool",
        ]),
        ("perf_tools", "rhel") => Some(vec!["perf", "strace", "ltrace", "sysstat", "htop"]),
        ("perf_tools", "debian") => Some(vec![
            "linux-tools-generic",
            "strace",
            "ltrace",
            "sysstat",
            "htop",
        ]),
        ("debug_tools", "rhel") => Some(vec!["gdb", "valgrind", "elfutils"]),
        ("debug_tools", "debian") => Some(vec!["gdb", "valgrind", "elfutils"]),
        ("rdma_userland", "rhel") => Some(vec!["rdma-core", "libibverbs-utils", "librdmacm-utils"]),
        ("rdma_userland", "debian") => Some(vec!["rdma-core", "ibverbs-utils", "rdmacm-utils"]),
        _ => None,
    }
}

/// All recognized toolchain set names.
const VALID_SETS: &[&str] = &[
    "build_essentials",
    "perf_tools",
    "debug_tools",
    "rdma_userland",
];

pub struct HpcToolchainModule;

impl Module for HpcToolchainModule {
    fn name(&self) -> &'static str {
        "hpc_toolchain"
    }

    fn description(&self) -> &'static str {
        "Install curated HPC toolchain sets (build tools, perf, debug, RDMA)"
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

        let sets = params
            .get_vec_string("sets")?
            .ok_or_else(|| ModuleError::MissingParameter("sets".to_string()))?;

        if sets.is_empty() {
            return Err(ModuleError::InvalidParameter(
                "sets must contain at least one toolchain set name".to_string(),
            ));
        }

        // Validate all set names up front
        for set_name in &sets {
            if !VALID_SETS.contains(&set_name.as_str()) {
                return Err(ModuleError::InvalidParameter(format!(
                    "Unknown toolchain set '{}'. Valid sets: {}",
                    set_name,
                    VALID_SETS.join(", ")
                )));
            }
        }

        // Detect OS family
        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported(
                "Unsupported OS. HPC toolchain module supports RHEL-family and Debian-family distributions."
                    .to_string(),
            )
        })?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();
        let mut installed_sets: Vec<String> = Vec::new();

        for set_name in &sets {
            let packages = toolchain_packages(set_name, os_family).ok_or_else(|| {
                ModuleError::InvalidParameter(format!(
                    "No packages defined for set '{}' on OS family '{}'",
                    set_name, os_family
                ))
            })?;

            // Check if all packages are already installed
            let check_cmd = match os_family {
                "rhel" => format!("rpm -q {} >/dev/null 2>&1", packages.join(" ")),
                _ => format!("dpkg -s {} >/dev/null 2>&1", packages.join(" ")),
            };
            let (all_installed, _, _) = run_cmd(connection, &check_cmd, context)?;

            if all_installed {
                installed_sets.push(set_name.clone());
                continue;
            }

            if context.check_mode {
                changes.push(format!(
                    "Would install {} set: {}",
                    set_name,
                    packages.join(", ")
                ));
                installed_sets.push(set_name.clone());
                continue;
            }

            let install_cmd = match os_family {
                "rhel" => format!("dnf install -y {}", packages.join(" ")),
                _ => format!(
                    "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                    packages.join(" ")
                ),
            };
            run_cmd_ok(connection, &install_cmd, context)?;
            changed = true;
            installed_sets.push(set_name.clone());
            changes.push(format!(
                "Installed {} set: {}",
                set_name,
                packages.join(", ")
            ));
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would install {} toolchain sets",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("installed_sets", serde_json::json!(installed_sets))
            .with_data("os_family", serde_json::json!(os_family)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Installed {} toolchain sets", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("installed_sets", serde_json::json!(installed_sets))
                    .with_data("os_family", serde_json::json!(os_family)),
            )
        } else {
            Ok(
                ModuleOutput::ok("All requested toolchain sets are already installed")
                    .with_data("installed_sets", serde_json::json!(installed_sets))
                    .with_data("os_family", serde_json::json!(os_family)),
            )
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["sets"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        HashMap::new()
    }
}
