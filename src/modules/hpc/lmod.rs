//! Lmod / Environment Modules support
//!
//! Manages Lmod installation, module path directories, and profile script
//! configuration for HPC clusters.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `modulepath` (optional): List of module path directories to create
//! - `profile_script` (optional): Whether to write /etc/profile.d/lmod.sh (default: true)

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

pub struct LmodModule;

impl LmodModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        os_family: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let check_cmd = match os_family {
            "rhel" => "rpm -q Lmod >/dev/null 2>&1",
            _ => "dpkg -s lmod >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            return Ok(ModuleOutput::ok("Lmod is not installed"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would remove Lmod"));
        }

        // Remove profile script if it exists
        let _ = run_cmd(connection, "rm -f /etc/profile.d/lmod.sh", context);

        // Remove packages
        let remove_cmd = match os_family {
            "rhel" => "dnf remove -y Lmod",
            _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y lmod",
        };
        run_cmd_ok(connection, remove_cmd, context)?;

        Ok(ModuleOutput::changed("Removed Lmod"))
    }
}

impl Module for LmodModule {
    fn name(&self) -> &'static str {
        "lmod"
    }

    fn description(&self) -> &'static str {
        "Manage Lmod / Environment Modules installation and configuration"
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
        let modulepath = params.get_vec_string("modulepath")?;
        let profile_script = params.get_bool_or("profile_script", true);

        // Detect OS family
        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported(
                "Unsupported OS. Lmod module supports RHEL-family and Debian-family distributions."
                    .to_string(),
            )
        })?;

        if state == "absent" {
            return self.handle_absent(connection, os_family, context);
        }

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        let install_method = params
            .get_string("install_method")?
            .unwrap_or_else(|| "package".to_string());
        let rebuild_cache = params.get_bool_or("rebuild_cache", false);

        // Install Lmod
        let check_cmd = match os_family {
            "rhel" => "rpm -q Lmod >/dev/null 2>&1",
            _ => "dpkg -s lmod >/dev/null 2>&1",
        };
        let (pkg_installed, _, _) = run_cmd(connection, check_cmd, context)?;
        // Also check for source install
        let (src_installed, _, _) = run_cmd(
            connection,
            "test -f /usr/local/lmod/lmod/init/bash",
            context,
        )?;
        let installed = pkg_installed || src_installed;

        if !installed {
            if context.check_mode {
                changes.push(format!("Would install Lmod via {}", install_method));
            } else if install_method == "source" {
                // Install from source: download, build, install
                let build_deps = match os_family {
                    "rhel" => "dnf install -y lua lua-posix lua-filesystem gcc make tcl",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y lua5.3 liblua5.3-dev lua-posix lua-filesystem tcl make",
                };
                run_cmd_ok(connection, build_deps, context)?;
                let version = "8.7.30";
                run_cmd_ok(
                    connection,
                    &format!(
                        "cd /tmp && curl -sL https://github.com/TACC/Lmod/archive/{}.tar.gz | tar xz && cd Lmod-{} && ./configure --prefix=/usr/local && make install",
                        version, version
                    ),
                    context,
                )?;
                changed = true;
                changes.push(format!("Installed Lmod {} from source", version));
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y epel-release && dnf install -y Lmod",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y lmod",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed Lmod packages".to_string());
            }
        }

        // Create module path directories
        if let Some(ref paths) = modulepath {
            for dir in paths {
                let (exists, _, _) = run_cmd(connection, &format!("test -d '{}'", dir), context)?;
                if !exists {
                    if context.check_mode {
                        changes.push(format!("Would create module directory {}", dir));
                    } else {
                        run_cmd_ok(connection, &format!("mkdir -p '{}'", dir), context)?;
                        changed = true;
                        changes.push(format!("Created module directory {}", dir));
                    }
                }
            }
        }

        // Write /etc/profile.d/lmod.sh with MODULEPATH exports
        if profile_script {
            let module_dirs = if let Some(ref paths) = modulepath {
                paths.clone()
            } else {
                vec!["/opt/modulefiles".to_string()]
            };

            let modulepath_export = module_dirs.join(":");
            let desired_content = format!(
                "# Lmod initialization - managed by Rustible\n\
                 if [ -f /usr/share/lmod/lmod/init/bash ]; then\n\
                 \x20\x20source /usr/share/lmod/lmod/init/bash\n\
                 fi\n\
                 export MODULEPATH=\"{}\"\n",
                modulepath_export
            );

            let (_, existing, _) = run_cmd(
                connection,
                "cat /etc/profile.d/lmod.sh 2>/dev/null || true",
                context,
            )?;

            if existing.trim() != desired_content.trim() {
                if context.check_mode {
                    changes.push("Would write /etc/profile.d/lmod.sh".to_string());
                } else {
                    run_cmd_ok(
                        connection,
                        &format!(
                            "printf '%s\\n' '{}' > /etc/profile.d/lmod.sh && chmod 0644 /etc/profile.d/lmod.sh",
                            desired_content.trim().replace('\'', "'\\''")
                        ),
                        context,
                    )?;
                    changed = true;
                    changes.push("Wrote /etc/profile.d/lmod.sh".to_string());
                }
            }
        }

        // Rebuild spider cache
        if rebuild_cache {
            if context.check_mode {
                changes.push("Would rebuild Lmod spider cache".to_string());
            } else {
                // Try both standard install paths
                let (ok, _, _) = run_cmd(
                    connection,
                    "/usr/share/lmod/lmod/libexec/update_lmod_system_cache_files 2>/dev/null || /usr/local/lmod/lmod/libexec/update_lmod_system_cache_files 2>/dev/null",
                    context,
                )?;
                if ok {
                    changed = true;
                    changes.push("Rebuilt Lmod spider cache".to_string());
                }
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Lmod changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("os_family", serde_json::json!(os_family)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} Lmod changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("os_family", serde_json::json!(os_family)),
            )
        } else {
            Ok(ModuleOutput::ok("Lmod is configured and up to date")
                .with_data("os_family", serde_json::json!(os_family)))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("modulepath", serde_json::json!(null));
        m.insert("profile_script", serde_json::json!(true));
        m.insert("install_method", serde_json::json!("package"));
        m.insert("rebuild_cache", serde_json::json!(false));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = LmodModule;
        assert_eq!(module.name(), "lmod");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_optional_params() {
        let module = LmodModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("modulepath"));
        assert!(optional.contains_key("profile_script"));
        assert!(optional.contains_key("install_method"));
        assert!(optional.contains_key("rebuild_cache"));
    }

    #[test]
    fn test_required_params_empty() {
        let module = LmodModule;
        assert!(module.required_params().is_empty());
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(
            detect_os_family("ID=rocky\nVERSION_ID=\"9.0\""),
            Some("rhel")
        );
        assert_eq!(
            detect_os_family("ID=ubuntu\nVERSION_ID=\"22.04\""),
            Some("debian")
        );
        assert_eq!(detect_os_family("ID=unknown"), None);
    }
}
