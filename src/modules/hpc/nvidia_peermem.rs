//! NVIDIA nvidia-peermem kernel module
//!
//! Manages the nvidia-peermem kernel module for GPUDirect RDMA support,
//! which enables direct GPU memory access over InfiniBand/RDMA fabrics.
//! Replaces the legacy nv_peer_mem module.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `autoload` (optional): Persist module across reboots (default: true)
//! - `blacklist_legacy` (optional): Blacklist old nv_peer_mem module (default: true)

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

// ---- Helper structs ----

#[derive(Debug, serde::Serialize)]
struct PeermemStatus {
    module_loaded: bool,
    legacy_blacklisted: bool,
    autoload_configured: bool,
    driver_version: Option<String>,
}

// ---- Helper functions ----

/// Parse driver version from `nvidia-smi` output (first line of CSV output).
fn parse_driver_version(nvidia_smi_output: &str) -> Option<String> {
    let version = nvidia_smi_output.trim();
    if version.is_empty() {
        return None;
    }
    Some(
        version
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string(),
    )
}

/// Parse the major version number from a driver version string like "535.183.01".
fn parse_driver_major(driver_str: &str) -> u32 {
    driver_str
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
}

/// Check if a kernel module is loaded by examining `lsmod` output.
///
/// Note: `nvidia-peermem` (hyphen) in modprobe becomes `nvidia_peermem`
/// (underscore) in lsmod. This function matches the exact module name as
/// it appears in lsmod.
fn module_is_loaded(lsmod_output: &str, module_name: &str) -> bool {
    for line in lsmod_output.lines() {
        let first_field = line.split_whitespace().next().unwrap_or("");
        if first_field == module_name {
            return true;
        }
    }
    false
}

// ---- NVIDIA Peermem Module ----

pub struct NvidiaPeermemModule;

impl Module for NvidiaPeermemModule {
    fn name(&self) -> &'static str {
        "nvidia_peermem"
    }

    fn description(&self) -> &'static str {
        "Manage nvidia-peermem kernel module for GPUDirect RDMA"
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
        let autoload = params.get_bool_or("autoload", true);
        let blacklist_legacy = params.get_bool_or("blacklist_legacy", true);

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // -- state=absent --
        if state == "absent" {
            let (_, lsmod_stdout, _) = run_cmd(connection, "lsmod", context)?;

            // Unload nvidia_peermem
            if module_is_loaded(&lsmod_stdout, "nvidia_peermem") {
                if context.check_mode {
                    changes.push("Would unload nvidia_peermem kernel module".to_string());
                } else {
                    run_cmd_ok(connection, "rmmod nvidia_peermem", context)?;
                    changed = true;
                    changes.push("Unloaded nvidia_peermem kernel module".to_string());
                }
            }

            // Remove autoload config
            let (autoload_exists, _, _) = run_cmd(
                connection,
                "test -f /etc/modules-load.d/nvidia-peermem.conf",
                context,
            )?;
            if autoload_exists {
                if context.check_mode {
                    changes.push(
                        "Would remove /etc/modules-load.d/nvidia-peermem.conf".to_string(),
                    );
                } else {
                    run_cmd_ok(
                        connection,
                        "rm -f /etc/modules-load.d/nvidia-peermem.conf",
                        context,
                    )?;
                    changed = true;
                    changes.push("Removed /etc/modules-load.d/nvidia-peermem.conf".to_string());
                }
            }

            // Remove blacklist config
            let (blacklist_exists, _, _) = run_cmd(
                connection,
                "test -f /etc/modprobe.d/blacklist-nv-peer-mem.conf",
                context,
            )?;
            if blacklist_exists {
                if context.check_mode {
                    changes.push(
                        "Would remove /etc/modprobe.d/blacklist-nv-peer-mem.conf".to_string(),
                    );
                } else {
                    run_cmd_ok(
                        connection,
                        "rm -f /etc/modprobe.d/blacklist-nv-peer-mem.conf",
                        context,
                    )?;
                    changed = true;
                    changes.push("Removed /etc/modprobe.d/blacklist-nv-peer-mem.conf".to_string());
                }
            }

            if context.check_mode && !changes.is_empty() {
                return Ok(ModuleOutput::changed(format!(
                    "Would apply {} nvidia-peermem removal changes",
                    changes.len()
                ))
                .with_data("changes", serde_json::json!(changes)));
            }

            if changed {
                return Ok(
                    ModuleOutput::changed("Removed nvidia-peermem")
                        .with_data("changes", serde_json::json!(changes)),
                );
            }

            return Ok(ModuleOutput::ok("nvidia-peermem is not loaded"));
        }

        // -- state=present --

        // Step 1: Verify driver version >= 470
        let (smi_ok, smi_stdout, _) = run_cmd(
            connection,
            "nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null",
            context,
        )?;

        let driver_version = if smi_ok {
            parse_driver_version(&smi_stdout)
        } else {
            None
        };

        if let Some(ref ver) = driver_version {
            let major = parse_driver_major(ver);
            if major < 470 {
                return Err(ModuleError::ExecutionFailed(format!(
                    "nvidia-peermem requires driver >= 470, found {} (major {})",
                    ver, major
                )));
            }
        } else if !context.check_mode {
            return Err(ModuleError::ExecutionFailed(
                "Cannot verify NVIDIA driver version; nvidia-smi not available".to_string(),
            ));
        }

        // Step 2: Check RDMA stack
        let (_, lsmod_stdout, _) = run_cmd(connection, "lsmod", context)?;
        if !module_is_loaded(&lsmod_stdout, "ib_core") && !context.check_mode {
            return Err(ModuleError::ExecutionFailed(
                "RDMA stack not loaded (ib_core module not found). Install and configure OFED first."
                    .to_string(),
            ));
        }

        // Step 3: Blacklist legacy nv_peer_mem
        if blacklist_legacy {
            // Unload legacy module if present
            if module_is_loaded(&lsmod_stdout, "nv_peer_mem") {
                if context.check_mode {
                    changes.push("Would unload legacy nv_peer_mem module".to_string());
                } else {
                    let _ = run_cmd(connection, "rmmod nv_peer_mem", context);
                    changed = true;
                    changes.push("Unloaded legacy nv_peer_mem module".to_string());
                }
            }

            // Write blacklist config
            let (blacklist_exists, _, _) = run_cmd(
                connection,
                "test -f /etc/modprobe.d/blacklist-nv-peer-mem.conf",
                context,
            )?;

            if !blacklist_exists {
                if context.check_mode {
                    changes.push(
                        "Would create /etc/modprobe.d/blacklist-nv-peer-mem.conf".to_string(),
                    );
                } else {
                    run_cmd_ok(
                        connection,
                        "echo 'blacklist nv_peer_mem' > /etc/modprobe.d/blacklist-nv-peer-mem.conf",
                        context,
                    )?;
                    changed = true;
                    changes.push("Blacklisted legacy nv_peer_mem module".to_string());
                }
            }
        }

        // Step 4: Load nvidia-peermem
        // Note: modprobe uses hyphen, lsmod shows underscore
        if !module_is_loaded(&lsmod_stdout, "nvidia_peermem") {
            if context.check_mode {
                changes.push("Would load nvidia-peermem kernel module".to_string());
            } else {
                run_cmd_ok(connection, "modprobe nvidia-peermem", context)?;
                changed = true;
                changes.push("Loaded nvidia-peermem kernel module".to_string());
            }
        }

        // Step 5: Configure autoload
        if autoload {
            let (autoload_exists, _, _) = run_cmd(
                connection,
                "test -f /etc/modules-load.d/nvidia-peermem.conf",
                context,
            )?;

            if !autoload_exists {
                if context.check_mode {
                    changes.push(
                        "Would create /etc/modules-load.d/nvidia-peermem.conf".to_string(),
                    );
                } else {
                    run_cmd_ok(
                        connection,
                        "echo 'nvidia-peermem' > /etc/modules-load.d/nvidia-peermem.conf",
                        context,
                    )?;
                    changed = true;
                    changes.push("Created /etc/modules-load.d/nvidia-peermem.conf".to_string());
                }
            }
        }

        // Step 6: Verify
        let final_loaded = if !context.check_mode {
            let (_, lsmod_final, _) = run_cmd(connection, "lsmod", context)?;
            module_is_loaded(&lsmod_final, "nvidia_peermem")
        } else {
            false
        };

        let status = PeermemStatus {
            module_loaded: final_loaded,
            legacy_blacklisted: blacklist_legacy,
            autoload_configured: autoload,
            driver_version: driver_version.clone(),
        };

        // Build output
        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} nvidia-peermem changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!(
                "Applied {} nvidia-peermem changes",
                changes.len()
            ))
        } else {
            ModuleOutput::ok("nvidia-peermem is loaded and configured")
        };

        output = output
            .with_data("changes", serde_json::json!(changes))
            .with_data("status", serde_json::json!(status));

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("autoload", serde_json::json!(true));
        m.insert("blacklist_legacy", serde_json::json!(true));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = NvidiaPeermemModule;
        assert_eq!(module.name(), "nvidia_peermem");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params().len(), 0);
    }

    #[test]
    fn test_parse_driver_version() {
        assert_eq!(
            parse_driver_version("535.183.01"),
            Some("535.183.01".to_string())
        );
        assert_eq!(
            parse_driver_version("550.120\n535.183.01\n"),
            Some("550.120".to_string())
        );
        assert_eq!(parse_driver_version(""), None);
        assert_eq!(
            parse_driver_version("  535.183.01  \n"),
            Some("535.183.01".to_string())
        );
    }

    #[test]
    fn test_parse_driver_major() {
        assert_eq!(parse_driver_major("535.183.01"), 535);
        assert_eq!(parse_driver_major("470.82.01"), 470);
        assert_eq!(parse_driver_major("550"), 550);
        assert_eq!(parse_driver_major(""), 0);
        assert_eq!(parse_driver_major("not-a-number"), 0);
    }

    #[test]
    fn test_driver_version_470_check() {
        // Versions >= 470 should pass
        assert!(parse_driver_major("535.183.01") >= 470);
        assert!(parse_driver_major("470.82.01") >= 470);
        assert!(parse_driver_major("550.120") >= 470);

        // Versions < 470 should fail
        assert!(parse_driver_major("460.91.03") < 470);
        assert!(parse_driver_major("450.80.02") < 470);
    }

    #[test]
    fn test_module_is_loaded_nvidia_peermem() {
        let lsmod = "Module                  Size  Used by\nnvidia_peermem         16384  0\nnvidia              57344000  1\nib_core               413696  9 rdma_ucm,ib_ipoib,nvidia_peermem\n";
        assert!(module_is_loaded(lsmod, "nvidia_peermem"));
        assert!(module_is_loaded(lsmod, "nvidia"));
        assert!(module_is_loaded(lsmod, "ib_core"));
    }

    #[test]
    fn test_module_is_loaded_nv_peer_mem() {
        let lsmod = "Module                  Size  Used by\nnv_peer_mem            16384  0\nnvidia              57344000  1\n";
        assert!(module_is_loaded(lsmod, "nv_peer_mem"));
        // nvidia_peermem should NOT match nv_peer_mem
        assert!(!module_is_loaded(lsmod, "nvidia_peermem"));
    }

    #[test]
    fn test_module_is_loaded_empty() {
        assert!(!module_is_loaded("", "nvidia_peermem"));
    }

    #[test]
    fn test_module_is_loaded_partial_name() {
        // Should NOT match partial module names
        let lsmod = "Module                  Size  Used by\nnvidia_peermem_extra  16384  0\n";
        assert!(!module_is_loaded(lsmod, "nvidia_peermem"));
    }

    #[test]
    fn test_peermem_status_serialization() {
        let status = PeermemStatus {
            module_loaded: true,
            legacy_blacklisted: true,
            autoload_configured: true,
            driver_version: Some("535.183.01".to_string()),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["module_loaded"], true);
        assert_eq!(json["legacy_blacklisted"], true);
        assert_eq!(json["autoload_configured"], true);
        assert_eq!(json["driver_version"], "535.183.01");
    }
}
