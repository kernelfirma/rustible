//! MPI configuration module
//!
//! Manages MPI library installation and configuration (OpenMPI, Intel MPI)
//! for HPC clusters. Supports MCA parameter configuration and optional
//! Lmod modulefile generation.
//!
//! # Parameters
//!
//! - `flavor` (optional): "openmpi" (default) or "intelmpi"
//! - `mca_params` (optional): Map of MCA parameter key/value pairs
//! - `lmod_module` (optional): Whether to create an Lmod modulefile (default: false)

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

pub struct MpiModule;

impl Module for MpiModule {
    fn name(&self) -> &'static str {
        "mpi_config"
    }

    fn description(&self) -> &'static str {
        "Configure MPI libraries (OpenMPI, Intel MPI)"
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

        let flavor = params
            .get_string("flavor")?
            .unwrap_or_else(|| "openmpi".to_string());
        let lmod_module = params.get_bool_or("lmod_module", false);

        // Validate flavor
        if flavor != "openmpi" && flavor != "intelmpi" {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid MPI flavor '{}'. Must be 'openmpi' or 'intelmpi'",
                flavor
            )));
        }

        // Detect OS family
        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported(
                "Unsupported OS. MPI module supports RHEL-family and Debian-family distributions."
                    .to_string(),
            )
        })?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Install MPI packages
        let (check_cmd, install_cmd) = match (&*flavor, os_family) {
            ("openmpi", "rhel") => (
                "rpm -q openmpi openmpi-devel >/dev/null 2>&1",
                "dnf install -y openmpi openmpi-devel",
            ),
            ("openmpi", _) => (
                "dpkg -s openmpi-bin libopenmpi-dev >/dev/null 2>&1",
                "DEBIAN_FRONTEND=noninteractive apt-get install -y openmpi-bin libopenmpi-dev",
            ),
            ("intelmpi", "rhel") => (
                "rpm -q intel-oneapi-mpi intel-oneapi-mpi-devel >/dev/null 2>&1",
                "dnf install -y intel-oneapi-mpi intel-oneapi-mpi-devel",
            ),
            ("intelmpi", _) => (
                "dpkg -s intel-oneapi-mpi intel-oneapi-mpi-devel >/dev/null 2>&1",
                "DEBIAN_FRONTEND=noninteractive apt-get install -y intel-oneapi-mpi intel-oneapi-mpi-devel",
            ),
            _ => unreachable!(),
        };

        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push(format!("Would install {} packages", flavor));
            } else {
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push(format!("Installed {} packages", flavor));
            }
        }

        // Write MCA config (OpenMPI only)
        if flavor == "openmpi" {
            if let Some(mca_val) = params.get("mca_params") {
                if let Some(mca_map) = mca_val.as_object() {
                    if !mca_map.is_empty() {
                        let mut mca_lines: Vec<String> = Vec::with_capacity(mca_map.len());
                        for (key, value) in mca_map {
                            let val_str =
                                value.as_str().unwrap_or(&value.to_string()).to_string();
                            mca_lines.push(format!("{} = {}", key, val_str));
                        }
                        mca_lines.sort();
                        let desired_content = mca_lines.join("\n");

                        let (_, existing, _) = run_cmd(
                            connection,
                            "cat /etc/openmpi/openmpi-mca-params.conf 2>/dev/null || true",
                            context,
                        )?;

                        if existing.trim() != desired_content.trim() {
                            if context.check_mode {
                                changes.push(format!(
                                    "Would write {} MCA parameters to /etc/openmpi/openmpi-mca-params.conf",
                                    mca_map.len()
                                ));
                            } else {
                                run_cmd_ok(
                                    connection,
                                    "mkdir -p /etc/openmpi",
                                    context,
                                )?;
                                run_cmd_ok(
                                    connection,
                                    &format!(
                                        "printf '%s\\n' '{}' > /etc/openmpi/openmpi-mca-params.conf",
                                        desired_content.replace('\'', "'\\''")
                                    ),
                                    context,
                                )?;
                                changed = true;
                                changes.push(format!(
                                    "Wrote {} MCA parameters to /etc/openmpi/openmpi-mca-params.conf",
                                    mca_map.len()
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Create Lmod modulefile
        if lmod_module {
            let module_dir = format!("/opt/modulefiles/mpi/{}", flavor);
            let module_file = format!("{}/default.lua", module_dir);

            let modulefile_content = match &*flavor {
                "openmpi" => {
                    let mpi_prefix = match os_family {
                        "rhel" => "/usr/lib64/openmpi",
                        _ => "/usr/lib/x86_64-linux-gnu/openmpi",
                    };
                    format!(
                        "-- OpenMPI modulefile - managed by Rustible\n\
                         help([[OpenMPI library for parallel computing]])\n\
                         \n\
                         local base = \"{prefix}\"\n\
                         \n\
                         prepend_path(\"PATH\", pathJoin(base, \"bin\"))\n\
                         prepend_path(\"LD_LIBRARY_PATH\", pathJoin(base, \"lib\"))\n\
                         prepend_path(\"MANPATH\", pathJoin(base, \"share/man\"))\n\
                         setenv(\"MPI_HOME\", base)\n",
                        prefix = mpi_prefix
                    )
                }
                "intelmpi" => {
                    "-- Intel MPI modulefile - managed by Rustible\n\
                     help([[Intel MPI library for parallel computing]])\n\
                     \n\
                     local base = \"/opt/intel/oneapi/mpi/latest\"\n\
                     \n\
                     prepend_path(\"PATH\", pathJoin(base, \"bin\"))\n\
                     prepend_path(\"LD_LIBRARY_PATH\", pathJoin(base, \"lib\"))\n\
                     prepend_path(\"MANPATH\", pathJoin(base, \"man\"))\n\
                     setenv(\"MPI_HOME\", base)\n\
                     setenv(\"I_MPI_ROOT\", base)\n"
                        .to_string()
                }
                _ => unreachable!(),
            };

            let (_, existing, _) = run_cmd(
                connection,
                &format!("cat '{}' 2>/dev/null || true", module_file),
                context,
            )?;

            if existing.trim() != modulefile_content.trim() {
                if context.check_mode {
                    changes.push(format!("Would create Lmod modulefile at {}", module_file));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("mkdir -p '{}'", module_dir),
                        context,
                    )?;
                    run_cmd_ok(
                        connection,
                        &format!(
                            "printf '%s\\n' '{}' > '{}'",
                            modulefile_content.trim().replace('\'', "'\\''"),
                            module_file
                        ),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Created Lmod modulefile at {}", module_file));
                }
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} MPI changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("flavor", serde_json::json!(flavor))
            .with_data("os_family", serde_json::json!(os_family)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} MPI changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("flavor", serde_json::json!(flavor))
                    .with_data("os_family", serde_json::json!(os_family)),
            )
        } else {
            Ok(ModuleOutput::ok(format!("MPI ({}) is configured and up to date", flavor))
                .with_data("flavor", serde_json::json!(flavor))
                .with_data("os_family", serde_json::json!(os_family)))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("flavor", serde_json::json!("openmpi"));
        m.insert("mca_params", serde_json::json!(null));
        m.insert("lmod_module", serde_json::json!(false));
        m
    }
}
