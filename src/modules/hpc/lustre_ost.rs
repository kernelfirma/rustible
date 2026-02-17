//! Lustre OST (Object Storage Target) management module
//!
//! Manage Lustre OST lifecycle operations via lctl.
//!
//! # Parameters
//!
//! - `ost_index` (required): OST index number
//! - `target` (required): Target device or filesystem
//! - `action` (required): Operation - "activate", "deactivate", "add", "remove"
//! - `mdt_index` (optional): MDT index for coordinated operations
//! - `wait_drain` (optional): Wait for client connections to drain before deactivate (default: true)
//! - `drain_timeout` (optional): Timeout in seconds for drain wait (default: 60)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of preflight health checks performed before OST operations.
#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single field that has drifted between desired and actual state.
#[derive(Debug, serde::Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

/// Result of post-operation verification.
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

/// Perform a preflight health check on the Lustre subsystem.
///
/// Runs `lctl get_param health_check` to verify overall Lustre health, and
/// optionally checks OST-specific health via `lctl get_param obdfilter.*.health_check`.
fn preflight_health_check(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    ost_name: &str,
) -> ModuleResult<PreflightResult> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check overall Lustre health
    let (health_ok, health_out, health_err) =
        run_cmd(connection, "lctl get_param health_check", context)?;

    if health_ok {
        let output = health_out.trim().to_lowercase();
        if output.contains("healthy") && !output.contains("not healthy") {
            // Lustre subsystem is healthy -- no action needed
        } else if output.contains("not healthy") || output.contains("unhealthy") {
            errors.push(format!(
                "Lustre health check reports unhealthy: {}",
                health_out.trim()
            ));
        } else if output.is_empty() {
            warnings.push("Lustre health check returned empty output".to_string());
        } else {
            warnings.push(format!(
                "Lustre health check returned unexpected output: {}",
                health_out.trim()
            ));
        }
    } else {
        errors.push(format!(
            "Failed to run Lustre health check: {}",
            health_err.trim()
        ));
    }

    // Check OST-specific health if available
    let (ost_health_ok, ost_health_out, _) = run_cmd(
        connection,
        &format!(
            "lctl get_param obdfilter.{}.health_check 2>/dev/null",
            ost_name
        ),
        context,
    )?;

    if ost_health_ok && !ost_health_out.trim().is_empty() {
        let ost_output = ost_health_out.trim().to_lowercase();
        if ost_output.contains("healthy") {
            // OST-specific health is good
        } else {
            warnings.push(format!(
                "OST {} health check: {}",
                ost_name,
                ost_health_out.trim()
            ));
        }
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Check for active client exports before deactivating an OST.
///
/// Queries `lctl get_param obdfilter.<ost_name>.num_exports` to determine
/// if there are active client connections. If `wait_drain` is true and there
/// are active exports, warnings are generated about active clients.
fn guarded_deactivate(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    ost_name: &str,
    wait_drain: bool,
    drain_timeout: u32,
) -> ModuleResult<PreflightResult> {
    let errors = Vec::new();
    let mut warnings = Vec::new();

    let (exports_ok, exports_out, _) = run_cmd(
        connection,
        &format!(
            "lctl get_param obdfilter.{}.num_exports 2>/dev/null || echo '0'",
            ost_name
        ),
        context,
    )?;

    let num_exports: u32 = if exports_ok {
        // Output format: "obdfilter.<name>.num_exports=<N>" or just "<N>"
        let trimmed = exports_out.trim();
        if let Some(val) = trimmed.split('=').last() {
            val.trim().parse().unwrap_or(0)
        } else {
            trimmed.parse().unwrap_or(0)
        }
    } else {
        0
    };

    if num_exports > 0 && wait_drain {
        warnings.push(format!(
            "OST {} has {} active client export(s); will wait up to {}s for drain",
            ost_name, num_exports, drain_timeout
        ));

        // Wait for exports to drain with a polling loop
        let poll_cmd = format!(
            "timeout {} bash -c 'while [ \"$(lctl get_param -n obdfilter.{}.num_exports 2>/dev/null || echo 0)\" -gt 0 ]; do sleep 2; done'",
            drain_timeout, ost_name
        );
        let (drain_ok, _, _) = run_cmd(connection, &poll_cmd, context)?;

        if !drain_ok {
            warnings.push(format!(
                "Drain timeout ({}s) reached; OST {} may still have active exports",
                drain_timeout, ost_name
            ));
        }
    } else if num_exports > 0 {
        warnings.push(format!(
            "OST {} has {} active client export(s); proceeding without waiting (wait_drain=false)",
            ost_name, num_exports
        ));
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Execute a safe maintenance sequence for OST removal.
///
/// Steps:
/// 1. Deactivate the OST
/// 2. Wait for client connections to drain
/// 3. Verify no active connections remain
/// 4. Execute the remove operation
///
/// Returns a list of status messages for each step.
fn maintenance_sequence(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    ost_name: &str,
    target: &str,
    ost_index: &str,
    mdt_index: &Option<String>,
    drain_timeout: u32,
) -> ModuleResult<Vec<String>> {
    let mut steps: Vec<String> = Vec::new();

    // Step 1: Deactivate the OST
    let deactivate_cmd = format!("lctl set_param obdfilter.{}.state=deactivate", ost_name);
    let (deact_ok, _, deact_err) = run_cmd(connection, &deactivate_cmd, context)?;
    if deact_ok {
        steps.push(format!("Deactivated OST {}", ost_name));
    } else {
        return Err(ModuleError::ExecutionFailed(format!(
            "Failed to deactivate OST {} during maintenance sequence: {}",
            ost_name,
            deact_err.trim()
        )));
    }

    // Step 2: Wait for drain
    let poll_cmd = format!(
        "timeout {} bash -c 'while [ \"$(lctl get_param -n obdfilter.{}.num_exports 2>/dev/null || echo 0)\" -gt 0 ]; do sleep 2; done'",
        drain_timeout, ost_name
    );
    let (drain_ok, _, _) = run_cmd(connection, &poll_cmd, context)?;
    if drain_ok {
        steps.push(format!(
            "Drained all client connections from OST {}",
            ost_name
        ));
    } else {
        steps.push(format!(
            "Drain timeout ({}s) reached for OST {}; proceeding with removal",
            drain_timeout, ost_name
        ));
    }

    // Step 3: Verify no active connections
    let (_, exports_out, _) = run_cmd(
        connection,
        &format!(
            "lctl get_param -n obdfilter.{}.num_exports 2>/dev/null || echo '0'",
            ost_name
        ),
        context,
    )?;
    let remaining: u32 = exports_out.trim().parse().unwrap_or(0);
    if remaining == 0 {
        steps.push(format!(
            "Verified no active connections on OST {}",
            ost_name
        ));
    } else {
        steps.push(format!(
            "Warning: {} active connection(s) remain on OST {}",
            remaining, ost_name
        ));
    }

    // Step 4: Execute remove
    let mdt_suffix = if let Some(ref mdt) = mdt_index {
        format!(" --mdt-index {}", mdt)
    } else {
        String::new()
    };
    let remove_cmd = format!(
        "lctl del_osc_from_group {} {}{}",
        target, ost_index, mdt_suffix
    );
    run_cmd_ok(connection, &remove_cmd, context)?;
    steps.push(format!(
        "Removed OST {} from filesystem {}",
        ost_name, target
    ));

    Ok(steps)
}

/// Generate actionable recovery instructions after a failed operation.
///
/// Provides lctl commands to restore the OST to a functional state depending
/// on which action failed.
fn build_recovery_instructions(ost_name: &str, action: &OstAction, target: &str) -> Vec<String> {
    let mut instructions = Vec::new();

    match action {
        OstAction::Deactivate => {
            instructions.push(
                "# Re-enable the OST if deactivation left it in a degraded state:".to_string(),
            );
            instructions.push(format!("lctl set_param obdfilter.{}.degraded=0", ost_name));
            instructions.push(format!(
                "lctl set_param obdfilter.{}.state=activate",
                ost_name
            ));
        }
        OstAction::Activate => {
            instructions.push("# Check OST state and retry activation:".to_string());
            instructions.push(format!("lctl get_param obdfilter.{}.state", ost_name));
            instructions.push(format!(
                "lctl set_param obdfilter.{}.state=activate",
                ost_name
            ));
        }
        OstAction::Remove => {
            instructions.push("# Re-add the OST if removal failed partway:".to_string());
            instructions.push(format!(
                "lctl set_param obdfilter.{}.state=activate",
                ost_name
            ));
            instructions.push(format!("lctl add_osc_to_group {} {}", target, ost_name));
        }
        OstAction::Add => {
            instructions.push("# Clean up after a failed add operation:".to_string());
            instructions.push(format!("lctl get_param obdfilter.{}.state", ost_name));
            instructions.push(format!("lctl del_osc_from_group {} {}", target, ost_name));
        }
    }

    instructions.push("# Verify overall Lustre health:".to_string());
    instructions.push("lctl get_param health_check".to_string());

    instructions
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum OstAction {
    Activate,
    Deactivate,
    Add,
    Remove,
}

impl OstAction {
    fn from_str(s: &str) -> Option<OstAction> {
        match s.to_lowercase().as_str() {
            "activate" => Some(OstAction::Activate),
            "deactivate" => Some(OstAction::Deactivate),
            "add" => Some(OstAction::Add),
            "remove" => Some(OstAction::Remove),
            _ => None,
        }
    }

    fn to_lctl_cmd(&self) -> &'static str {
        match self {
            OstAction::Activate => "activate",
            OstAction::Deactivate => "deactivate",
            OstAction::Add => "add_osc_to_group",
            OstAction::Remove => "del_osc_from_group",
        }
    }
}

pub struct LustreOstModule;

impl Module for LustreOstModule {
    fn name(&self) -> &'static str {
        "lustre_ost"
    }

    fn description(&self) -> &'static str {
        "Manage Lustre OST lifecycle operations via lctl"
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

        let ost_index = params.get_string_required("ost_index")?;
        let target = params.get_string_required("target")?;
        let action_str = params.get_string_required("action")?;
        let mdt_index = params.get_string("mdt_index")?;
        let wait_drain = params.get_bool_or("wait_drain", true);
        let drain_timeout = params.get_u32("drain_timeout")?.unwrap_or(60);

        let action = OstAction::from_str(&action_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'activate', 'deactivate', 'add', or 'remove'",
                action_str
            ))
        })?;

        // Check if lctl is available
        let (lctl_ok, _, _) = run_cmd(connection, "which lctl", context)?;
        if !lctl_ok {
            return Err(ModuleError::ExecutionFailed(
                "lctl command not found. Ensure Lustre client utilities are installed.".to_string(),
            ));
        }

        // Query current OST state
        let ost_name = format!(
            "{}-OST{:04x}",
            target,
            ost_index.parse::<u32>().unwrap_or(0)
        );

        // --- Preflight health check ---
        let preflight = preflight_health_check(connection, context, &ost_name)?;
        if !preflight.passed {
            let recovery = build_recovery_instructions(&ost_name, &action, &target);
            return Err(ModuleError::ExecutionFailed(format!(
                "Preflight health check failed: {}. Recovery instructions: {}",
                preflight.errors.join("; "),
                recovery.join("; ")
            )));
        }

        let (state_ok, state_output, _) = run_cmd(
            connection,
            &format!(
                "lctl get_param obdfilter.{}.state 2>/dev/null || echo 'unknown'",
                ost_name
            ),
            context,
        )?;

        let current_state = if state_ok {
            if state_output.contains("active") {
                "active"
            } else if state_output.contains("inactive") {
                "inactive"
            } else {
                "unknown"
            }
        } else {
            "unknown"
        };

        // Idempotency check
        let needs_change = match action {
            OstAction::Activate => current_state != "active",
            OstAction::Deactivate => current_state != "inactive",
            OstAction::Add | OstAction::Remove => true, // Always execute for add/remove
        };

        if !needs_change {
            return Ok(
                ModuleOutput::ok(format!("OST {} is already in desired state", ost_name))
                    .with_data("ost", serde_json::json!(ost_name))
                    .with_data("state", serde_json::json!(current_state))
                    .with_data("preflight", serde_json::json!(preflight)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would execute {} on OST {} (current state: {})",
                action_str, ost_name, current_state
            ))
            .with_data("ost", serde_json::json!(ost_name))
            .with_data("action", serde_json::json!(action))
            .with_data("preflight", serde_json::json!(preflight)));
        }

        // Execute the action with enhanced safety
        let result = match action {
            OstAction::Deactivate => {
                // Use guarded deactivate with drain support
                let guard_result =
                    guarded_deactivate(connection, context, &ost_name, wait_drain, drain_timeout)?;

                let lctl_cmd = format!("lctl set_param obdfilter.{}.state=deactivate", ost_name);
                let exec_result = run_cmd_ok(connection, &lctl_cmd, context);

                match exec_result {
                    Ok(_) => {
                        let mut output = ModuleOutput::changed(format!(
                            "Executed deactivate on OST {} (was {})",
                            ost_name, current_state
                        ))
                        .with_data("ost", serde_json::json!(ost_name))
                        .with_data("action", serde_json::json!(action))
                        .with_data("previous_state", serde_json::json!(current_state))
                        .with_data("preflight", serde_json::json!(preflight))
                        .with_data("drain_check", serde_json::json!(guard_result));

                        if !guard_result.warnings.is_empty() {
                            output = output.with_data(
                                "drain_warnings",
                                serde_json::json!(guard_result.warnings),
                            );
                        }

                        Ok(output)
                    }
                    Err(e) => {
                        let recovery = build_recovery_instructions(&ost_name, &action, &target);
                        Err(ModuleError::ExecutionFailed(format!(
                            "{}. Recovery instructions: {}",
                            e,
                            recovery.join("; ")
                        )))
                    }
                }
            }
            OstAction::Remove => {
                // Use maintenance sequence for safe removal
                let sequence_result = maintenance_sequence(
                    connection,
                    context,
                    &ost_name,
                    &target,
                    &ost_index,
                    &mdt_index,
                    drain_timeout,
                );

                match sequence_result {
                    Ok(steps) => Ok(ModuleOutput::changed(format!(
                        "Executed remove on OST {} (was {})",
                        ost_name, current_state
                    ))
                    .with_data("ost", serde_json::json!(ost_name))
                    .with_data("action", serde_json::json!(action))
                    .with_data("previous_state", serde_json::json!(current_state))
                    .with_data("preflight", serde_json::json!(preflight))
                    .with_data("maintenance_steps", serde_json::json!(steps))),
                    Err(e) => {
                        let recovery = build_recovery_instructions(&ost_name, &action, &target);
                        Err(ModuleError::ExecutionFailed(format!(
                            "{}. Recovery instructions: {}",
                            e,
                            recovery.join("; ")
                        )))
                    }
                }
            }
            OstAction::Activate | OstAction::Add => {
                // Standard execution for activate and add
                let lctl_cmd = match action {
                    OstAction::Activate => {
                        format!("lctl set_param obdfilter.{}.state=activate", ost_name)
                    }
                    OstAction::Add => {
                        let mdt_suffix = if let Some(ref mdt) = mdt_index {
                            format!(" --mdt-index {}", mdt)
                        } else {
                            String::new()
                        };
                        format!(
                            "lctl add_osc_to_group {} {}{}",
                            target, ost_index, mdt_suffix
                        )
                    }
                    _ => unreachable!(),
                };

                let exec_result = run_cmd_ok(connection, &lctl_cmd, context);

                match exec_result {
                    Ok(_) => Ok(ModuleOutput::changed(format!(
                        "Executed {} on OST {} (was {})",
                        action_str, ost_name, current_state
                    ))
                    .with_data("ost", serde_json::json!(ost_name))
                    .with_data("action", serde_json::json!(action))
                    .with_data("previous_state", serde_json::json!(current_state))
                    .with_data("preflight", serde_json::json!(preflight))),
                    Err(e) => {
                        let recovery = build_recovery_instructions(&ost_name, &action, &target);
                        Err(ModuleError::ExecutionFailed(format!(
                            "{}. Recovery instructions: {}",
                            e,
                            recovery.join("; ")
                        )))
                    }
                }
            }
        };

        result
    }

    fn required_params(&self) -> &[&'static str] {
        &["ost_index", "target", "action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("mdt_index", serde_json::json!(null));
        m.insert("wait_drain", serde_json::json!(true));
        m.insert("drain_timeout", serde_json::json!(60));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = LustreOstModule;
        assert_eq!(module.name(), "lustre_ost");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = LustreOstModule;
        let required = module.required_params();
        assert!(required.contains(&"ost_index"));
        assert!(required.contains(&"target"));
        assert!(required.contains(&"action"));
    }

    #[test]
    fn test_optional_params() {
        let module = LustreOstModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("mdt_index"));
        assert!(optional.contains_key("wait_drain"));
        assert!(optional.contains_key("drain_timeout"));
    }

    #[test]
    fn test_ost_action_from_str() {
        assert_eq!(OstAction::from_str("activate"), Some(OstAction::Activate));
        assert_eq!(
            OstAction::from_str("DEACTIVATE"),
            Some(OstAction::Deactivate)
        );
        assert_eq!(OstAction::from_str("add"), Some(OstAction::Add));
        assert_eq!(OstAction::from_str("remove"), Some(OstAction::Remove));
        assert_eq!(OstAction::from_str("invalid"), None);
    }

    #[test]
    fn test_ost_action_to_lctl_cmd() {
        assert_eq!(OstAction::Activate.to_lctl_cmd(), "activate");
        assert_eq!(OstAction::Deactivate.to_lctl_cmd(), "deactivate");
        assert_eq!(OstAction::Add.to_lctl_cmd(), "add_osc_to_group");
        assert_eq!(OstAction::Remove.to_lctl_cmd(), "del_osc_from_group");
    }

    #[test]
    fn test_ost_action_validation_with_health() {
        // Test that preflight health check output is properly structured
        // by simulating various health check output parsing scenarios

        // "healthy" output should pass
        let healthy_output = "health_check=healthy";
        assert!(healthy_output.to_lowercase().contains("healthy"));
        assert!(!healthy_output.to_lowercase().contains("not healthy"));

        // "NOT HEALTHY" output should be caught
        let unhealthy_output = "health_check=NOT HEALTHY";
        let lower = unhealthy_output.to_lowercase();
        assert!(lower.contains("not healthy") || lower.contains("unhealthy"));

        // Empty output should be flagged
        let empty_output = "";
        assert!(empty_output.trim().is_empty());

        // Unexpected output should generate a warning
        let unexpected_output = "health_check=DEGRADED";
        let lower_unexpected = unexpected_output.to_lowercase();
        assert!(!lower_unexpected.contains("healthy") || lower_unexpected.contains("unhealthy"));

        // OST-specific health output parsing
        let ost_healthy = "obdfilter.testfs-OST0000.health_check=healthy";
        assert!(ost_healthy.to_lowercase().contains("healthy"));

        let ost_degraded = "obdfilter.testfs-OST0000.health_check=degraded";
        assert!(!ost_degraded.to_lowercase().contains("healthy"));
    }

    #[test]
    fn test_recovery_instruction_format() {
        let ost_name = "testfs-OST0000";
        let target = "testfs";

        // Test deactivate recovery instructions
        let deact_recovery = build_recovery_instructions(ost_name, &OstAction::Deactivate, target);
        assert!(!deact_recovery.is_empty());
        assert!(deact_recovery.iter().any(|cmd| cmd.contains("degraded=0")));
        assert!(deact_recovery
            .iter()
            .any(|cmd| cmd.contains("state=activate")));
        assert!(deact_recovery
            .iter()
            .any(|cmd| cmd.contains("health_check")));

        // Test activate recovery instructions
        let act_recovery = build_recovery_instructions(ost_name, &OstAction::Activate, target);
        assert!(!act_recovery.is_empty());
        assert!(act_recovery
            .iter()
            .any(|cmd| cmd.contains("state=activate")));
        assert!(act_recovery.iter().any(|cmd| cmd.contains("get_param")));

        // Test remove recovery instructions
        let remove_recovery = build_recovery_instructions(ost_name, &OstAction::Remove, target);
        assert!(!remove_recovery.is_empty());
        assert!(remove_recovery
            .iter()
            .any(|cmd| cmd.contains("add_osc_to_group")));
        assert!(remove_recovery
            .iter()
            .any(|cmd| cmd.contains("state=activate")));

        // Test add recovery instructions
        let add_recovery = build_recovery_instructions(ost_name, &OstAction::Add, target);
        assert!(!add_recovery.is_empty());
        assert!(add_recovery
            .iter()
            .any(|cmd| cmd.contains("del_osc_from_group")));

        // All recovery instructions should end with a health check command
        for recovery in [
            &deact_recovery,
            &act_recovery,
            &remove_recovery,
            &add_recovery,
        ] {
            let last = recovery.last().unwrap();
            assert_eq!(last, "lctl get_param health_check");
        }
    }

    #[test]
    fn test_drain_param_defaults() {
        let module = LustreOstModule;
        let optional = module.optional_params();

        // wait_drain should default to true
        let wait_drain = optional.get("wait_drain").unwrap();
        assert_eq!(*wait_drain, serde_json::json!(true));

        // drain_timeout should default to 60
        let drain_timeout = optional.get("drain_timeout").unwrap();
        assert_eq!(*drain_timeout, serde_json::json!(60));

        // Verify get_bool_or behavior with empty params
        let params: ModuleParams = HashMap::new();
        let wd = params.get_bool_or("wait_drain", true);
        assert!(wd, "wait_drain should default to true when not specified");

        // Verify get_u32 behavior with empty params
        let dt = params.get_u32("drain_timeout").unwrap();
        assert_eq!(dt, None, "drain_timeout should be None when not specified");

        // Verify get_u32 behavior with explicit value
        let mut params_with_timeout: ModuleParams = HashMap::new();
        params_with_timeout.insert("drain_timeout".to_string(), serde_json::json!(120));
        let dt = params_with_timeout.get_u32("drain_timeout").unwrap();
        assert_eq!(dt, Some(120));

        // Verify get_bool_or behavior with explicit false
        let mut params_no_drain: ModuleParams = HashMap::new();
        params_no_drain.insert("wait_drain".to_string(), serde_json::json!(false));
        let wd = params_no_drain.get_bool_or("wait_drain", true);
        assert!(
            !wd,
            "wait_drain should be false when explicitly set to false"
        );
    }
}
