//! Slurm partition management module
//!
//! Manage Slurm partitions via scontrol.
//!
//! # Parameters
//!
//! - `name` (required): Partition name
//! - `state` (optional, default "present"): "present" or "absent"
//! - `nodes` (optional): Comma-separated list of nodes
//! - `max_time` (optional): Maximum wall time (e.g., "7-00:00:00")
//! - `default` (optional, default false): Set as default partition
//! - `priority_tier` (optional): Priority tier value
//! - `properties` (optional): Map of additional key=value properties
//! - `staged` (optional, default false): If true, only compute drift without applying changes
//! - `validate` (optional, default true): Run property validation before applying

use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of preflight property validation.
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

/// Post-change verification result.
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

pub struct SlurmPartitionModule;

impl Module for SlurmPartitionModule {
    fn name(&self) -> &'static str {
        "slurm_partition"
    }

    fn description(&self) -> &'static str {
        "Manage Slurm partitions (scontrol)"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::GlobalExclusive
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

        let name = params.get_string_required("name")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        match state.as_str() {
            "present" => self.ensure_present(connection, &name, params, context),
            "absent" => self.ensure_absent(connection, &name, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'present' or 'absent'",
                state
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("nodes", serde_json::json!(null));
        m.insert("max_time", serde_json::json!(null));
        m.insert("default", serde_json::json!(false));
        m.insert("priority_tier", serde_json::json!(null));
        m.insert("properties", serde_json::json!(null));
        m.insert("staged", serde_json::json!(false));
        m.insert("validate", serde_json::json!(true));
        m
    }
}

impl SlurmPartitionModule {
    /// Check if a partition exists by querying scontrol.
    fn partition_exists(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("scontrol show partition {}", name),
            context,
        )?;
        // scontrol returns success and output if partition exists,
        // failure or empty output if not
        Ok(ok && !stdout.trim().is_empty() && !stdout.contains("not found"))
    }

    fn ensure_present(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let staged = params.get_bool_or("staged", false);
        let do_validate = params.get_bool_or("validate", true);

        // Run property validation if enabled
        if do_validate {
            let preflight = validate_partition_properties(params)?;
            if !preflight.passed {
                return Err(ModuleError::InvalidParameter(format!(
                    "Property validation failed: {}",
                    preflight.errors.join("; ")
                )));
            }
        }

        let exists = self.partition_exists(connection, name, context)?;

        if exists {
            // Partition exists - use drift comparison to determine what changed
            let current_props = get_partition_properties(connection, name, context)?;
            let desired = build_desired_properties(params)?;
            let drift = compute_partition_drift(&desired, &current_props);

            if drift.is_empty() {
                return Ok(
                    ModuleOutput::ok(format!("Partition '{}' already up to date", name))
                        .with_data("partition", serde_json::json!(name))
                        .with_data("drift", serde_json::json!([])),
                );
            }

            // In staged mode, report drift without applying
            if staged {
                return Ok(ModuleOutput::ok(format!(
                    "Partition '{}' has {} drifted properties (staged mode)",
                    name,
                    drift.len()
                ))
                .with_data("partition", serde_json::json!(name))
                .with_data("drift", serde_json::json!(drift))
                .with_data("staged", serde_json::json!(true)));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would update partition '{}' ({} properties changed)",
                    name,
                    drift.len()
                ))
                .with_data("partition", serde_json::json!(name))
                .with_data("drift", serde_json::json!(drift)));
            }

            // Build update command with only the drifted properties
            let update_props: Vec<String> = drift
                .iter()
                .map(|d| format!("{}={}", d.field, d.desired))
                .collect();
            let cmd = format!(
                "scontrol update PartitionName={} {}",
                name,
                update_props.join(" ")
            );
            run_cmd_ok(connection, &cmd, context)?;

            Ok(
                ModuleOutput::changed(format!("Updated partition '{}'", name))
                    .with_data("partition", serde_json::json!(name))
                    .with_data("drift", serde_json::json!(drift)),
            )
        } else {
            // Partition doesn't exist - create it
            if staged {
                return Ok(ModuleOutput::ok(format!(
                    "Partition '{}' does not exist (staged mode, would create)",
                    name
                ))
                .with_data("partition", serde_json::json!(name))
                .with_data("staged", serde_json::json!(true)));
            }

            if context.check_mode {
                return Ok(
                    ModuleOutput::changed(format!("Would create partition '{}'", name))
                        .with_data("partition", serde_json::json!(name)),
                );
            }

            let props = build_partition_properties(params)?;
            let cmd = format!("scontrol create PartitionName={} {}", name, props);
            run_cmd_ok(connection, &cmd, context)?;

            Ok(
                ModuleOutput::changed(format!("Created partition '{}'", name))
                    .with_data("partition", serde_json::json!(name)),
            )
        }
    }

    fn ensure_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Idempotency check
        if !self.partition_exists(connection, name, context)? {
            return Ok(
                ModuleOutput::ok(format!("Partition '{}' does not exist", name))
                    .with_data("partition", serde_json::json!(name)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would delete partition '{}'", name))
                    .with_data("partition", serde_json::json!(name)),
            );
        }

        let cmd = format!("scontrol delete PartitionName={}", name);
        run_cmd_ok(connection, &cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Deleted partition '{}'", name))
                .with_data("partition", serde_json::json!(name)),
        )
    }
}

/// Parse `scontrol show partition <name>` output into a HashMap of key=value pairs.
///
/// The scontrol output format uses key=value pairs separated by whitespace,
/// potentially spanning multiple lines. Some values may contain spaces when
/// enclosed in parentheses or brackets.
fn get_partition_properties(
    connection: &Arc<dyn Connection + Send + Sync>,
    name: &str,
    context: &ModuleContext,
) -> ModuleResult<HashMap<String, String>> {
    let stdout = run_cmd_ok(
        connection,
        &format!("scontrol show partition {}", name),
        context,
    )?;
    Ok(parse_scontrol_partition_output(&stdout))
}

/// Parse raw scontrol partition output into a property map.
fn parse_scontrol_partition_output(output: &str) -> HashMap<String, String> {
    let mut props = HashMap::new();
    // scontrol output is key=value pairs separated by whitespace across lines.
    // Values themselves do not contain unquoted spaces for partition output,
    // but we handle it defensively by splitting on key=value boundaries.
    //
    // Example output:
    //   PartitionName=batch
    //      AllowGroups=ALL AllowAccounts=ALL AllowQos=ALL
    //      Default=NO MaxTime=7-00:00:00 State=UP
    //      Nodes=node[01-10] PriorityTier=1
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Split each line into tokens by whitespace, then parse key=value
        for token in trimmed.split_whitespace() {
            if let Some(eq_pos) = token.find('=') {
                let key = &token[..eq_pos];
                let value = &token[eq_pos + 1..];
                if !key.is_empty() {
                    props.insert(key.to_string(), value.to_string());
                }
            }
        }
    }
    props
}

/// Compute drift between desired and current partition properties.
///
/// Returns a list of `DriftItem` entries for every property that differs
/// between the desired configuration and the current state reported by Slurm.
fn compute_partition_drift(
    desired: &HashMap<String, String>,
    current: &HashMap<String, String>,
) -> Vec<DriftItem> {
    let mut drift = Vec::new();

    for (key, desired_value) in desired {
        let actual_value = current.get(key).map(|v| v.as_str()).unwrap_or("(not set)");
        // Case-insensitive comparison for most Slurm properties
        if !desired_value.eq_ignore_ascii_case(actual_value) {
            drift.push(DriftItem {
                field: key.clone(),
                desired: desired_value.clone(),
                actual: actual_value.to_string(),
            });
        }
    }

    // Sort by field name for deterministic output
    drift.sort_by(|a, b| a.field.cmp(&b.field));
    drift
}

/// Validate user-provided partition properties before applying.
///
/// Checks format constraints on known properties:
/// - MaxTime: must match Slurm time format or INFINITE/UNLIMITED
/// - Nodes: must be valid Slurm hostlist syntax
/// - PriorityTier: must be a positive integer
/// - State: must be UP or DOWN
fn validate_partition_properties(params: &ModuleParams) -> ModuleResult<PreflightResult> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Validate MaxTime format
    if let Some(max_time) = params.get_string("max_time")? {
        if !is_valid_max_time(&max_time) {
            errors.push(format!(
                "Invalid MaxTime format '{}'. Expected INFINITE, UNLIMITED, or time format like 7-00:00:00, 1:00:00, 30:00, etc.",
                max_time
            ));
        }
    }

    // Validate node list syntax
    if let Some(nodes) = params.get_string("nodes")? {
        if !is_valid_hostlist(&nodes) {
            errors.push(format!(
                "Invalid node list syntax '{}'. Must be a valid Slurm hostlist (alphanumeric, brackets, commas, dashes)",
                nodes
            ));
        }
    }

    // Validate PriorityTier
    if let Some(priority_tier) = params.get_string("priority_tier")? {
        match priority_tier.parse::<u32>() {
            Ok(0) => {
                warnings
                    .push("PriorityTier=0 means this partition has lowest priority".to_string());
            }
            Ok(_) => {}
            Err(_) => errors.push(format!(
                "Invalid PriorityTier '{}'. Must be a non-negative integer",
                priority_tier
            )),
        }
    }

    // Validate State in properties map
    if let Some(serde_json::Value::Object(obj)) = params.get("properties") {
        if let Some(state_val) = obj.get("State") {
            if let Some(state_str) = state_val.as_str() {
                let upper = state_str.to_uppercase();
                if upper != "UP" && upper != "DOWN" {
                    errors.push(format!(
                        "Invalid partition State '{}'. Must be 'UP' or 'DOWN'",
                        state_str
                    ));
                }
            }
        }

        // Validate MaxTime in properties map if set there
        if let Some(mt_val) = obj.get("MaxTime") {
            if let Some(mt_str) = mt_val.as_str() {
                if !is_valid_max_time(mt_str) {
                    errors.push(format!(
                        "Invalid MaxTime format '{}' in properties map",
                        mt_str
                    ));
                }
            }
        }

        // Validate PriorityTier in properties map if set there
        if let Some(pt_val) = obj.get("PriorityTier") {
            let pt_str = if let Some(s) = pt_val.as_str() {
                s.to_string()
            } else if let Some(n) = pt_val.as_i64() {
                n.to_string()
            } else {
                String::new()
            };
            if !pt_str.is_empty() {
                match pt_str.parse::<u32>() {
                    Ok(_) => {}
                    Err(_) => errors.push(format!(
                        "Invalid PriorityTier '{}' in properties map. Must be a non-negative integer",
                        pt_str
                    )),
                }
            }
        }

        // Validate Nodes in properties map if set there
        if let Some(nodes_val) = obj.get("Nodes") {
            if let Some(nodes_str) = nodes_val.as_str() {
                if !is_valid_hostlist(nodes_str) {
                    errors.push(format!(
                        "Invalid node list syntax '{}' in properties map",
                        nodes_str
                    ));
                }
            }
        }
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Check if a string is a valid Slurm MaxTime format.
///
/// Valid formats: INFINITE, UNLIMITED, or time specifications like:
///   minutes, minutes:seconds, hours:minutes:seconds,
///   days-hours, days-hours:minutes, days-hours:minutes:seconds
fn is_valid_max_time(s: &str) -> bool {
    let upper = s.to_uppercase();
    if upper == "INFINITE" || upper == "UNLIMITED" {
        return true;
    }
    // Match Slurm time formats:
    //   D-HH:MM:SS, D-HH:MM, D-HH, HH:MM:SS, MM:SS, MM
    let re = Regex::new(r"^(\d+(-\d+(:\d+(:\d+)?)?)?|\d+(:\d+(:\d+)?)?)$").unwrap();
    re.is_match(s)
}

/// Check if a string is valid Slurm hostlist notation.
///
/// Valid: alphanumeric node names, bracket ranges like node[01-10],
/// comma-separated lists, and combinations thereof.
fn is_valid_hostlist(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    // Slurm hostlists: alphanumeric, hyphens, brackets, commas, underscores
    let re = Regex::new(r"^[a-zA-Z0-9\[\]\-_,]+$").unwrap();
    re.is_match(s)
}

/// Build a map of desired partition properties from params for drift comparison.
///
/// Maps user-facing param names to their Slurm scontrol property equivalents.
fn build_desired_properties(params: &ModuleParams) -> ModuleResult<HashMap<String, String>> {
    let mut desired = HashMap::new();

    if let Some(nodes) = params.get_string("nodes")? {
        desired.insert("Nodes".to_string(), nodes);
    }
    if let Some(max_time) = params.get_string("max_time")? {
        desired.insert("MaxTime".to_string(), max_time);
    }
    if let Some(default_val) = params.get_bool("default")? {
        desired.insert(
            "Default".to_string(),
            if default_val { "YES" } else { "NO" }.to_string(),
        );
    }
    if let Some(priority_tier) = params.get_string("priority_tier")? {
        desired.insert("PriorityTier".to_string(), priority_tier);
    }

    // Include additional properties from the map
    if let Some(serde_json::Value::Object(obj)) = params.get("properties") {
        for (key, value) in obj {
            let val_str = if let Some(s) = value.as_str() {
                s.to_string()
            } else if let Some(n) = value.as_i64() {
                n.to_string()
            } else if let Some(b) = value.as_bool() {
                if b { "YES" } else { "NO" }.to_string()
            } else {
                continue;
            };
            desired.insert(key.clone(), val_str);
        }
    }

    Ok(desired)
}

/// Build scontrol partition property string from params.
fn build_partition_properties(params: &ModuleParams) -> ModuleResult<String> {
    let mut props = Vec::new();

    if let Some(nodes) = params.get_string("nodes")? {
        props.push(format!("Nodes={}", nodes));
    }
    if let Some(max_time) = params.get_string("max_time")? {
        props.push(format!("MaxTime={}", max_time));
    }
    if let Some(default) = params.get_bool("default")? {
        if default {
            props.push("Default=YES".to_string());
        } else {
            props.push("Default=NO".to_string());
        }
    }
    if let Some(priority_tier) = params.get_string("priority_tier")? {
        props.push(format!("PriorityTier={}", priority_tier));
    }

    // Handle additional properties map
    if let Some(serde_json::Value::Object(obj)) = params.get("properties") {
        for (key, value) in obj {
            if let Some(val_str) = value.as_str() {
                props.push(format!("{}={}", key, val_str));
            } else if let Some(val_num) = value.as_i64() {
                props.push(format!("{}={}", key, val_num));
            } else if let Some(val_bool) = value.as_bool() {
                props.push(format!("{}={}", key, if val_bool { "YES" } else { "NO" }));
            }
        }
    }

    Ok(props.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_name() {
        let module = SlurmPartitionModule;
        assert_eq!(module.name(), "slurm_partition");
    }

    #[test]
    fn test_required_params() {
        let module = SlurmPartitionModule;
        let required = module.required_params();
        assert_eq!(required, &["name"]);
    }

    #[test]
    fn test_optional_params() {
        let module = SlurmPartitionModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("nodes"));
        assert!(optional.contains_key("max_time"));
        assert!(optional.contains_key("default"));
        assert!(optional.contains_key("priority_tier"));
        assert!(optional.contains_key("properties"));
        assert!(optional.contains_key("staged"));
        assert!(optional.contains_key("validate"));
    }

    #[test]
    fn test_build_partition_properties_full() {
        let mut params = ModuleParams::new();
        params.insert("nodes".to_string(), serde_json::json!("node[01-10]"));
        params.insert("max_time".to_string(), serde_json::json!("7-00:00:00"));
        params.insert("default".to_string(), serde_json::json!(true));
        params.insert("priority_tier".to_string(), serde_json::json!("10"));

        let props = build_partition_properties(&params).unwrap();
        assert!(props.contains("Nodes=node[01-10]"));
        assert!(props.contains("MaxTime=7-00:00:00"));
        assert!(props.contains("Default=YES"));
        assert!(props.contains("PriorityTier=10"));
    }

    #[test]
    fn test_build_partition_properties_empty() {
        let params = ModuleParams::new();
        let props = build_partition_properties(&params).unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn test_build_partition_properties_partial() {
        let mut params = ModuleParams::new();
        params.insert("nodes".to_string(), serde_json::json!("node01"));
        params.insert("default".to_string(), serde_json::json!(false));

        let props = build_partition_properties(&params).unwrap();
        assert!(props.contains("Nodes=node01"));
        assert!(props.contains("Default=NO"));
        assert!(!props.contains("MaxTime"));
        assert!(!props.contains("PriorityTier"));
    }

    #[test]
    fn test_build_partition_properties_with_map() {
        let mut params = ModuleParams::new();
        params.insert("nodes".to_string(), serde_json::json!("node01"));

        let mut properties_map = serde_json::Map::new();
        properties_map.insert("State".to_string(), serde_json::json!("UP"));
        properties_map.insert("AllowGroups".to_string(), serde_json::json!("physics"));
        properties_map.insert("PreemptMode".to_string(), serde_json::json!("CANCEL"));
        properties_map.insert("OverSubscribe".to_string(), serde_json::json!(false));

        params.insert("properties".to_string(), serde_json::json!(properties_map));

        let props = build_partition_properties(&params).unwrap();
        assert!(props.contains("Nodes=node01"));
        assert!(props.contains("State=UP"));
        assert!(props.contains("AllowGroups=physics"));
        assert!(props.contains("PreemptMode=CANCEL"));
        assert!(props.contains("OverSubscribe=NO"));
    }

    #[test]
    fn test_build_partition_properties_with_numeric_property() {
        let mut params = ModuleParams::new();

        let mut properties_map = serde_json::Map::new();
        properties_map.insert("MaxNodes".to_string(), serde_json::json!(100));
        properties_map.insert("MinNodes".to_string(), serde_json::json!(1));

        params.insert("properties".to_string(), serde_json::json!(properties_map));

        let props = build_partition_properties(&params).unwrap();
        assert!(props.contains("MaxNodes=100"));
        assert!(props.contains("MinNodes=1"));
    }

    // --- New tests for SCH-02 enhancements ---

    #[test]
    fn test_partition_property_parsing() {
        // Typical scontrol show partition output
        let output = "\
PartitionName=batch
   AllowGroups=ALL AllowAccounts=ALL AllowQos=ALL
   AllocNodes=ALL Default=NO QoS=N/A
   DefaultTime=NONE DisableRootJobs=NO ExclusiveUser=NO GraceTime=0 Hidden=NO
   MaxNodes=UNLIMITED MaxTime=7-00:00:00 MinNodes=0 LLN=NO MaxCPUsPerNode=UNLIMITED
   Nodes=node[01-10]
   PriorityJobFactor=1 PriorityTier=1 RootOnly=NO ReqResv=NO OverSubscribe=NO
   OverTimeLimit=NONE PreemptMode=OFF
   State=UP TotalCPUs=320 TotalNodes=10 SelectTypeParameters=NONE";

        let props = parse_scontrol_partition_output(output);

        assert_eq!(props.get("PartitionName"), Some(&"batch".to_string()));
        assert_eq!(props.get("AllowGroups"), Some(&"ALL".to_string()));
        assert_eq!(props.get("MaxTime"), Some(&"7-00:00:00".to_string()));
        assert_eq!(props.get("Nodes"), Some(&"node[01-10]".to_string()));
        assert_eq!(props.get("State"), Some(&"UP".to_string()));
        assert_eq!(props.get("PriorityTier"), Some(&"1".to_string()));
        assert_eq!(props.get("Default"), Some(&"NO".to_string()));
        assert_eq!(props.get("MaxNodes"), Some(&"UNLIMITED".to_string()));
        assert_eq!(props.get("MinNodes"), Some(&"0".to_string()));
        assert_eq!(props.get("TotalCPUs"), Some(&"320".to_string()));
        assert_eq!(props.get("TotalNodes"), Some(&"10".to_string()));
    }

    #[test]
    fn test_partition_property_parsing_empty() {
        let props = parse_scontrol_partition_output("");
        assert!(props.is_empty());
    }

    #[test]
    fn test_partition_property_parsing_single_line() {
        let output = "PartitionName=debug MaxTime=1:00:00 State=UP Nodes=node01";
        let props = parse_scontrol_partition_output(output);

        assert_eq!(props.get("PartitionName"), Some(&"debug".to_string()));
        assert_eq!(props.get("MaxTime"), Some(&"1:00:00".to_string()));
        assert_eq!(props.get("State"), Some(&"UP".to_string()));
        assert_eq!(props.get("Nodes"), Some(&"node01".to_string()));
    }

    #[test]
    fn test_drift_detection() {
        let mut desired = HashMap::new();
        desired.insert("MaxTime".to_string(), "14-00:00:00".to_string());
        desired.insert("Nodes".to_string(), "node[01-20]".to_string());
        desired.insert("State".to_string(), "UP".to_string());
        desired.insert("PriorityTier".to_string(), "5".to_string());

        let mut current = HashMap::new();
        current.insert("MaxTime".to_string(), "7-00:00:00".to_string());
        current.insert("Nodes".to_string(), "node[01-10]".to_string());
        current.insert("State".to_string(), "UP".to_string());
        current.insert("PriorityTier".to_string(), "1".to_string());

        let drift = compute_partition_drift(&desired, &current);

        // State matches (UP=UP), so 3 drifted fields
        assert_eq!(drift.len(), 3);

        // Verify each drifted field (sorted by field name)
        assert_eq!(drift[0].field, "MaxTime");
        assert_eq!(drift[0].desired, "14-00:00:00");
        assert_eq!(drift[0].actual, "7-00:00:00");

        assert_eq!(drift[1].field, "Nodes");
        assert_eq!(drift[1].desired, "node[01-20]");
        assert_eq!(drift[1].actual, "node[01-10]");

        assert_eq!(drift[2].field, "PriorityTier");
        assert_eq!(drift[2].desired, "5");
        assert_eq!(drift[2].actual, "1");
    }

    #[test]
    fn test_drift_detection_no_drift() {
        let mut desired = HashMap::new();
        desired.insert("MaxTime".to_string(), "7-00:00:00".to_string());
        desired.insert("State".to_string(), "UP".to_string());

        let mut current = HashMap::new();
        current.insert("MaxTime".to_string(), "7-00:00:00".to_string());
        current.insert("State".to_string(), "UP".to_string());

        let drift = compute_partition_drift(&desired, &current);
        assert!(drift.is_empty());
    }

    #[test]
    fn test_drift_detection_case_insensitive() {
        let mut desired = HashMap::new();
        desired.insert("State".to_string(), "up".to_string());

        let mut current = HashMap::new();
        current.insert("State".to_string(), "UP".to_string());

        let drift = compute_partition_drift(&desired, &current);
        assert!(
            drift.is_empty(),
            "State comparison should be case-insensitive"
        );
    }

    #[test]
    fn test_drift_detection_missing_current_property() {
        let mut desired = HashMap::new();
        desired.insert("MaxTime".to_string(), "7-00:00:00".to_string());
        desired.insert("PriorityTier".to_string(), "5".to_string());

        let mut current = HashMap::new();
        current.insert("MaxTime".to_string(), "7-00:00:00".to_string());
        // PriorityTier not in current

        let drift = compute_partition_drift(&desired, &current);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].field, "PriorityTier");
        assert_eq!(drift[0].actual, "(not set)");
    }

    #[test]
    fn test_property_validation_valid_max_time() {
        // All valid Slurm time formats
        assert!(is_valid_max_time("INFINITE"));
        assert!(is_valid_max_time("UNLIMITED"));
        assert!(is_valid_max_time("infinite"));
        assert!(is_valid_max_time("30"));
        assert!(is_valid_max_time("30:00"));
        assert!(is_valid_max_time("1:30:00"));
        assert!(is_valid_max_time("7-00:00:00"));
        assert!(is_valid_max_time("7-00"));
        assert!(is_valid_max_time("7-00:00"));
    }

    #[test]
    fn test_property_validation_invalid_max_time() {
        assert!(!is_valid_max_time(""));
        assert!(!is_valid_max_time("abc"));
        assert!(!is_valid_max_time("7 days"));
        assert!(!is_valid_max_time("-1:00:00"));
    }

    #[test]
    fn test_property_validation_valid_hostlist() {
        assert!(is_valid_hostlist("node01"));
        assert!(is_valid_hostlist("node[01-10]"));
        assert!(is_valid_hostlist("node[01-10],node[20-30]"));
        assert!(is_valid_hostlist("compute-node01"));
        assert!(is_valid_hostlist("rack1_node[01-05]"));
    }

    #[test]
    fn test_property_validation_invalid_hostlist() {
        assert!(!is_valid_hostlist(""));
        assert!(!is_valid_hostlist("node 01"));
        assert!(!is_valid_hostlist("node;01"));
        assert!(!is_valid_hostlist("node|01"));
    }

    #[test]
    fn test_property_validation_full() {
        let mut params = ModuleParams::new();
        params.insert("max_time".to_string(), serde_json::json!("7-00:00:00"));
        params.insert("nodes".to_string(), serde_json::json!("node[01-10]"));
        params.insert("priority_tier".to_string(), serde_json::json!("5"));

        let mut properties_map = serde_json::Map::new();
        properties_map.insert("State".to_string(), serde_json::json!("UP"));
        params.insert("properties".to_string(), serde_json::json!(properties_map));

        let result = validate_partition_properties(&params).unwrap();
        assert!(result.passed);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_property_validation_invalid_state() {
        let mut params = ModuleParams::new();

        let mut properties_map = serde_json::Map::new();
        properties_map.insert("State".to_string(), serde_json::json!("RUNNING"));
        params.insert("properties".to_string(), serde_json::json!(properties_map));

        let result = validate_partition_properties(&params).unwrap();
        assert!(!result.passed);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("Invalid partition State")));
    }

    #[test]
    fn test_property_validation_invalid_priority_tier() {
        let mut params = ModuleParams::new();
        params.insert("priority_tier".to_string(), serde_json::json!("abc"));

        let result = validate_partition_properties(&params).unwrap();
        assert!(!result.passed);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("Invalid PriorityTier")));
    }

    #[test]
    fn test_property_validation_priority_tier_zero_warning() {
        let mut params = ModuleParams::new();
        params.insert("priority_tier".to_string(), serde_json::json!("0"));

        let result = validate_partition_properties(&params).unwrap();
        assert!(result.passed);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("lowest priority")));
    }

    #[test]
    fn test_build_desired_properties() {
        let mut params = ModuleParams::new();
        params.insert("nodes".to_string(), serde_json::json!("node[01-10]"));
        params.insert("max_time".to_string(), serde_json::json!("7-00:00:00"));
        params.insert("default".to_string(), serde_json::json!(true));
        params.insert("priority_tier".to_string(), serde_json::json!("5"));

        let mut properties_map = serde_json::Map::new();
        properties_map.insert("State".to_string(), serde_json::json!("UP"));
        params.insert("properties".to_string(), serde_json::json!(properties_map));

        let desired = build_desired_properties(&params).unwrap();
        assert_eq!(desired.get("Nodes"), Some(&"node[01-10]".to_string()));
        assert_eq!(desired.get("MaxTime"), Some(&"7-00:00:00".to_string()));
        assert_eq!(desired.get("Default"), Some(&"YES".to_string()));
        assert_eq!(desired.get("PriorityTier"), Some(&"5".to_string()));
        assert_eq!(desired.get("State"), Some(&"UP".to_string()));
    }
}
