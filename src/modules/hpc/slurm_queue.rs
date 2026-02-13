//! Slurm partition (queue) management module
//!
//! Manage partitions at runtime via scontrol create/update/delete.
//!
//! # Parameters
//!
//! - `action` (required): "create", "update", "delete", or "state"
//! - `name` (required): Partition name
//! - `nodes` (optional): Node list for the partition
//! - `default` (optional): Whether this is the default partition ("yes"/"no")
//! - `max_time` (optional): Maximum time limit (e.g. "7-00:00:00")
//! - `max_nodes` (optional): Maximum nodes per job
//! - `state` (optional): Partition state ("UP" or "DOWN")
//! - `priority_tier` (optional): Priority tier value
//! - `allow_groups` (optional): Allowed groups (comma-separated)

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

pub struct SlurmQueueModule;

impl Module for SlurmQueueModule {
    fn name(&self) -> &'static str {
        "slurm_queue"
    }

    fn description(&self) -> &'static str {
        "Manage Slurm partitions at runtime (create, update, delete, state)"
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

        let action = params.get_string_required("action")?;
        let name = params.get_string_required("name")?;

        match action.as_str() {
            "create" => self.action_create(connection, &name, params, context),
            "update" => self.action_update(connection, &name, params, context),
            "delete" => self.action_delete(connection, &name, context),
            "state" => self.action_state(connection, &name, params, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'create', 'update', 'delete', or 'state'",
                action
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action", "name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("nodes", serde_json::json!(null));
        m.insert("default", serde_json::json!(null));
        m.insert("max_time", serde_json::json!(null));
        m.insert("max_nodes", serde_json::json!(null));
        m.insert("state", serde_json::json!(null));
        m.insert("priority_tier", serde_json::json!(null));
        m.insert("allow_groups", serde_json::json!(null));
        m
    }
}

impl SlurmQueueModule {
    fn get_partition(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<HashMap<String, String>>> {
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("scontrol show partition {} -o 2>/dev/null", name),
            context,
        )?;
        if !ok || stdout.trim().is_empty() || stdout.contains("not found") {
            return Ok(None);
        }
        Ok(Some(parse_scontrol_oneliner(&stdout)))
    }

    fn action_create(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Idempotency: check if partition already exists
        if let Some(existing) = self.get_partition(connection, name, context)? {
            return Ok(
                ModuleOutput::ok(format!("Partition '{}' already exists", name))
                    .with_data("partition", serde_json::json!(existing)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would create partition '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        let props = build_partition_properties(params)?;
        let cmd = format!("scontrol create PartitionName={} {}", name, props);
        run_cmd_ok(connection, &cmd, context)?;

        let current = self.get_partition(connection, name, context)?;

        Ok(
            ModuleOutput::changed(format!("Created partition '{}'", name))
                .with_data("name", serde_json::json!(name))
                .with_data("partition", serde_json::json!(current)),
        )
    }

    fn action_update(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Check existence
        let current = self
            .get_partition(connection, name, context)?
            .ok_or_else(|| {
                ModuleError::ExecutionFailed(format!(
                    "Partition '{}' does not exist; cannot update",
                    name
                ))
            })?;

        // Build desired properties and compare
        let desired = build_desired_properties(params)?;
        let changes = compute_property_changes(&current, &desired);

        if changes.is_empty() {
            return Ok(
                ModuleOutput::ok(format!("Partition '{}' is already up to date", name))
                    .with_data("partition", serde_json::json!(current)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would update partition '{}': {}",
                name,
                changes
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(" ")
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let update_str: String = changes
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(" ");
        let cmd = format!("scontrol update PartitionName={} {}", name, update_str);
        run_cmd_ok(connection, &cmd, context)?;

        let updated = self.get_partition(connection, name, context)?;

        Ok(
            ModuleOutput::changed(format!("Updated partition '{}'", name))
                .with_data("name", serde_json::json!(name))
                .with_data("changes", serde_json::json!(changes))
                .with_data("partition", serde_json::json!(updated)),
        )
    }

    fn action_delete(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Idempotency: check if partition exists
        if self.get_partition(connection, name, context)?.is_none() {
            return Ok(
                ModuleOutput::ok(format!("Partition '{}' does not exist", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would delete partition '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        run_cmd_ok(
            connection,
            &format!("scontrol delete PartitionName={}", name),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Deleted partition '{}'", name))
                .with_data("name", serde_json::json!(name)),
        )
    }

    fn action_state(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let desired_state = params.get_string_required("state")?;
        let desired_upper = desired_state.to_uppercase();

        if desired_upper != "UP" && desired_upper != "DOWN" {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'UP' or 'DOWN'",
                desired_state
            )));
        }

        let current = self
            .get_partition(connection, name, context)?
            .ok_or_else(|| {
                ModuleError::ExecutionFailed(format!("Partition '{}' does not exist", name))
            })?;

        let current_state = current.get("State").map(|s| s.to_uppercase());
        if current_state.as_deref() == Some(&desired_upper) {
            return Ok(ModuleOutput::ok(format!(
                "Partition '{}' is already {}",
                name, desired_upper
            ))
            .with_data("state", serde_json::json!(desired_upper)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would set partition '{}' to {}",
                name, desired_upper
            ))
            .with_data("state", serde_json::json!(desired_upper)));
        }

        run_cmd_ok(
            connection,
            &format!(
                "scontrol update PartitionName={} State={}",
                name, desired_upper
            ),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Set partition '{}' to {}", name, desired_upper))
                .with_data("name", serde_json::json!(name))
                .with_data("state", serde_json::json!(desired_upper)),
        )
    }
}

/// Parse scontrol one-liner output into key-value HashMap.
/// Format: "Key1=Value1 Key2=Value2 Key3=Value3"
fn parse_scontrol_oneliner(output: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let line = output.lines().next().unwrap_or("").trim();
    for token in line.split_whitespace() {
        if let Some((key, value)) = token.split_once('=') {
            map.insert(key.to_string(), value.to_string());
        }
    }
    map
}

/// Build scontrol partition property string from params.
fn build_partition_properties(params: &ModuleParams) -> ModuleResult<String> {
    let mut props = Vec::new();

    if let Some(nodes) = params.get_string("nodes")? {
        props.push(format!("Nodes={}", nodes));
    }
    if let Some(default) = params.get_string("default")? {
        let val = if default.to_lowercase() == "yes" || default.to_lowercase() == "true" {
            "YES"
        } else {
            "NO"
        };
        props.push(format!("Default={}", val));
    }
    if let Some(max_time) = params.get_string("max_time")? {
        props.push(format!("MaxTime={}", max_time));
    }
    if let Some(max_nodes) = params.get_string("max_nodes")? {
        props.push(format!("MaxNodes={}", max_nodes));
    }
    if let Some(state) = params.get_string("state")? {
        props.push(format!("State={}", state.to_uppercase()));
    }
    if let Some(priority) = params.get_string("priority_tier")? {
        props.push(format!("PriorityTier={}", priority));
    }
    if let Some(groups) = params.get_string("allow_groups")? {
        props.push(format!("AllowGroups={}", groups));
    }

    Ok(props.join(" "))
}

/// Build a map of desired scontrol property names to values (using Slurm key names).
fn build_desired_properties(params: &ModuleParams) -> ModuleResult<HashMap<String, String>> {
    let mut desired = HashMap::new();
    if let Some(nodes) = params.get_string("nodes")? {
        desired.insert("Nodes".to_string(), nodes);
    }
    if let Some(default) = params.get_string("default")? {
        let val = if default.to_lowercase() == "yes" || default.to_lowercase() == "true" {
            "YES".to_string()
        } else {
            "NO".to_string()
        };
        desired.insert("Default".to_string(), val);
    }
    if let Some(max_time) = params.get_string("max_time")? {
        desired.insert("MaxTime".to_string(), max_time);
    }
    if let Some(max_nodes) = params.get_string("max_nodes")? {
        desired.insert("MaxNodes".to_string(), max_nodes);
    }
    if let Some(state) = params.get_string("state")? {
        desired.insert("State".to_string(), state.to_uppercase());
    }
    if let Some(priority) = params.get_string("priority_tier")? {
        desired.insert("PriorityTier".to_string(), priority);
    }
    if let Some(groups) = params.get_string("allow_groups")? {
        desired.insert("AllowGroups".to_string(), groups);
    }
    Ok(desired)
}

/// Compare desired properties against current and return only differences.
fn compute_property_changes(
    current: &HashMap<String, String>,
    desired: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut changes = HashMap::new();
    for (key, desired_val) in desired {
        let needs_change = match current.get(key) {
            Some(current_val) => current_val != desired_val,
            None => true,
        };
        if needs_change {
            changes.insert(key.clone(), desired_val.clone());
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_scontrol_oneliner() {
        let output =
            "PartitionName=compute AllowGroups=ALL AllowAccounts=ALL Default=YES MaxTime=UNLIMITED State=UP TotalCPUs=512 TotalNodes=16\n";
        let map = parse_scontrol_oneliner(output);
        assert_eq!(map.get("PartitionName"), Some(&"compute".to_string()));
        assert_eq!(map.get("Default"), Some(&"YES".to_string()));
        assert_eq!(map.get("State"), Some(&"UP".to_string()));
        assert_eq!(map.get("TotalCPUs"), Some(&"512".to_string()));
        assert_eq!(map.get("TotalNodes"), Some(&"16".to_string()));
        assert_eq!(map.get("MaxTime"), Some(&"UNLIMITED".to_string()));
    }

    #[test]
    fn test_parse_scontrol_oneliner_empty() {
        let map = parse_scontrol_oneliner("");
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_scontrol_oneliner_no_equals() {
        let map = parse_scontrol_oneliner("JustSomeText NoEquals Here");
        assert!(map.is_empty());
    }

    #[test]
    fn test_build_partition_properties() {
        let mut params = ModuleParams::new();
        params.insert("nodes".to_string(), serde_json::json!("node[01-10]"));
        params.insert("default".to_string(), serde_json::json!("yes"));
        params.insert("max_time".to_string(), serde_json::json!("7-00:00:00"));
        params.insert("state".to_string(), serde_json::json!("up"));

        let props = build_partition_properties(&params).unwrap();
        assert!(props.contains("Nodes=node[01-10]"));
        assert!(props.contains("Default=YES"));
        assert!(props.contains("MaxTime=7-00:00:00"));
        assert!(props.contains("State=UP"));
    }

    #[test]
    fn test_build_partition_properties_empty() {
        let params = ModuleParams::new();
        let props = build_partition_properties(&params).unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn test_build_desired_properties() {
        let mut params = ModuleParams::new();
        params.insert("nodes".to_string(), serde_json::json!("node[01-05]"));
        params.insert("default".to_string(), serde_json::json!("no"));
        params.insert("priority_tier".to_string(), serde_json::json!("100"));

        let desired = build_desired_properties(&params).unwrap();
        assert_eq!(desired.get("Nodes"), Some(&"node[01-05]".to_string()));
        assert_eq!(desired.get("Default"), Some(&"NO".to_string()));
        assert_eq!(desired.get("PriorityTier"), Some(&"100".to_string()));
    }

    #[test]
    fn test_compute_property_changes_no_changes() {
        let mut current = HashMap::new();
        current.insert("State".to_string(), "UP".to_string());
        current.insert("Nodes".to_string(), "node[01-10]".to_string());

        let mut desired = HashMap::new();
        desired.insert("State".to_string(), "UP".to_string());
        desired.insert("Nodes".to_string(), "node[01-10]".to_string());

        let changes = compute_property_changes(&current, &desired);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_compute_property_changes_with_changes() {
        let mut current = HashMap::new();
        current.insert("State".to_string(), "UP".to_string());
        current.insert("Nodes".to_string(), "node[01-10]".to_string());
        current.insert("MaxTime".to_string(), "UNLIMITED".to_string());

        let mut desired = HashMap::new();
        desired.insert("State".to_string(), "DOWN".to_string());
        desired.insert("Nodes".to_string(), "node[01-10]".to_string());
        desired.insert("MaxTime".to_string(), "7-00:00:00".to_string());

        let changes = compute_property_changes(&current, &desired);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes.get("State"), Some(&"DOWN".to_string()));
        assert_eq!(changes.get("MaxTime"), Some(&"7-00:00:00".to_string()));
        assert!(!changes.contains_key("Nodes"));
    }

    #[test]
    fn test_compute_property_changes_new_property() {
        let current = HashMap::new();
        let mut desired = HashMap::new();
        desired.insert("PriorityTier".to_string(), "50".to_string());

        let changes = compute_property_changes(&current, &desired);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes.get("PriorityTier"), Some(&"50".to_string()));
    }

    #[test]
    fn test_build_partition_properties_allow_groups() {
        let mut params = ModuleParams::new();
        params.insert(
            "allow_groups".to_string(),
            serde_json::json!("admin,research"),
        );
        params.insert("max_nodes".to_string(), serde_json::json!("8"));

        let props = build_partition_properties(&params).unwrap();
        assert!(props.contains("AllowGroups=admin,research"));
        assert!(props.contains("MaxNodes=8"));
    }
}
