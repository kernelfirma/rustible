//! GDRCopy module
//!
//! Installs and configures GDRCopy for GPU-direct RDMA memory copies,
//! including kernel module loading and optional sanity validation.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `autoload` (optional): Persist gdrdrv module across reboots (default: true)
//! - `validate` (optional): Run gdrcopy_sanity validation test (default: false)

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
struct GdrcopyStatus {
    module_loaded: bool,
    device_present: bool,
    autoload_configured: bool,
}

#[derive(Debug, serde::Serialize)]
struct SanityResult {
    passed: bool,
    details: Vec<String>,
}

// ---- Helper functions ----

/// Check if a kernel module is loaded by examining `lsmod` output.
///
/// Looks for an exact match on the module name at the start of a line.
fn module_is_loaded(lsmod_output: &str, module_name: &str) -> bool {
    for line in lsmod_output.lines() {
        let first_field = line.split_whitespace().next().unwrap_or("");
        if first_field == module_name {
            return true;
        }
    }
    false
}

/// Parse `gdrcopy_sanity` output to determine pass/fail.
///
/// Example output:
/// ```text
/// GPU 0: NVIDIA A100-SXM4-80GB (GPU-...)
/// Testing data validation with GDRCopy...
/// Buffer size: 4194304 bytes
/// Validating data... OK
/// gdrcopy_sanity: PASSED
/// ```
fn parse_sanity_output(output: &str) -> SanityResult {
    let mut passed = false;
    let mut details = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        details.push(line.to_string());
        if line.to_lowercase().contains("passed") {
            passed = true;
        } else if line.to_lowercase().contains("failed") || line.to_lowercase().contains("error") {
            passed = false;
        }
    }

    SanityResult { passed, details }
}

// ---- GDRCopy Module ----

pub struct GdrcopyModule;

impl Module for GdrcopyModule {
    fn name(&self) -> &'static str {
        "gdrcopy"
    }

    fn description(&self) -> &'static str {
        "Install and configure GDRCopy for GPU-direct RDMA memory copies"
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
            ModuleError::Unsupported("Unsupported OS for GDRCopy module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let autoload = params.get_bool_or("autoload", true);
        let validate = params.get_bool_or("validate", false);

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // -- state=absent --
        if state == "absent" {
            // Unload module
            let (_, lsmod_stdout, _) = run_cmd(connection, "lsmod", context)?;
            if module_is_loaded(&lsmod_stdout, "gdrdrv") {
                if context.check_mode {
                    changes.push("Would unload gdrdrv kernel module".to_string());
                } else {
                    run_cmd_ok(connection, "rmmod gdrdrv", context)?;
                    changes.push("Unloaded gdrdrv kernel module".to_string());
                }
            }

            // Remove autoload config
            let (autoload_exists, _, _) = run_cmd(
                connection,
                "test -f /etc/modules-load.d/gdrdrv.conf",
                context,
            )?;
            if autoload_exists {
                if context.check_mode {
                    changes.push("Would remove /etc/modules-load.d/gdrdrv.conf".to_string());
                } else {
                    run_cmd_ok(
                        connection,
                        "rm -f /etc/modules-load.d/gdrdrv.conf",
                        context,
                    )?;
                    changes.push("Removed /etc/modules-load.d/gdrdrv.conf".to_string());
                }
            }

            // Remove packages
            let remove_cmd = match os_family {
                "rhel" => "dnf remove -y gdrdrv-dkms libgdrapi gdrcopy-tests",
                _ => "DEBIAN_FRONTEND=noninteractive apt-get remove --purge -y gdrdrv-dkms libgdrapi gdrcopy-tests",
            };
            if !context.check_mode {
                let _ = run_cmd(connection, remove_cmd, context);
                changes.push("Removed GDRCopy packages".to_string());
            } else {
                changes.push("Would remove GDRCopy packages".to_string());
            }

            if context.check_mode && !changes.is_empty() {
                return Ok(ModuleOutput::changed(format!(
                    "Would apply {} GDRCopy removal changes",
                    changes.len()
                ))
                .with_data("changes", serde_json::json!(changes)));
            }

            return Ok(
                ModuleOutput::changed("Removed GDRCopy")
                    .with_data("changes", serde_json::json!(changes)),
            );
        }

        // -- state=present --

        // Step 1: Install packages
        let check_cmd = match os_family {
            "rhel" => "rpm -q gdrdrv-dkms >/dev/null 2>&1",
            _ => "dpkg -s gdrdrv-dkms >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install GDRCopy packages".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y gdrdrv-dkms libgdrapi gdrcopy-tests",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y gdrdrv-dkms libgdrapi gdrcopy-tests",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed GDRCopy packages".to_string());
            }
        }

        // Step 2: Load kernel module
        if !context.check_mode {
            let (_, lsmod_stdout, _) = run_cmd(connection, "lsmod", context)?;
            if !module_is_loaded(&lsmod_stdout, "gdrdrv") {
                run_cmd_ok(connection, "modprobe gdrdrv", context)?;
                changed = true;
                changes.push("Loaded gdrdrv kernel module".to_string());
            }
        } else if !installed {
            changes.push("Would load gdrdrv kernel module".to_string());
        }

        // Step 3: Configure autoload
        if autoload {
            let (autoload_exists, _, _) = run_cmd(
                connection,
                "test -f /etc/modules-load.d/gdrdrv.conf",
                context,
            )?;

            if !autoload_exists {
                if context.check_mode {
                    changes.push("Would create /etc/modules-load.d/gdrdrv.conf".to_string());
                } else {
                    run_cmd_ok(
                        connection,
                        "echo 'gdrdrv' > /etc/modules-load.d/gdrdrv.conf",
                        context,
                    )?;
                    changed = true;
                    changes.push("Created /etc/modules-load.d/gdrdrv.conf".to_string());
                }
            }
        }

        // Step 4: Verify /dev/gdrdrv
        let device_present = if !context.check_mode {
            let (exists, _, _) = run_cmd(connection, "test -c /dev/gdrdrv", context)?;
            if !exists {
                changes.push("Warning: /dev/gdrdrv device not found".to_string());
            }
            exists
        } else {
            false
        };

        // Step 5: Run sanity test
        let sanity_result = if validate && !context.check_mode {
            let (ok, stdout, stderr) = run_cmd(connection, "gdrcopy_sanity", context)?;
            let combined = format!("{}\n{}", stdout, stderr);
            let result = parse_sanity_output(&combined);
            if !result.passed && !ok {
                changes.push("GDRCopy sanity test failed".to_string());
            } else {
                changes.push("GDRCopy sanity test passed".to_string());
            }
            Some(result)
        } else if validate && context.check_mode {
            changes.push("Would run gdrcopy_sanity".to_string());
            None
        } else {
            None
        };

        let (_, lsmod_final, _) = if !context.check_mode {
            run_cmd(connection, "lsmod", context)?
        } else {
            (false, String::new(), String::new())
        };

        let status = GdrcopyStatus {
            module_loaded: !context.check_mode && module_is_loaded(&lsmod_final, "gdrdrv"),
            device_present,
            autoload_configured: autoload,
        };

        // Build output
        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} GDRCopy changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!("Applied {} GDRCopy changes", changes.len()))
        } else {
            ModuleOutput::ok("GDRCopy is installed and configured")
        };

        output = output
            .with_data("changes", serde_json::json!(changes))
            .with_data("status", serde_json::json!(status));

        if let Some(ref sanity) = sanity_result {
            output = output.with_data("sanity_test", serde_json::json!(sanity));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("autoload", serde_json::json!(true));
        m.insert("validate", serde_json::json!(false));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = GdrcopyModule;
        assert_eq!(module.name(), "gdrcopy");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params().len(), 0);
    }

    #[test]
    fn test_module_is_loaded_positive() {
        let lsmod = "Module                  Size  Used by\ngdrdrv                 53248  0\nnvidia              57344000  1\n";
        assert!(module_is_loaded(lsmod, "gdrdrv"));
        assert!(module_is_loaded(lsmod, "nvidia"));
    }

    #[test]
    fn test_module_is_loaded_negative() {
        let lsmod = "Module                  Size  Used by\nnvidia              57344000  1\n";
        assert!(!module_is_loaded(lsmod, "gdrdrv"));
    }

    #[test]
    fn test_module_is_loaded_partial_name() {
        // Should NOT match partial names
        let lsmod = "Module                  Size  Used by\ngdrdrv_extra         53248  0\n";
        assert!(!module_is_loaded(lsmod, "gdrdrv"));
    }

    #[test]
    fn test_parse_sanity_pass() {
        let output = r#"GPU 0: NVIDIA A100-SXM4-80GB (GPU-abcd1234)
Testing data validation with GDRCopy...
Buffer size: 4194304 bytes
Validating data... OK
gdrcopy_sanity: PASSED
"#;
        let result = parse_sanity_output(output);
        assert!(result.passed);
        assert!(!result.details.is_empty());
    }

    #[test]
    fn test_parse_sanity_fail() {
        let output = "gdrcopy_sanity: FAILED\nError: could not open /dev/gdrdrv\n";
        let result = parse_sanity_output(output);
        assert!(!result.passed);
        assert!(result.details.len() >= 2);
    }

    #[test]
    fn test_parse_sanity_empty() {
        let result = parse_sanity_output("");
        assert!(!result.passed);
        assert!(result.details.is_empty());
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID_LIKE=\"rhel centos fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID=freebsd"), None);
    }

    #[test]
    fn test_gdrcopy_status_serialization() {
        let status = GdrcopyStatus {
            module_loaded: true,
            device_present: true,
            autoload_configured: true,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["module_loaded"], true);
        assert_eq!(json["device_present"], true);
        assert_eq!(json["autoload_configured"], true);
    }

    #[test]
    fn test_sanity_result_serialization() {
        let result = SanityResult {
            passed: true,
            details: vec!["gdrcopy_sanity: PASSED".to_string()],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["passed"], true);
        assert_eq!(json["details"].as_array().unwrap().len(), 1);
    }
}
