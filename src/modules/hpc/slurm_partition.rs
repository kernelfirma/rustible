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
        let exists = self.partition_exists(connection, name, context)?;

        if exists {
            // Partition exists - update if properties are provided
            let props = build_partition_properties(params)?;
            if props.is_empty() {
                return Ok(
                    ModuleOutput::ok(format!("Partition '{}' already exists", name))
                        .with_data("partition", serde_json::json!(name)),
                );
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would update partition '{}' with: {}",
                    name, props
                ))
                .with_data("partition", serde_json::json!(name)));
            }

            let cmd = format!("scontrol update PartitionName={} {}", name, props);
            run_cmd_ok(connection, &cmd, context)?;

            Ok(
                ModuleOutput::changed(format!("Updated partition '{}'", name))
                    .with_data("partition", serde_json::json!(name))
                    .with_data("properties", serde_json::json!(props)),
            )
        } else {
            // Partition doesn't exist - create it
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
}
