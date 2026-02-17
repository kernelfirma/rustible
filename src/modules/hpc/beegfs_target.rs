//! BeeGFS storage/metadata target management
//!
//! Manages BeeGFS target lifecycle including creation, removal,
//! start, stop, and status operations.
//!
//! # Parameters
//!
//! - `target_id` (required): BeeGFS target numeric ID
//! - `action` (required): "create", "remove", "start", "stop", "status"
//! - `target_type` (optional): "storage" or "meta" (default: "storage")
//! - `mgmtd_host` (optional): Management daemon hostname
//! - `storage_path` (optional): Path for storage target data
//! - `state` (optional): "present" or "absent" (default: "present")

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of preflight health checks performed before BeeGFS target operations.
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

/// Valid BeeGFS target actions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum TargetAction {
    Create,
    Remove,
    Start,
    Stop,
    Status,
}

impl TargetAction {
    fn from_str(s: &str) -> Option<TargetAction> {
        match s.to_lowercase().as_str() {
            "create" => Some(TargetAction::Create),
            "remove" => Some(TargetAction::Remove),
            "start" => Some(TargetAction::Start),
            "stop" => Some(TargetAction::Stop),
            "status" => Some(TargetAction::Status),
            _ => None,
        }
    }
}

/// Valid BeeGFS target types.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum TargetType {
    Storage,
    Meta,
}

impl TargetType {
    fn from_str(s: &str) -> Option<TargetType> {
        match s.to_lowercase().as_str() {
            "storage" => Some(TargetType::Storage),
            "meta" => Some(TargetType::Meta),
            _ => None,
        }
    }

    fn as_nodetype(&self) -> &'static str {
        match self {
            TargetType::Storage => "storage",
            TargetType::Meta => "meta",
        }
    }

    fn as_service_name(&self) -> &'static str {
        match self {
            TargetType::Storage => "beegfs-storage",
            TargetType::Meta => "beegfs-meta",
        }
    }
}

/// Detected state of a BeeGFS target on the system.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum DetectedState {
    /// Target exists and is reachable/online
    Online,
    /// Target exists but is offline
    Offline,
    /// Target is not found in the cluster
    Absent,
    /// Unable to determine target state
    Unknown,
}

/// Perform a preflight health check before BeeGFS target operations.
///
/// Verifies that `beegfs-ctl` is installed and optionally checks that the
/// management daemon is reachable.
fn preflight_check(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    mgmtd_host: &Option<String>,
) -> ModuleResult<PreflightResult> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check that beegfs-ctl is available
    let (ctl_ok, _, ctl_err) = run_cmd(connection, "which beegfs-ctl", context)?;
    if !ctl_ok {
        errors.push(format!(
            "beegfs-ctl command not found. Ensure BeeGFS utils are installed: {}",
            ctl_err.trim()
        ));
    }

    // If a management host is specified, verify connectivity
    if let Some(ref host) = mgmtd_host {
        let mgmtd_cmd = format!(
            "beegfs-ctl --listnodes --nodetype=mgmt --cfgFile=/dev/null --sysMgmtdHost={} 2>&1 || true",
            host
        );
        let (mgmtd_ok, mgmtd_out, _) = run_cmd(connection, &mgmtd_cmd, context)?;

        if !mgmtd_ok
            || mgmtd_out.to_lowercase().contains("error")
            || mgmtd_out.to_lowercase().contains("communication error")
        {
            warnings.push(format!(
                "Management daemon at '{}' may not be reachable: {}",
                host,
                mgmtd_out.trim()
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

/// Detect the current state of a BeeGFS target by parsing `beegfs-ctl --listtargets` output.
///
/// Runs `beegfs-ctl --listtargets --nodetype=<type>` and searches for
/// the given target ID in the output. Returns the detected state.
fn detect_target_state(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    target_id: &str,
    target_type: &TargetType,
    mgmtd_host: &Option<String>,
) -> ModuleResult<(DetectedState, Vec<DriftItem>)> {
    let mgmtd_flag = mgmtd_host
        .as_ref()
        .map(|h| format!(" --sysMgmtdHost={}", h))
        .unwrap_or_default();

    let list_cmd = format!(
        "beegfs-ctl --listtargets --nodetype={}{} --state 2>/dev/null || echo 'BEEGFS_LIST_FAILED'",
        target_type.as_nodetype(),
        mgmtd_flag
    );
    let (list_ok, list_out, _) = run_cmd(connection, &list_cmd, context)?;

    if !list_ok || list_out.contains("BEEGFS_LIST_FAILED") {
        return Ok((DetectedState::Unknown, Vec::new()));
    }

    let drift = Vec::new();
    let state = parse_target_state(&list_out, target_id);

    Ok((state, drift))
}

/// Parse the `beegfs-ctl --listtargets --state` output to find a target by ID.
///
/// Typical output format:
/// ```text
/// TargetID   NodeID   State
/// ========   ======   =====
///        1     node1   Good
///        2     node2   Good
/// ```
///
/// Or with `--state`:
/// ```text
/// TargetID     Reachability  Consistency   NodeID
/// ========     ============  ===========   ======
///        1     Online        Good          node1
///        2     Offline       Needs-resync  node2
/// ```
fn parse_target_state(output: &str, target_id: &str) -> DetectedState {
    for line in output.lines() {
        let trimmed = line.trim();
        // Skip header lines and separator lines
        if trimmed.is_empty()
            || trimmed.starts_with("TargetID")
            || trimmed.starts_with("========")
            || trimmed.starts_with("------")
        {
            continue;
        }

        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }

        // First field is the target ID
        if fields[0] == target_id {
            // Look for state/reachability indicators in remaining fields
            let line_lower = trimmed.to_lowercase();
            if line_lower.contains("online") || line_lower.contains("good") {
                return DetectedState::Online;
            } else if line_lower.contains("offline")
                || line_lower.contains("unreachable")
                || line_lower.contains("needs-resync")
            {
                return DetectedState::Offline;
            } else {
                // Target found but state is unclear -- treat as online
                return DetectedState::Online;
            }
        }
    }

    DetectedState::Absent
}

/// Execute the requested BeeGFS target action.
fn apply_action(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    action: &TargetAction,
    target_id: &str,
    target_type: &TargetType,
    mgmtd_host: &Option<String>,
    storage_path: &Option<String>,
) -> ModuleResult<String> {
    let mgmtd_flag = mgmtd_host
        .as_ref()
        .map(|h| format!(" --sysMgmtdHost={}", h))
        .unwrap_or_default();

    match action {
        TargetAction::Create => {
            let path = storage_path.as_deref().unwrap_or("/data/beegfs");
            let create_cmd = match target_type {
                TargetType::Storage => format!(
                    "beegfs-ctl --addstoragepool --targets={}{} && mkdir -p {} && chown beegfs:beegfs {} 2>/dev/null; \
                     beegfs-ctl --addtarget --storagepath={} --targetid={}{}",
                    target_id, mgmtd_flag, path, path, path, target_id, mgmtd_flag
                ),
                TargetType::Meta => format!(
                    "mkdir -p {} && chown beegfs:beegfs {} 2>/dev/null; \
                     beegfs-ctl --addtarget --storagepath={} --targetid={} --nodetype=meta{}",
                    path, path, path, target_id, mgmtd_flag
                ),
            };
            run_cmd_ok(connection, &create_cmd, context)?;
            Ok(format!(
                "Created {} target {} at {}",
                target_type.as_nodetype(),
                target_id,
                path
            ))
        }
        TargetAction::Remove => {
            let remove_cmd = format!(
                "beegfs-ctl --removetarget --targetid={} --nodetype={}{}",
                target_id,
                target_type.as_nodetype(),
                mgmtd_flag
            );
            run_cmd_ok(connection, &remove_cmd, context)?;
            Ok(format!(
                "Removed {} target {}",
                target_type.as_nodetype(),
                target_id
            ))
        }
        TargetAction::Start => {
            let start_cmd = format!("systemctl start {}", target_type.as_service_name());
            run_cmd_ok(connection, &start_cmd, context)?;
            Ok(format!(
                "Started {} service for target {}",
                target_type.as_service_name(),
                target_id
            ))
        }
        TargetAction::Stop => {
            let stop_cmd = format!("systemctl stop {}", target_type.as_service_name());
            run_cmd_ok(connection, &stop_cmd, context)?;
            Ok(format!(
                "Stopped {} service for target {}",
                target_type.as_service_name(),
                target_id
            ))
        }
        TargetAction::Status => {
            // Status is read-only; return current state information
            let status_cmd = format!(
                "beegfs-ctl --listtargets --nodetype={}{} --state",
                target_type.as_nodetype(),
                mgmtd_flag
            );
            let output = run_cmd_ok(connection, &status_cmd, context)?;
            Ok(output)
        }
    }
}

/// Post-operation verification: re-check target state after action.
fn post_verify(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    target_id: &str,
    target_type: &TargetType,
    action: &TargetAction,
    mgmtd_host: &Option<String>,
) -> ModuleResult<VerifyResult> {
    let mut details = Vec::new();
    let mut warnings = Vec::new();

    let (post_state, _) =
        detect_target_state(connection, context, target_id, target_type, mgmtd_host)?;

    let verified = match action {
        TargetAction::Create | TargetAction::Start => {
            if post_state == DetectedState::Online {
                details.push(format!(
                    "Target {} is online after {}",
                    target_id,
                    serde_json::to_string(action).unwrap_or_else(|_| "action".to_string())
                ));
                true
            } else {
                warnings.push(format!(
                    "Target {} is {:?} after create/start; expected Online",
                    target_id, post_state
                ));
                false
            }
        }
        TargetAction::Remove => {
            if post_state == DetectedState::Absent {
                details.push(format!(
                    "Target {} confirmed absent after removal",
                    target_id
                ));
                true
            } else {
                warnings.push(format!(
                    "Target {} is {:?} after removal; expected Absent",
                    target_id, post_state
                ));
                false
            }
        }
        TargetAction::Stop => {
            if post_state == DetectedState::Offline || post_state == DetectedState::Absent {
                details.push(format!(
                    "Target {} is {:?} after stop",
                    target_id, post_state
                ));
                true
            } else {
                warnings.push(format!(
                    "Target {} is {:?} after stop; expected Offline",
                    target_id, post_state
                ));
                false
            }
        }
        TargetAction::Status => {
            details.push(format!(
                "Target {} current state: {:?}",
                target_id, post_state
            ));
            true
        }
    };

    Ok(VerifyResult {
        verified,
        details,
        warnings,
    })
}

/// Collect health telemetry from BeeGFS targets.
///
/// Gathers `beegfs-ctl --listtargets --state` output for operational visibility.
fn health_telemetry(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    target_type: &TargetType,
    mgmtd_host: &Option<String>,
) -> ModuleResult<serde_json::Value> {
    let mgmtd_flag = mgmtd_host
        .as_ref()
        .map(|h| format!(" --sysMgmtdHost={}", h))
        .unwrap_or_default();

    let telemetry_cmd = format!(
        "beegfs-ctl --listtargets --nodetype={}{} --state --longnodes 2>/dev/null || echo ''",
        target_type.as_nodetype(),
        mgmtd_flag,
    );
    let (_, telemetry_out, _) = run_cmd(connection, &telemetry_cmd, context)?;

    // Parse targets into structured data
    let mut targets = Vec::new();
    for line in telemetry_out.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("TargetID")
            || trimmed.starts_with("========")
            || trimmed.starts_with("------")
        {
            continue;
        }
        let fields: Vec<&str> = trimmed.split_whitespace().collect();
        if fields.len() >= 2 {
            let mut entry = serde_json::Map::new();
            entry.insert("target_id".to_string(), serde_json::json!(fields[0]));
            if fields.len() >= 3 {
                entry.insert("reachability".to_string(), serde_json::json!(fields[1]));
                entry.insert("consistency".to_string(), serde_json::json!(fields[2]));
            }
            if fields.len() >= 4 {
                entry.insert("node_id".to_string(), serde_json::json!(fields[3]));
            }
            targets.push(serde_json::Value::Object(entry));
        }
    }

    Ok(serde_json::json!({
        "raw_output": telemetry_out.trim(),
        "targets": targets,
        "target_count": targets.len(),
    }))
}

/// Build recovery instructions after a failed BeeGFS target operation.
fn build_recovery_instructions(
    target_id: &str,
    action: &TargetAction,
    target_type: &TargetType,
) -> Vec<String> {
    let mut instructions = Vec::new();

    match action {
        TargetAction::Create => {
            instructions.push(format!(
                "# Check if target {} was partially created:",
                target_id
            ));
            instructions.push(format!(
                "beegfs-ctl --listtargets --nodetype={}",
                target_type.as_nodetype()
            ));
            instructions.push(format!(
                "# Remove partial target if needed: beegfs-ctl --removetarget --targetid={} --nodetype={}",
                target_id,
                target_type.as_nodetype()
            ));
        }
        TargetAction::Remove => {
            instructions.push(format!("# Verify target {} was removed:", target_id));
            instructions.push(format!(
                "beegfs-ctl --listtargets --nodetype={}",
                target_type.as_nodetype()
            ));
        }
        TargetAction::Start => {
            instructions.push(format!(
                "# Check service status: systemctl status {}",
                target_type.as_service_name()
            ));
            instructions.push(format!(
                "# Check logs: journalctl -u {} --no-pager -n 50",
                target_type.as_service_name()
            ));
        }
        TargetAction::Stop => {
            instructions.push(format!(
                "# Force stop if needed: systemctl kill {}",
                target_type.as_service_name()
            ));
            instructions.push(format!(
                "# Check status: systemctl status {}",
                target_type.as_service_name()
            ));
        }
        TargetAction::Status => {
            instructions.push("# Status is read-only; no recovery needed".to_string());
        }
    }

    instructions.push("# Verify BeeGFS cluster health:".to_string());
    instructions.push("beegfs-ctl --listnodes --nodetype=mgmt".to_string());

    instructions
}

pub struct BeegfsTargetModule;

impl Module for BeegfsTargetModule {
    fn name(&self) -> &'static str {
        "beegfs_target"
    }

    fn description(&self) -> &'static str {
        "Manage BeeGFS storage/metadata target lifecycle (create, remove, start, stop, status)"
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

        // Parse required parameters
        let target_id = params.get_string_required("target_id")?;
        let action_str = params.get_string_required("action")?;

        // Parse optional parameters
        let target_type_str = params
            .get_string("target_type")?
            .unwrap_or_else(|| "storage".to_string());
        let mgmtd_host = params.get_string("mgmtd_host")?;
        let storage_path = params.get_string("storage_path")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        // Validate action
        let action = TargetAction::from_str(&action_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'create', 'remove', 'start', 'stop', or 'status'",
                action_str
            ))
        })?;

        // Validate target type
        let target_type = TargetType::from_str(&target_type_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid target_type '{}'. Must be 'storage' or 'meta'",
                target_type_str
            ))
        })?;

        // Validate state parameter
        if state != "present" && state != "absent" {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'present' or 'absent'",
                state
            )));
        }

        // --- Preflight check ---
        let preflight = preflight_check(connection, context, &mgmtd_host)?;
        if !preflight.passed {
            let recovery = build_recovery_instructions(&target_id, &action, &target_type);
            return Err(ModuleError::ExecutionFailed(format!(
                "Preflight check failed: {}. Recovery: {}",
                preflight.errors.join("; "),
                recovery.join("; ")
            )));
        }

        // --- Detect current target state ---
        let (current_state, drift) =
            detect_target_state(connection, context, &target_id, &target_type, &mgmtd_host)?;

        // Idempotency: determine if action is needed
        let needs_change = match (&action, &current_state, state.as_str()) {
            // Status never changes anything
            (TargetAction::Status, _, _) => false,
            // Create is idempotent if target already present and state=present
            (TargetAction::Create, DetectedState::Online, "present") => false,
            // Remove is idempotent if target already absent
            (TargetAction::Remove, DetectedState::Absent, _) => false,
            // Start is idempotent if already online
            (TargetAction::Start, DetectedState::Online, _) => false,
            // Stop is idempotent if already offline or absent
            (TargetAction::Stop, DetectedState::Offline, _) => false,
            (TargetAction::Stop, DetectedState::Absent, _) => false,
            // All other cases need action
            _ => true,
        };

        // Handle status action as a special read-only case
        if action == TargetAction::Status {
            let telemetry = health_telemetry(connection, context, &target_type, &mgmtd_host)?;
            return Ok(ModuleOutput::ok(format!(
                "BeeGFS {} target {} status retrieved",
                target_type.as_nodetype(),
                target_id
            ))
            .with_data("target_id", serde_json::json!(target_id))
            .with_data("target_type", serde_json::json!(target_type))
            .with_data("current_state", serde_json::json!(current_state))
            .with_data("preflight", serde_json::json!(preflight))
            .with_data("telemetry", telemetry));
        }

        if !needs_change {
            return Ok(ModuleOutput::ok(format!(
                "BeeGFS {} target {} is already in desired state ({:?})",
                target_type.as_nodetype(),
                target_id,
                current_state
            ))
            .with_data("target_id", serde_json::json!(target_id))
            .with_data("target_type", serde_json::json!(target_type))
            .with_data("current_state", serde_json::json!(current_state))
            .with_data("preflight", serde_json::json!(preflight))
            .with_data("drift", serde_json::json!(drift)));
        }

        // --- Check mode ---
        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would execute {} on BeeGFS {} target {} (current state: {:?})",
                action_str,
                target_type.as_nodetype(),
                target_id,
                current_state
            ))
            .with_data("target_id", serde_json::json!(target_id))
            .with_data("target_type", serde_json::json!(target_type))
            .with_data("action", serde_json::json!(action))
            .with_data("current_state", serde_json::json!(current_state))
            .with_data("preflight", serde_json::json!(preflight)));
        }

        // --- Apply the action ---
        let apply_result = apply_action(
            connection,
            context,
            &action,
            &target_id,
            &target_type,
            &mgmtd_host,
            &storage_path,
        );

        match apply_result {
            Ok(action_msg) => {
                // --- Post-verification ---
                let verify = post_verify(
                    connection,
                    context,
                    &target_id,
                    &target_type,
                    &action,
                    &mgmtd_host,
                )?;

                // --- Health telemetry ---
                let telemetry = health_telemetry(connection, context, &target_type, &mgmtd_host)?;

                let mut output = ModuleOutput::changed(format!(
                    "Executed {} on BeeGFS {} target {} (was {:?}): {}",
                    action_str,
                    target_type.as_nodetype(),
                    target_id,
                    current_state,
                    action_msg,
                ))
                .with_data("target_id", serde_json::json!(target_id))
                .with_data("target_type", serde_json::json!(target_type))
                .with_data("action", serde_json::json!(action))
                .with_data("previous_state", serde_json::json!(current_state))
                .with_data("preflight", serde_json::json!(preflight))
                .with_data("verify", serde_json::json!(verify))
                .with_data("telemetry", telemetry);

                if !verify.warnings.is_empty() {
                    output =
                        output.with_data("verify_warnings", serde_json::json!(verify.warnings));
                }

                Ok(output)
            }
            Err(e) => {
                let recovery = build_recovery_instructions(&target_id, &action, &target_type);
                Err(ModuleError::ExecutionFailed(format!(
                    "{}. Recovery instructions: {}",
                    e,
                    recovery.join("; ")
                )))
            }
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["target_id", "action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("target_type", serde_json::json!("storage"));
        m.insert("mgmtd_host", serde_json::json!(null));
        m.insert("storage_path", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = BeegfsTargetModule;
        assert_eq!(module.name(), "beegfs_target");
        assert!(!module.description().is_empty());
        assert!(module.description().contains("BeeGFS"));

        let required = module.required_params();
        assert!(required.contains(&"target_id"));
        assert!(required.contains(&"action"));
        assert_eq!(required.len(), 2);

        let optional = module.optional_params();
        assert!(optional.contains_key("target_type"));
        assert!(optional.contains_key("mgmtd_host"));
        assert!(optional.contains_key("storage_path"));
        assert!(optional.contains_key("state"));
        assert_eq!(
            *optional.get("target_type").unwrap(),
            serde_json::json!("storage")
        );
        assert_eq!(
            *optional.get("state").unwrap(),
            serde_json::json!("present")
        );
    }

    #[test]
    fn test_target_type_validation() {
        // Valid target types
        assert_eq!(TargetType::from_str("storage"), Some(TargetType::Storage));
        assert_eq!(TargetType::from_str("STORAGE"), Some(TargetType::Storage));
        assert_eq!(TargetType::from_str("meta"), Some(TargetType::Meta));
        assert_eq!(TargetType::from_str("META"), Some(TargetType::Meta));
        assert_eq!(TargetType::from_str("Meta"), Some(TargetType::Meta));

        // Invalid target types
        assert_eq!(TargetType::from_str("object"), None);
        assert_eq!(TargetType::from_str("invalid"), None);
        assert_eq!(TargetType::from_str(""), None);
        assert_eq!(TargetType::from_str("storagex"), None);

        // Verify nodetype mapping
        assert_eq!(TargetType::Storage.as_nodetype(), "storage");
        assert_eq!(TargetType::Meta.as_nodetype(), "meta");

        // Verify service name mapping
        assert_eq!(TargetType::Storage.as_service_name(), "beegfs-storage");
        assert_eq!(TargetType::Meta.as_service_name(), "beegfs-meta");
    }

    #[test]
    fn test_action_validation() {
        // Valid actions
        assert_eq!(TargetAction::from_str("create"), Some(TargetAction::Create));
        assert_eq!(TargetAction::from_str("CREATE"), Some(TargetAction::Create));
        assert_eq!(TargetAction::from_str("remove"), Some(TargetAction::Remove));
        assert_eq!(TargetAction::from_str("start"), Some(TargetAction::Start));
        assert_eq!(TargetAction::from_str("stop"), Some(TargetAction::Stop));
        assert_eq!(TargetAction::from_str("status"), Some(TargetAction::Status));
        assert_eq!(TargetAction::from_str("Status"), Some(TargetAction::Status));

        // Invalid actions
        assert_eq!(TargetAction::from_str("restart"), None);
        assert_eq!(TargetAction::from_str("invalid"), None);
        assert_eq!(TargetAction::from_str(""), None);
        assert_eq!(TargetAction::from_str("activate"), None);
        assert_eq!(TargetAction::from_str("deactivate"), None);
    }

    #[test]
    fn test_target_state_parsing() {
        // Typical listtargets --state output
        let output = "\
TargetID     Reachability  Consistency   NodeID
========     ============  ===========   ======
       1     Online        Good          node1
       2     Online        Good          node2
       3     Offline       Needs-resync  node3";

        assert_eq!(parse_target_state(output, "1"), DetectedState::Online);
        assert_eq!(parse_target_state(output, "2"), DetectedState::Online);
        assert_eq!(parse_target_state(output, "3"), DetectedState::Offline);
        assert_eq!(parse_target_state(output, "99"), DetectedState::Absent);

        // Empty output
        assert_eq!(parse_target_state("", "1"), DetectedState::Absent);

        // Output with only headers
        let headers_only = "\
TargetID     Reachability  Consistency   NodeID
========     ============  ===========   ======";
        assert_eq!(parse_target_state(headers_only, "1"), DetectedState::Absent);

        // Output with unreachable target
        let unreachable_output = "\
TargetID     Reachability  Consistency   NodeID
========     ============  ===========   ======
       5     Unreachable   Unknown       node5";
        assert_eq!(
            parse_target_state(unreachable_output, "5"),
            DetectedState::Offline
        );

        // Simple format without --state
        let simple_output = "\
TargetID   NodeID
========   ======
       1   node1
       2   node2";
        // Target found but no reachability keywords -- defaults to Online
        assert_eq!(
            parse_target_state(simple_output, "1"),
            DetectedState::Online
        );
        assert_eq!(
            parse_target_state(simple_output, "2"),
            DetectedState::Online
        );
        assert_eq!(
            parse_target_state(simple_output, "3"),
            DetectedState::Absent
        );
    }

    #[test]
    fn test_recovery_instructions() {
        let instructions =
            build_recovery_instructions("42", &TargetAction::Create, &TargetType::Storage);
        assert!(!instructions.is_empty());
        assert!(instructions.iter().any(|i| i.contains("listtargets")));
        assert!(instructions
            .iter()
            .any(|i| i.contains("listnodes") || i.contains("health")));

        let stop_instructions =
            build_recovery_instructions("42", &TargetAction::Stop, &TargetType::Meta);
        assert!(stop_instructions.iter().any(|i| i.contains("beegfs-meta")));

        let status_instructions =
            build_recovery_instructions("42", &TargetAction::Status, &TargetType::Storage);
        assert!(status_instructions.iter().any(|i| i.contains("read-only")));
    }

    #[test]
    fn test_parallelization_hint() {
        let module = BeegfsTargetModule;
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }

    #[test]
    fn test_detected_state_serialization() {
        // Verify DetectedState serializes correctly for JSON output
        assert_eq!(
            serde_json::to_string(&DetectedState::Online).unwrap(),
            "\"online\""
        );
        assert_eq!(
            serde_json::to_string(&DetectedState::Offline).unwrap(),
            "\"offline\""
        );
        assert_eq!(
            serde_json::to_string(&DetectedState::Absent).unwrap(),
            "\"absent\""
        );
        assert_eq!(
            serde_json::to_string(&DetectedState::Unknown).unwrap(),
            "\"unknown\""
        );
    }
}
