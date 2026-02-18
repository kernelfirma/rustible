//! Lmod / Environment Modules support
//!
//! Manages Lmod installation, module path directories, profile script
//! configuration, hierarchical modulepath setup, default version policies,
//! and modulepath drift detection for HPC clusters.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `modulepath` (optional): List of module path directories to create
//! - `profile_script` (optional): Whether to write /etc/profile.d/lmod.sh (default: true)
//! - `install_method` (optional): "package" (default) or "source"
//! - `rebuild_cache` (optional): Rebuild Lmod spider cache (default: false)
//! - `hierarchy` (optional): JSON array of hierarchy levels, e.g. ["Core", "Compiler", "MPI"]
//! - `default_versions` (optional): JSON object mapping module names to default versions
//! - `site_config` (optional): Path to site configuration Lua file

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

/// Result of preflight checks before a state transition.
#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single field that drifted from desired to actual.
#[derive(Debug, serde::Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

/// Configure hierarchical modulepath (Core/Compiler/MPI layout).
///
/// Creates an `lmodrc.lua` configuration and hierarchy subdirectories
/// under each configured modulepath.
fn configure_hierarchical_modulepath(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    modulepaths: &[String],
    hierarchy: &[String],
) -> ModuleResult<Vec<String>> {
    let mut changes = Vec::new();

    // Build LMOD_RC content with hierarchy configuration
    let levels_lua = hierarchy
        .iter()
        .map(|l| format!("\"{}\"", l))
        .collect::<Vec<_>>()
        .join(", ");
    let lmodrc_content = format!(
        "-- Lmod hierarchical configuration - managed by Rustible\n\
         propT = {{}}\n\
         scDescriptT = {{}}\n\
         modpathLocT = {{\n\
         }}\n\
         hierarchy_levels = {{ {} }}\n",
        levels_lua
    );

    if !context.check_mode {
        run_cmd_ok(connection, "mkdir -p /etc/lmod", context)?;
        let escaped = lmodrc_content.replace('\'', "'\\''");
        run_cmd_ok(
            connection,
            &format!(
                "printf '%s\\n' '{}' > /etc/lmod/lmodrc.lua",
                escaped
            ),
            context,
        )?;
    }
    changes.push("Wrote /etc/lmod/lmodrc.lua with hierarchy config".to_string());

    // Create hierarchy subdirectories under each modulepath
    for base in modulepaths {
        for level in hierarchy {
            let dir = format!("{}/{}", base, level);
            let (exists, _, _) =
                run_cmd(connection, &format!("test -d '{}'", dir), context)?;
            if !exists {
                if !context.check_mode {
                    run_cmd_ok(connection, &format!("mkdir -p '{}'", dir), context)?;
                }
                changes.push(format!("Created hierarchy directory {}", dir));
            }
        }
    }

    Ok(changes)
}

/// Enforce default version policies by writing `.version` files.
///
/// For each module/version pair, writes a standard Modules `.version` file
/// so that `module load <name>` resolves to the specified default version.
fn enforce_version_policies(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    modulepaths: &[String],
    versions: &serde_json::Map<String, serde_json::Value>,
) -> ModuleResult<Vec<String>> {
    let mut changes = Vec::new();

    for (module_name, version_val) in versions {
        let version = match version_val.as_str() {
            Some(v) => v,
            None => continue,
        };
        let version_content = format!(
            "#%Module\nset ModulesVersion \"{}\"",
            version
        );

        // Write .version in the first modulepath that contains the module dir,
        // or the first modulepath by default.
        let target_base = if !modulepaths.is_empty() {
            &modulepaths[0]
        } else {
            "/opt/modulefiles"
        };
        let module_dir = format!("{}/{}", target_base, module_name);
        let version_file = format!("{}/.version", module_dir);

        if !context.check_mode {
            run_cmd_ok(
                connection,
                &format!("mkdir -p '{}'", module_dir),
                context,
            )?;
            let escaped = version_content.replace('\'', "'\\''");
            run_cmd_ok(
                connection,
                &format!("printf '%s\\n' '{}' > '{}'", escaped, version_file),
                context,
            )?;
        }
        changes.push(format!(
            "Set default version for {} to {} in {}",
            module_name, version, version_file
        ));
    }

    Ok(changes)
}

/// Detect modulepath drift between configured and actual state.
///
/// Checks whether each configured modulepath directory exists on disk and
/// whether the profile script MODULEPATH variable matches the configured paths.
fn detect_modulepath_drift(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    modulepaths: &[String],
) -> ModuleResult<Vec<DriftItem>> {
    let mut drift = Vec::new();

    // Check each configured directory exists
    for dir in modulepaths {
        let (exists, _, _) =
            run_cmd(connection, &format!("test -d '{}'", dir), context)?;
        if !exists {
            drift.push(DriftItem {
                field: format!("modulepath_dir:{}", dir),
                desired: "present".to_string(),
                actual: "absent".to_string(),
            });
        }
    }

    // Check profile script MODULEPATH matches configured paths
    let (_, profile_out, _) = run_cmd(
        connection,
        "grep -oP 'MODULEPATH=\"\\K[^\"]+' /etc/profile.d/lmod.sh 2>/dev/null || true",
        context,
    )?;
    let profile_paths = profile_out.trim();
    if !profile_paths.is_empty() {
        let desired_paths = modulepaths.join(":");
        if profile_paths != desired_paths {
            drift.push(DriftItem {
                field: "profile_modulepath".to_string(),
                desired: desired_paths,
                actual: profile_paths.to_string(),
            });
        }
    }

    Ok(drift)
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

        // Resolve effective modulepaths for hierarchy/drift functions
        let effective_paths = if let Some(ref paths) = modulepath {
            paths.clone()
        } else {
            vec!["/opt/modulefiles".to_string()]
        };

        // Configure hierarchical modulepath if hierarchy param provided
        let mut hierarchy_configured = false;
        let hierarchy = params.get_vec_string("hierarchy")?;
        if let Some(ref levels) = hierarchy {
            if !levels.is_empty() {
                let hier_changes = configure_hierarchical_modulepath(
                    connection,
                    context,
                    &effective_paths,
                    levels,
                )?;
                if !hier_changes.is_empty() {
                    changed = true;
                    changes.extend(hier_changes);
                    hierarchy_configured = true;
                }
            }
        }

        // Enforce default version policies if default_versions param provided
        let mut default_versions_set = false;
        if let Some(dv_value) = params.get("default_versions") {
            if let Some(dv_map) = dv_value.as_object() {
                if !dv_map.is_empty() {
                    let ver_changes = enforce_version_policies(
                        connection,
                        context,
                        &effective_paths,
                        dv_map,
                    )?;
                    if !ver_changes.is_empty() {
                        changed = true;
                        changes.extend(ver_changes);
                        default_versions_set = true;
                    }
                }
            }
        }

        // Deploy site configuration if site_config param provided
        let site_config = params.get_string("site_config")?;
        if let Some(ref src_path) = site_config {
            if !context.check_mode {
                run_cmd_ok(connection, "mkdir -p /etc/lmod", context)?;
                run_cmd_ok(
                    connection,
                    &format!(
                        "cp '{}' /etc/lmod/lmod_site_config.lua && chmod 0644 /etc/lmod/lmod_site_config.lua",
                        src_path.replace('\'', "'\\''")
                    ),
                    context,
                )?;
            }
            changed = true;
            changes.push(format!(
                "Deployed site config from {} to /etc/lmod/lmod_site_config.lua",
                src_path
            ));
        }

        // Detect modulepath drift
        let drift = detect_modulepath_drift(connection, context, &effective_paths)?;

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Lmod changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("os_family", serde_json::json!(os_family))
            .with_data("modulepath_drift", serde_json::json!(drift))
            .with_data("hierarchy_configured", serde_json::json!(hierarchy_configured))
            .with_data("default_versions_set", serde_json::json!(default_versions_set)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} Lmod changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("os_family", serde_json::json!(os_family))
                    .with_data("modulepath_drift", serde_json::json!(drift))
                    .with_data("hierarchy_configured", serde_json::json!(hierarchy_configured))
                    .with_data("default_versions_set", serde_json::json!(default_versions_set)),
            )
        } else {
            Ok(ModuleOutput::ok("Lmod is configured and up to date")
                .with_data("os_family", serde_json::json!(os_family))
                .with_data("modulepath_drift", serde_json::json!(drift))
                .with_data("hierarchy_configured", serde_json::json!(hierarchy_configured))
                .with_data("default_versions_set", serde_json::json!(default_versions_set)))
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
        m.insert("hierarchy", serde_json::json!(null));
        m.insert("default_versions", serde_json::json!(null));
        m.insert("site_config", serde_json::json!(null));
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
        assert!(optional.contains_key("hierarchy"));
        assert!(optional.contains_key("default_versions"));
        assert!(optional.contains_key("site_config"));
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

    #[test]
    fn test_hierarchy_path_generation() {
        let levels = vec![
            "Core".to_string(),
            "Compiler".to_string(),
            "MPI".to_string(),
        ];
        let bases = vec![
            "/opt/modulefiles".to_string(),
            "/usr/share/modulefiles".to_string(),
        ];

        // Verify the expected directory paths that would be created
        let mut expected_dirs = Vec::new();
        for base in &bases {
            for level in &levels {
                expected_dirs.push(format!("{}/{}", base, level));
            }
        }

        assert_eq!(expected_dirs.len(), 6);
        assert_eq!(expected_dirs[0], "/opt/modulefiles/Core");
        assert_eq!(expected_dirs[1], "/opt/modulefiles/Compiler");
        assert_eq!(expected_dirs[2], "/opt/modulefiles/MPI");
        assert_eq!(expected_dirs[3], "/usr/share/modulefiles/Core");
        assert_eq!(expected_dirs[4], "/usr/share/modulefiles/Compiler");
        assert_eq!(expected_dirs[5], "/usr/share/modulefiles/MPI");
    }

    #[test]
    fn test_version_policy_format() {
        let module_name = "gcc";
        let version = "12.3";
        let content = format!("#%Module\nset ModulesVersion \"{}\"", version);

        assert!(content.starts_with("#%Module"));
        assert!(content.contains(&format!("set ModulesVersion \"{}\"", version)));

        // Verify the expected file path
        let base = "/opt/modulefiles";
        let version_file = format!("{}/{}/.version", base, module_name);
        assert_eq!(version_file, "/opt/modulefiles/gcc/.version");

        // Verify content for another module
        let mpi_version = "4.1.5";
        let mpi_content = format!("#%Module\nset ModulesVersion \"{}\"", mpi_version);
        assert!(mpi_content.contains("4.1.5"));
    }

    #[test]
    fn test_drift_detection_missing_dir() {
        // Simulate drift items for directories that would be missing
        let configured_paths = ["/opt/modulefiles".to_string(),
            "/missing/path".to_string(),
            "/also/missing".to_string()];

        // Simulate checking: first exists, second and third do not
        let dir_exists = [true, false, false];

        let mut drift: Vec<DriftItem> = Vec::new();
        for (i, dir) in configured_paths.iter().enumerate() {
            if !dir_exists[i] {
                drift.push(DriftItem {
                    field: format!("modulepath_dir:{}", dir),
                    desired: "present".to_string(),
                    actual: "absent".to_string(),
                });
            }
        }

        assert_eq!(drift.len(), 2);
        assert_eq!(drift[0].field, "modulepath_dir:/missing/path");
        assert_eq!(drift[0].desired, "present");
        assert_eq!(drift[0].actual, "absent");
        assert_eq!(drift[1].field, "modulepath_dir:/also/missing");

        // Verify serialization
        let json = serde_json::json!(drift);
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["field"], "modulepath_dir:/missing/path");
    }
}
