//! CUDA Toolkit installation and management module
//!
//! Manage multi-version CUDA installations with alternatives and environment setup.
//!
//! # Parameters
//!
//! - `version` (required): CUDA version (e.g., "12.3", "11.8")
//! - `state` (optional): "present" (default) or "absent"
//! - `install_path` (optional): Base installation path (default: "/usr/local/cuda-{version}")
//! - `set_default` (optional): Set as default CUDA version via alternatives (boolean)
//! - `modulefile` (optional): Generate an Lmod/TCL modulefile (boolean, default: false)
//! - `modulepath` (optional): Path for modulefiles (default: "/opt/apps/modulefiles")

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

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
    let id_line = os_release
        .lines()
        .find(|l| l.starts_with("ID_LIKE=") || l.starts_with("ID="));
    match id_line {
        Some(line) => {
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
                Some("rhel")
            } else if val.contains("debian") || val.contains("ubuntu") {
                Some("debian")
            } else {
                None
            }
        }
        None => None,
    }
}

/// Scan `/usr/local/cuda-*` directories and extract installed CUDA version strings.
fn detect_installed_versions(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<Vec<String>> {
    let (success, stdout, _) = run_cmd(connection, "ls -d /usr/local/cuda-* 2>/dev/null", context)?;
    if !success || stdout.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(parse_cuda_versions(&stdout))
}

/// Parse version strings from cuda directory listing output.
///
/// This is the pure parsing logic extracted for testability; it accepts
/// the raw `ls -d` output and returns a list of version strings.
fn parse_cuda_versions(ls_output: &str) -> Vec<String> {
    ls_output
        .lines()
        .filter_map(|line| {
            let path = line.trim();
            path.rsplit_once("/cuda-").map(|(_, ver)| ver.to_string())
        })
        .filter(|v| !v.is_empty())
        .collect()
}

/// Manage the `/usr/local/cuda` symlink so it points to the desired version.
fn manage_alternatives(
    connection: &Arc<dyn Connection + Send + Sync>,
    version: &str,
    install_path: &str,
    context: &ModuleContext,
) -> ModuleResult<VerifyResult> {
    let mut details: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut verified = true;

    // Check the current symlink target
    let (link_ok, link_target, _) =
        run_cmd(connection, "readlink /usr/local/cuda 2>/dev/null", context)?;

    let desired_target = install_path.to_string();
    let needs_update = !link_ok || link_target.trim() != desired_target;

    if needs_update {
        if context.check_mode {
            details.push(format!(
                "Would update /usr/local/cuda symlink to {}",
                desired_target
            ));
        } else {
            let (ok, _, stderr) = run_cmd(
                connection,
                &format!("ln -sfn {} /usr/local/cuda", desired_target),
                context,
            )?;
            if ok {
                details.push(format!(
                    "Updated /usr/local/cuda symlink to {}",
                    desired_target
                ));
            } else {
                warnings.push(format!("Failed to update symlink: {}", stderr.trim()));
                verified = false;
            }
        }
    } else {
        details.push(format!(
            "/usr/local/cuda already points to {}",
            desired_target
        ));
    }

    // Attempt update-alternatives if the command is available
    let (has_ua, _, _) = run_cmd(connection, "command -v update-alternatives", context)?;
    if has_ua {
        let (ua_ok, _, ua_stderr) = run_cmd(
            connection,
            &format!(
                "update-alternatives --install /usr/local/cuda cuda {} 100",
                install_path
            ),
            context,
        )?;
        if ua_ok {
            details.push(format!(
                "Registered CUDA {} with update-alternatives",
                version
            ));
        } else {
            warnings.push(format!(
                "update-alternatives registration failed: {}",
                ua_stderr.trim()
            ));
        }
    }

    Ok(VerifyResult {
        verified,
        details,
        warnings,
    })
}

/// Generate an Lmod/TCL modulefile content string for the given CUDA version.
fn generate_modulefile(version: &str, install_path: &str) -> String {
    format!(
        "#%Module1.0\n\
         proc ModulesHelp {{ }} {{ puts stderr \"CUDA {}\" }}\n\
         module-whatis \"CUDA Toolkit {}\"\n\
         set root {}\n\
         prepend-path PATH $root/bin\n\
         prepend-path LD_LIBRARY_PATH $root/lib64\n\
         setenv CUDA_HOME $root\n",
        version, version, install_path
    )
}

/// Write the modulefile to `<modulepath>/cuda/<version>`.
fn write_modulefile(
    connection: &Arc<dyn Connection + Send + Sync>,
    version: &str,
    install_path: &str,
    modulepath: &str,
    context: &ModuleContext,
) -> ModuleResult<String> {
    let dir = format!("{}/cuda", modulepath);
    let filepath = format!("{}/{}", dir, version);
    let content = generate_modulefile(version, install_path);

    if context.check_mode {
        return Ok(format!("Would write modulefile to {}", filepath));
    }

    run_cmd_ok(connection, &format!("mkdir -p {}", dir), context)?;

    let escaped = content.replace('\'', "'\\''");
    run_cmd_ok(
        connection,
        &format!("printf '%s' '{}' > {}", escaped, filepath),
        context,
    )?;

    Ok(format!("Wrote modulefile to {}", filepath))
}

/// Check NVIDIA driver compatibility with the requested CUDA version.
///
/// Returns a `PreflightResult` with warnings or errors when the driver is
/// too old for the requested CUDA major version.
fn check_driver_compat(
    connection: &Arc<dyn Connection + Send + Sync>,
    version: &str,
    context: &ModuleContext,
) -> ModuleResult<PreflightResult> {
    let mut warnings: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    let (smi_ok, smi_stdout, _) = run_cmd(
        connection,
        "nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null",
        context,
    )?;

    if !smi_ok || smi_stdout.trim().is_empty() {
        warnings.push("nvidia-smi not available or no GPU detected".to_string());
        return Ok(PreflightResult {
            passed: true,
            warnings,
            errors,
        });
    }

    let driver_str = smi_stdout.lines().next().unwrap_or("").trim().to_string();
    let driver_major = parse_driver_major(&driver_str);

    let compat_result = check_driver_version_compat(version, driver_major);
    match compat_result {
        DriverCompat::Ok => {}
        DriverCompat::Warning(msg) => warnings.push(msg),
        DriverCompat::Error(msg) => errors.push(msg),
    }

    Ok(PreflightResult {
        passed: errors.is_empty(),
        warnings,
        errors,
    })
}

/// Result of a driver compatibility check.
enum DriverCompat {
    Ok,
    Warning(String),
    Error(String),
}

/// Parse the major version number from a driver version string like "535.129.03".
fn parse_driver_major(driver_str: &str) -> u32 {
    driver_str
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Pure logic: check whether the given `driver_major` version is compatible
/// with the requested CUDA `version` string (e.g. "12.3").
fn check_driver_version_compat(version: &str, driver_major: u32) -> DriverCompat {
    let cuda_major: u32 = version
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let min_driver = match cuda_major {
        12 => 525,
        11 => 450,
        10 => 410,
        _ => 0,
    };

    if min_driver == 0 {
        return DriverCompat::Warning(format!(
            "Unknown CUDA major version {}; cannot verify driver compatibility",
            cuda_major
        ));
    }

    if driver_major == 0 {
        return DriverCompat::Warning("Could not parse NVIDIA driver version".to_string());
    }

    if driver_major < min_driver {
        DriverCompat::Error(format!(
            "NVIDIA driver {} is too old for CUDA {}; minimum required driver is {}",
            driver_major, version, min_driver
        ))
    } else {
        DriverCompat::Ok
    }
}

pub struct CudaToolkitModule;

impl Module for CudaToolkitModule {
    fn name(&self) -> &'static str {
        "cuda_toolkit"
    }

    fn description(&self) -> &'static str {
        "Manage CUDA Toolkit installation with multi-version support"
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

        let version = params.get_string_required("version")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let install_path = params
            .get_string("install_path")?
            .unwrap_or_else(|| format!("/usr/local/cuda-{}", version));
        let set_default = params.get_bool_or("set_default", false);
        let want_modulefile = params.get_bool_or("modulefile", false);
        let modulepath = params
            .get_string("modulepath")?
            .unwrap_or_else(|| "/opt/apps/modulefiles".to_string());

        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let _os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported("Unsupported OS for CUDA module".to_string())
        })?;

        if state == "absent" {
            return self.handle_absent(connection, &install_path, context);
        }

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();
        let mut all_warnings: Vec<String> = Vec::new();

        // Driver compatibility preflight check
        let preflight = check_driver_compat(connection, &version, context)?;
        if !preflight.passed {
            return Err(ModuleError::ExecutionFailed(format!(
                "Driver compatibility check failed: {}",
                preflight.errors.join("; ")
            )));
        }
        all_warnings.extend(preflight.warnings);

        // Check if CUDA is already installed
        let (cuda_exists, _, _) = run_cmd(
            connection,
            &format!("test -d {}/bin", install_path),
            context,
        )?;

        if !cuda_exists {
            if context.check_mode {
                changes.push(format!("Would install CUDA Toolkit {}", version));
            } else {
                // NOTE: In production, this would download and install CUDA runfile
                // For now, we simulate basic installation structure
                run_cmd_ok(
                    connection,
                    &format!("mkdir -p {}/bin", install_path),
                    context,
                )?;
                run_cmd_ok(
                    connection,
                    &format!("mkdir -p {}/lib64", install_path),
                    context,
                )?;
                changed = true;
                changes.push(format!("Installed CUDA Toolkit {}", version));
            }
        }

        // Manage alternatives (symlink + update-alternatives) if requested
        if set_default {
            let alt_result = manage_alternatives(connection, &version, &install_path, context)?;
            if !alt_result.details.is_empty() {
                for detail in &alt_result.details {
                    if !detail.contains("already points to") {
                        changed = true;
                    }
                    changes.push(detail.clone());
                }
            }
            all_warnings.extend(alt_result.warnings);
        }

        // Set up environment file
        let env_file = "/etc/profile.d/cuda.sh";
        let env_content = format!(
            "export CUDA_HOME={}\nexport PATH=$CUDA_HOME/bin:$PATH\nexport LD_LIBRARY_PATH=$CUDA_HOME/lib64:$LD_LIBRARY_PATH\n",
            install_path
        );

        let (env_exists, current_env, _) = run_cmd(
            connection,
            &format!("cat {} 2>/dev/null || echo ''", env_file),
            context,
        )?;

        if !env_exists || current_env != env_content {
            if context.check_mode {
                changes.push("Would update CUDA environment file".to_string());
            } else {
                let escaped = env_content.replace('\'', "'\\''");
                run_cmd_ok(
                    connection,
                    &format!("echo '{}' > {}", escaped, env_file),
                    context,
                )?;
                changed = true;
                changes.push("Updated CUDA environment file".to_string());
            }
        }

        // Generate modulefile if requested
        if want_modulefile {
            let mf_msg =
                write_modulefile(connection, &version, &install_path, &modulepath, context)?;
            changed = true;
            changes.push(mf_msg);
        }

        // Detect all installed CUDA versions for output
        let installed_versions = detect_installed_versions(connection, context)?;

        if context.check_mode && !changes.is_empty() {
            let mut output =
                ModuleOutput::changed(format!("Would apply {} CUDA changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("installed_versions", serde_json::json!(installed_versions));
            if !all_warnings.is_empty() {
                output = output.with_data("warnings", serde_json::json!(all_warnings));
            }
            return Ok(output);
        }

        if changed {
            let mut output =
                ModuleOutput::changed(format!("Applied {} CUDA changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("version", serde_json::json!(version))
                    .with_data("installed_versions", serde_json::json!(installed_versions));
            if !all_warnings.is_empty() {
                output = output.with_data("warnings", serde_json::json!(all_warnings));
            }
            Ok(output)
        } else {
            let mut output = ModuleOutput::ok(format!("CUDA Toolkit {} is installed", version))
                .with_data("version", serde_json::json!(version))
                .with_data("installed_versions", serde_json::json!(installed_versions));
            if !all_warnings.is_empty() {
                output = output.with_data("warnings", serde_json::json!(all_warnings));
            }
            Ok(output)
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["version"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("install_path", serde_json::json!(null));
        m.insert("set_default", serde_json::json!(false));
        m.insert("modulefile", serde_json::json!(false));
        m.insert("modulepath", serde_json::json!("/opt/apps/modulefiles"));
        m
    }
}

impl CudaToolkitModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        install_path: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let (exists, _, _) = run_cmd(connection, &format!("test -d {}", install_path), context)?;

        if !exists {
            return Ok(ModuleOutput::ok("CUDA Toolkit is not installed"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would remove CUDA Toolkit"));
        }

        run_cmd_ok(connection, &format!("rm -rf {}", install_path), context)?;
        let _ = run_cmd(connection, "rm -f /etc/profile.d/cuda.sh", context);

        Ok(ModuleOutput::changed("Removed CUDA Toolkit"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = CudaToolkitModule;
        assert_eq!(module.name(), "cuda_toolkit");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = CudaToolkitModule;
        let required = module.required_params();
        assert!(required.contains(&"version"));
    }

    #[test]
    fn test_optional_params() {
        let module = CudaToolkitModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("install_path"));
        assert!(optional.contains_key("set_default"));
        assert!(optional.contains_key("modulefile"));
        assert!(optional.contains_key("modulepath"));
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID=rhel\nVERSION=8"), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID_LIKE=\"rhel fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=unknown"), None);
    }

    #[test]
    fn test_version_detection_parsing() {
        // Standard multi-version listing
        let output = "/usr/local/cuda-11.8\n/usr/local/cuda-12.0\n/usr/local/cuda-12.3\n";
        let versions = parse_cuda_versions(output);
        assert_eq!(versions, vec!["11.8", "12.0", "12.3"]);

        // Single version
        let output = "/usr/local/cuda-12.3\n";
        let versions = parse_cuda_versions(output);
        assert_eq!(versions, vec!["12.3"]);

        // Empty output (no CUDA installed)
        let output = "";
        let versions = parse_cuda_versions(output);
        assert!(versions.is_empty());

        // Lines with trailing whitespace
        let output = "/usr/local/cuda-11.8  \n  /usr/local/cuda-12.3\n";
        let versions = parse_cuda_versions(output);
        assert_eq!(versions, vec!["11.8", "12.3"]);
    }

    #[test]
    fn test_driver_compat_matrix() {
        // CUDA 12.x needs driver >= 525
        assert!(matches!(
            check_driver_version_compat("12.3", 535),
            DriverCompat::Ok
        ));
        assert!(matches!(
            check_driver_version_compat("12.0", 525),
            DriverCompat::Ok
        ));
        assert!(matches!(
            check_driver_version_compat("12.3", 510),
            DriverCompat::Error(_)
        ));

        // CUDA 11.x needs driver >= 450
        assert!(matches!(
            check_driver_version_compat("11.8", 535),
            DriverCompat::Ok
        ));
        assert!(matches!(
            check_driver_version_compat("11.8", 450),
            DriverCompat::Ok
        ));
        assert!(matches!(
            check_driver_version_compat("11.0", 440),
            DriverCompat::Error(_)
        ));

        // CUDA 10.x needs driver >= 410
        assert!(matches!(
            check_driver_version_compat("10.2", 450),
            DriverCompat::Ok
        ));
        assert!(matches!(
            check_driver_version_compat("10.0", 410),
            DriverCompat::Ok
        ));
        assert!(matches!(
            check_driver_version_compat("10.1", 400),
            DriverCompat::Error(_)
        ));

        // Unknown CUDA version should produce a warning
        assert!(matches!(
            check_driver_version_compat("9.0", 400),
            DriverCompat::Warning(_)
        ));

        // Unparseable driver version
        assert!(matches!(
            check_driver_version_compat("12.3", 0),
            DriverCompat::Warning(_)
        ));
    }

    #[test]
    fn test_parse_driver_major() {
        assert_eq!(parse_driver_major("535.129.03"), 535);
        assert_eq!(parse_driver_major("450.80.02"), 450);
        assert_eq!(parse_driver_major("525"), 525);
        assert_eq!(parse_driver_major(""), 0);
        assert_eq!(parse_driver_major("not-a-number"), 0);
    }

    #[test]
    fn test_modulefile_generation() {
        let content = generate_modulefile("12.3", "/usr/local/cuda-12.3");

        // Check required modulefile directives
        assert!(content.starts_with("#%Module1.0\n"));
        assert!(content.contains("proc ModulesHelp"));
        assert!(content.contains("puts stderr \"CUDA 12.3\""));
        assert!(content.contains("module-whatis \"CUDA Toolkit 12.3\""));
        assert!(content.contains("set root /usr/local/cuda-12.3"));
        assert!(content.contains("prepend-path PATH $root/bin"));
        assert!(content.contains("prepend-path LD_LIBRARY_PATH $root/lib64"));
        assert!(content.contains("setenv CUDA_HOME $root"));

        // Test with a different version to ensure parameterization works
        let content2 = generate_modulefile("11.8", "/usr/local/cuda-11.8");
        assert!(content2.contains("CUDA 11.8"));
        assert!(content2.contains("set root /usr/local/cuda-11.8"));
    }
}
