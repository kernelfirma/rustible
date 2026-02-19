//! NVIDIA MIG (Multi-Instance GPU) configuration module
//!
//! Manages MIG mode enablement, GPU instance creation, and compute instance
//! configuration on supported NVIDIA GPUs (A100, A30, H100, etc.).
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent" (destroy + disable MIG)
//! - `gpu_id` (required): GPU index or UUID
//! - `mig_enabled` (optional): Enable/disable MIG mode (default: true)
//! - `profiles` (optional): List of MIG profiles, e.g. ["1g.5gb", "2g.10gb"]
//! - `auto_create_compute` (optional): Auto-create compute instances with -C (default: true)

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
struct MigInstance {
    gpu_instance_id: String,
    profile: String,
    placement: String,
}

#[derive(Debug, serde::Serialize)]
struct MigStatus {
    mig_enabled: bool,
    instances: Vec<MigInstance>,
    available_profiles: Vec<String>,
    reboot_required: bool,
}

// ---- Helper functions ----

/// Parse MIG mode from `nvidia-smi -i <gpu> --query-gpu=mig.mode.current --format=csv,noheader`.
///
/// Output is typically "Enabled" or "Disabled".
fn parse_mig_mode(output: &str) -> Option<bool> {
    let val = output.trim().to_lowercase();
    match val.as_str() {
        "enabled" => Some(true),
        "disabled" => Some(false),
        _ => None,
    }
}

/// Parse existing GPU instances from `nvidia-smi mig -lgi -i <gpu>` output.
///
/// Example output:
/// ```text
/// +-------------------------------------------------------+
/// | GPU instances:                                         |
/// | GPU   Name          Profile  Instance   Placement     |
/// |                       ID       ID       Start:Size    |
/// |=======================================================|
/// |   0  MIG 1g.5gb        19        1          0:1       |
/// |   0  MIG 2g.10gb        14        2          1:2      |
/// +-------------------------------------------------------+
/// ```
fn parse_gpu_instances(output: &str) -> Vec<MigInstance> {
    let mut instances = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with('|') || line.contains("GPU instances") || line.contains("GPU   Name") || line.contains("ID") {
            continue;
        }
        // Strip leading/trailing '|' and parse columns
        let inner = line.trim_matches('|').trim();
        if inner.starts_with('=') || inner.starts_with('-') || inner.starts_with('+') || inner.is_empty() {
            continue;
        }
        let parts: Vec<&str> = inner.split_whitespace().collect();
        // Expected: GPU_IDX  "MIG"  profile  profile_id  instance_id  placement
        if parts.len() >= 6 && parts[1] == "MIG" {
            instances.push(MigInstance {
                gpu_instance_id: parts[4].to_string(),
                profile: parts[2].to_string(),
                placement: parts[5].to_string(),
            });
        }
    }
    instances
}

/// Parse available MIG profiles from `nvidia-smi mig -lgip -i <gpu>` output.
///
/// Example output:
/// ```text
/// +-------------------------------------------------------+
/// | GPU instance profiles:                                 |
/// | GPU   Name          ID    Instances   Memory     P2P  |
/// |=======================================================|
/// |   0  MIG 1g.5gb     19     7/7        4864MiB    No   |
/// |   0  MIG 2g.10gb    14     3/3        9856MiB    No   |
/// |   0  MIG 3g.20gb     9     2/2        19968MiB   No   |
/// |   0  MIG 4g.40gb     5     1/1        40192MiB   No   |
/// |   0  MIG 7g.80gb     0     1/1        79872MiB   No   |
/// +-------------------------------------------------------+
/// ```
fn parse_available_profiles(output: &str) -> Vec<String> {
    let mut profiles = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let inner = line.trim_matches('|').trim();
        if inner.is_empty()
            || inner.starts_with('=')
            || inner.starts_with('-')
            || inner.starts_with('+')
            || inner.contains("GPU instance profiles")
            || inner.contains("GPU   Name")
        {
            continue;
        }
        let parts: Vec<&str> = inner.split_whitespace().collect();
        if parts.len() >= 3 && parts[1] == "MIG" {
            profiles.push(parts[2].to_string());
        }
    }
    profiles
}

/// Validate a MIG profile name.
///
/// Valid profiles follow the pattern `<slices>g.<memory>gb`, e.g. "1g.5gb", "7g.80gb".
fn validate_profile(profile: &str) -> bool {
    let parts: Vec<&str> = profile.split('.').collect();
    if parts.len() != 2 {
        return false;
    }
    let slice_ok = parts[0].ends_with('g')
        && parts[0]
            .trim_end_matches('g')
            .parse::<u32>()
            .is_ok();
    let mem_ok = parts[1].ends_with("gb")
        && parts[1]
            .trim_end_matches("gb")
            .parse::<u32>()
            .is_ok();
    slice_ok && mem_ok
}

// ---- MIG Config Module ----

pub struct MigConfigModule;

impl Module for MigConfigModule {
    fn name(&self) -> &'static str {
        "mig_config"
    }

    fn description(&self) -> &'static str {
        "Configure NVIDIA Multi-Instance GPU (MIG) mode and instances"
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
        let gpu_id = params.get_string_required("gpu_id")?;
        let mig_enabled = params.get_bool_or("mig_enabled", true);
        let profiles = params.get_vec_string("profiles")?;
        let auto_create_compute = params.get_bool_or("auto_create_compute", true);

        // Validate profiles if provided
        if let Some(ref profile_list) = profiles {
            for p in profile_list {
                if !validate_profile(p) {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid MIG profile '{}'. Expected format: <N>g.<M>gb (e.g. 1g.5gb)",
                        p
                    )));
                }
            }
        }

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();
        let mut reboot_required = false;

        // Query current MIG state
        let (ok, mig_stdout, _) = run_cmd(
            connection,
            &format!(
                "nvidia-smi -i {} --query-gpu=mig.mode.current --format=csv,noheader",
                gpu_id
            ),
            context,
        )?;

        let current_mig = if ok {
            parse_mig_mode(&mig_stdout)
        } else {
            None
        };

        // -- state=absent: destroy instances and disable MIG --
        if state == "absent" {
            if current_mig == Some(true) {
                if context.check_mode {
                    return Ok(ModuleOutput::changed(
                        "Would destroy MIG instances and disable MIG mode",
                    ));
                }

                // Destroy all GPU instances
                let _ = run_cmd(
                    connection,
                    &format!("nvidia-smi mig -dgi -i {}", gpu_id),
                    context,
                );

                // Disable MIG mode
                run_cmd_ok(
                    connection,
                    &format!("nvidia-smi -i {} -mig 0", gpu_id),
                    context,
                )?;

                return Ok(
                    ModuleOutput::changed("Destroyed MIG instances and disabled MIG mode")
                        .with_data("reboot_required", serde_json::json!(true)),
                );
            }

            return Ok(ModuleOutput::ok("MIG mode is already disabled"));
        }

        // -- state=present --

        // Step 1: Enable MIG mode if needed
        if mig_enabled && current_mig != Some(true) {
            if context.check_mode {
                changes.push(format!("Would enable MIG mode on GPU {}", gpu_id));
            } else {
                run_cmd_ok(
                    connection,
                    &format!("nvidia-smi -i {} -mig 1", gpu_id),
                    context,
                )?;
                changed = true;
                reboot_required = true;
                changes.push(format!(
                    "Enabled MIG mode on GPU {} (reboot may be required)",
                    gpu_id
                ));
            }
        } else if !mig_enabled && current_mig == Some(true) {
            if context.check_mode {
                changes.push(format!("Would disable MIG mode on GPU {}", gpu_id));
            } else {
                // Destroy existing instances first
                let _ = run_cmd(
                    connection,
                    &format!("nvidia-smi mig -dgi -i {}", gpu_id),
                    context,
                );
                run_cmd_ok(
                    connection,
                    &format!("nvidia-smi -i {} -mig 0", gpu_id),
                    context,
                )?;
                changed = true;
                reboot_required = true;
                changes.push(format!(
                    "Disabled MIG mode on GPU {} (reboot may be required)",
                    gpu_id
                ));
            }
        }

        // Step 2: Create MIG instances if profiles specified and MIG enabled
        if mig_enabled {
            if let Some(ref profile_list) = profiles {
                if !profile_list.is_empty() {
                    if context.check_mode {
                        changes.push(format!(
                            "Would create MIG instances: {}",
                            profile_list.join(", ")
                        ));
                    } else {
                        // Destroy existing instances to apply fresh configuration
                        let _ = run_cmd(
                            connection,
                            &format!("nvidia-smi mig -dgi -i {}", gpu_id),
                            context,
                        );

                        // Create new instances
                        let profiles_arg = profile_list.join(",");
                        let create_cmd = if auto_create_compute {
                            format!(
                                "nvidia-smi mig -cgi {} -C -i {}",
                                profiles_arg, gpu_id
                            )
                        } else {
                            format!(
                                "nvidia-smi mig -cgi {} -i {}",
                                profiles_arg, gpu_id
                            )
                        };

                        run_cmd_ok(connection, &create_cmd, context)?;
                        changed = true;
                        changes.push(format!(
                            "Created MIG instances: {}",
                            profile_list.join(", ")
                        ));
                    }
                }
            }
        }

        // Step 3: Query final state
        let (instances, available_profiles) = if !context.check_mode && mig_enabled {
            let (ok1, gi_stdout, _) = run_cmd(
                connection,
                &format!("nvidia-smi mig -lgi -i {}", gpu_id),
                context,
            )?;
            let instances = if ok1 {
                parse_gpu_instances(&gi_stdout)
            } else {
                Vec::new()
            };

            let (ok2, gip_stdout, _) = run_cmd(
                connection,
                &format!("nvidia-smi mig -lgip -i {}", gpu_id),
                context,
            )?;
            let available = if ok2 {
                parse_available_profiles(&gip_stdout)
            } else {
                Vec::new()
            };

            (instances, available)
        } else {
            (Vec::new(), Vec::new())
        };

        let status = MigStatus {
            mig_enabled,
            instances,
            available_profiles,
            reboot_required,
        };

        // Build output
        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} MIG configuration changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!(
                "Applied {} MIG configuration changes",
                changes.len()
            ))
        } else {
            ModuleOutput::ok("MIG configuration is up to date")
        };

        output = output
            .with_data("changes", serde_json::json!(changes))
            .with_data("gpu_id", serde_json::json!(gpu_id))
            .with_data("status", serde_json::json!(status));

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["gpu_id"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("mig_enabled", serde_json::json!(true));
        m.insert("profiles", serde_json::json!(null));
        m.insert("auto_create_compute", serde_json::json!(true));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = MigConfigModule;
        assert_eq!(module.name(), "mig_config");
        assert!(!module.description().is_empty());
        assert!(module.required_params().contains(&"gpu_id"));
    }

    #[test]
    fn test_parse_mig_mode() {
        assert_eq!(parse_mig_mode("Enabled"), Some(true));
        assert_eq!(parse_mig_mode("Disabled"), Some(false));
        assert_eq!(parse_mig_mode("enabled\n"), Some(true));
        assert_eq!(parse_mig_mode("  Disabled  "), Some(false));
        assert_eq!(parse_mig_mode("[N/A]"), None);
        assert_eq!(parse_mig_mode(""), None);
    }

    #[test]
    fn test_parse_gpu_instances() {
        let output = r#"+-------------------------------------------------------+
| GPU instances:                                         |
| GPU   Name          Profile  Instance   Placement     |
|                       ID       ID       Start:Size    |
|=======================================================|
|   0  MIG 1g.5gb        19        1          0:1       |
|   0  MIG 2g.10gb       14        2          1:2       |
+-------------------------------------------------------+"#;
        let instances = parse_gpu_instances(output);
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].profile, "1g.5gb");
        assert_eq!(instances[0].gpu_instance_id, "1");
        assert_eq!(instances[0].placement, "0:1");
        assert_eq!(instances[1].profile, "2g.10gb");
        assert_eq!(instances[1].gpu_instance_id, "2");
    }

    #[test]
    fn test_parse_gpu_instances_empty() {
        let output = r#"+-------------------------------------------------------+
| GPU instances:                                         |
| GPU   Name          Profile  Instance   Placement     |
|                       ID       ID       Start:Size    |
|=======================================================|
| No GPU instances found.                                |
+-------------------------------------------------------+"#;
        let instances = parse_gpu_instances(output);
        assert!(instances.is_empty());
    }

    #[test]
    fn test_parse_available_profiles() {
        let output = r#"+-------------------------------------------------------+
| GPU instance profiles:                                 |
| GPU   Name          ID    Instances   Memory     P2P  |
|=======================================================|
|   0  MIG 1g.5gb     19     7/7        4864MiB    No   |
|   0  MIG 2g.10gb    14     3/3        9856MiB    No   |
|   0  MIG 3g.20gb     9     2/2        19968MiB   No   |
|   0  MIG 7g.80gb     0     1/1        79872MiB   No   |
+-------------------------------------------------------+"#;
        let profiles = parse_available_profiles(output);
        assert_eq!(profiles.len(), 4);
        assert_eq!(profiles[0], "1g.5gb");
        assert_eq!(profiles[1], "2g.10gb");
        assert_eq!(profiles[2], "3g.20gb");
        assert_eq!(profiles[3], "7g.80gb");
    }

    #[test]
    fn test_validate_profile() {
        assert!(validate_profile("1g.5gb"));
        assert!(validate_profile("2g.10gb"));
        assert!(validate_profile("3g.20gb"));
        assert!(validate_profile("4g.40gb"));
        assert!(validate_profile("7g.80gb"));
        assert!(!validate_profile("invalid"));
        assert!(!validate_profile("1g"));
        assert!(!validate_profile("5gb"));
        assert!(!validate_profile(""));
        assert!(!validate_profile("xg.ygb"));
    }

    #[test]
    fn test_mig_instance_serialization() {
        let instance = MigInstance {
            gpu_instance_id: "1".to_string(),
            profile: "1g.5gb".to_string(),
            placement: "0:1".to_string(),
        };
        let json = serde_json::to_value(&instance).unwrap();
        assert_eq!(json["gpu_instance_id"], "1");
        assert_eq!(json["profile"], "1g.5gb");
        assert_eq!(json["placement"], "0:1");
    }

    #[test]
    fn test_mig_status_serialization() {
        let status = MigStatus {
            mig_enabled: true,
            instances: vec![],
            available_profiles: vec!["1g.5gb".to_string(), "7g.80gb".to_string()],
            reboot_required: false,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["mig_enabled"], true);
        assert_eq!(json["reboot_required"], false);
        assert_eq!(json["available_profiles"].as_array().unwrap().len(), 2);
    }
}
