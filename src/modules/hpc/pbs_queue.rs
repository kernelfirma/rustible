//! PBS Pro queue management module
//!
//! Manage PBS queues via qstat and qmgr (create, delete, enable, disable, start, stop).
//!
//! # Parameters
//!
//! - `action` (required): "list", "create", "delete", "enable", "disable", "start", "stop",
//!   or "set_attributes"
//! - `name` (required): Queue name
//! - `queue_type` (optional): Queue type ("execution" or "route", default "execution")
//! - `enabled` (optional): Whether the queue accepts jobs ("True"/"False")
//! - `started` (optional): Whether the queue routes/runs jobs ("True"/"False")
//! - `max_run` (optional): Maximum running jobs in queue
//! - `max_queued` (optional): Maximum queued jobs in queue
//! - `resources_max_walltime` (optional): Maximum walltime (e.g. "168:00:00")
//! - `resources_max_ncpus` (optional): Maximum CPUs per job
//! - `resources_max_mem` (optional): Maximum memory per job (e.g. "256gb")
//! - `resources_default_walltime` (optional): Default walltime for jobs
//! - `priority` (optional): Queue priority value
//! - `acl_groups` (optional): Comma-separated ACL groups
//! - `attributes` (optional): JSON object of arbitrary queue attributes

use std::collections::HashMap;
use std::sync::Arc;

use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
    ParallelizationHint,
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

pub struct PbsQueueModule;

impl Module for PbsQueueModule {
    fn name(&self) -> &'static str {
        "pbs_queue"
    }

    fn description(&self) -> &'static str {
        "Manage PBS Pro queues (create, delete, enable, disable, start, stop, set_attributes)"
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
            "list" => self.action_list(connection, context),
            "create" => self.action_create(connection, &name, params, context),
            "delete" => self.action_delete(connection, &name, context),
            "enable" => self.action_enable(connection, &name, context),
            "disable" => self.action_disable(connection, &name, context),
            "start" => self.action_start(connection, &name, context),
            "stop" => self.action_stop(connection, &name, context),
            "set_attributes" => self.action_set_attributes(connection, &name, params, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'list', 'create', 'delete', 'enable', 'disable', 'start', 'stop', or 'set_attributes'",
                action
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action", "name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("queue_type", serde_json::json!("execution"));
        m.insert("enabled", serde_json::json!(null));
        m.insert("started", serde_json::json!(null));
        m.insert("max_run", serde_json::json!(null));
        m.insert("max_queued", serde_json::json!(null));
        m.insert("resources_max_walltime", serde_json::json!(null));
        m.insert("resources_max_ncpus", serde_json::json!(null));
        m.insert("resources_max_mem", serde_json::json!(null));
        m.insert("resources_default_walltime", serde_json::json!(null));
        m.insert("priority", serde_json::json!(null));
        m.insert("acl_groups", serde_json::json!(null));
        m.insert("attributes", serde_json::json!(null));
        m
    }
}

impl PbsQueueModule {
    fn get_queue_info(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<serde_json::Value>> {
        let (ok, stdout, _) = run_cmd(
            connection,
            "qstat -Q -f -F json 2>/dev/null",
            context,
        )?;
        if !ok || stdout.trim().is_empty() {
            return Ok(None);
        }
        let queues = parse_pbs_json_queues(&stdout);
        if let Some(obj) = queues.as_object() {
            if let Some(queue) = obj.get(name) {
                return Ok(Some(queue.clone()));
            }
        }
        Ok(None)
    }

    fn action_list(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let stdout = run_cmd_ok(
            connection,
            "qstat -Q -f -F json 2>/dev/null",
            context,
        )?;
        let queues = parse_pbs_json_queues(&stdout);

        let count = queues.as_object().map_or(0, |o| o.len());

        Ok(ModuleOutput::ok(format!("Listed {} queue(s)", count))
            .with_data("queues", queues)
            .with_data("count", serde_json::json!(count)))
    }

    fn action_create(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Idempotency: check if queue already exists
        if let Some(existing) = self.get_queue_info(connection, name, context)? {
            return Ok(
                ModuleOutput::ok(format!("Queue '{}' already exists", name))
                    .with_data("queue", existing),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would create queue '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        let queue_type = params
            .get_string("queue_type")?
            .unwrap_or_else(|| "execution".to_string());

        let cmd = format!(
            "qmgr -c \"create queue {} queue_type={}\"",
            name, queue_type
        );
        run_cmd_ok(connection, &cmd, context)?;

        // Apply additional attributes
        let attr_cmds = build_queue_attribute_commands(name, params)?;
        for attr_cmd in &attr_cmds {
            run_cmd_ok(connection, attr_cmd, context)?;
        }

        let current = self.get_queue_info(connection, name, context)?;

        Ok(
            ModuleOutput::changed(format!("Created queue '{}'", name))
                .with_data("name", serde_json::json!(name))
                .with_data("queue", serde_json::json!(current)),
        )
    }

    fn action_delete(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Idempotency: check if queue exists
        if self.get_queue_info(connection, name, context)?.is_none() {
            return Ok(
                ModuleOutput::ok(format!("Queue '{}' does not exist", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would delete queue '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        run_cmd_ok(
            connection,
            &format!("qmgr -c \"delete queue {}\"", name),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Deleted queue '{}'", name))
                .with_data("name", serde_json::json!(name)),
        )
    }

    fn action_enable(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(info) = self.get_queue_info(connection, name, context)? {
            if info.get("enabled").and_then(|v| v.as_str()) == Some("True") {
                return Ok(
                    ModuleOutput::ok(format!("Queue '{}' is already enabled", name))
                        .with_data("name", serde_json::json!(name)),
                );
            }
        } else {
            return Err(ModuleError::ExecutionFailed(format!(
                "Queue '{}' does not exist",
                name
            )));
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would enable queue '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        run_cmd_ok(
            connection,
            &format!("qmgr -c \"set queue {} enabled=True\"", name),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Enabled queue '{}'", name))
                .with_data("name", serde_json::json!(name)),
        )
    }

    fn action_disable(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(info) = self.get_queue_info(connection, name, context)? {
            if info.get("enabled").and_then(|v| v.as_str()) == Some("False") {
                return Ok(
                    ModuleOutput::ok(format!("Queue '{}' is already disabled", name))
                        .with_data("name", serde_json::json!(name)),
                );
            }
        } else {
            return Err(ModuleError::ExecutionFailed(format!(
                "Queue '{}' does not exist",
                name
            )));
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would disable queue '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        run_cmd_ok(
            connection,
            &format!("qmgr -c \"set queue {} enabled=False\"", name),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Disabled queue '{}'", name))
                .with_data("name", serde_json::json!(name)),
        )
    }

    fn action_start(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(info) = self.get_queue_info(connection, name, context)? {
            if info.get("started").and_then(|v| v.as_str()) == Some("True") {
                return Ok(
                    ModuleOutput::ok(format!("Queue '{}' is already started", name))
                        .with_data("name", serde_json::json!(name)),
                );
            }
        } else {
            return Err(ModuleError::ExecutionFailed(format!(
                "Queue '{}' does not exist",
                name
            )));
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would start queue '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        run_cmd_ok(
            connection,
            &format!("qmgr -c \"set queue {} started=True\"", name),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Started queue '{}'", name))
                .with_data("name", serde_json::json!(name)),
        )
    }

    fn action_stop(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(info) = self.get_queue_info(connection, name, context)? {
            if info.get("started").and_then(|v| v.as_str()) == Some("False") {
                return Ok(
                    ModuleOutput::ok(format!("Queue '{}' is already stopped", name))
                        .with_data("name", serde_json::json!(name)),
                );
            }
        } else {
            return Err(ModuleError::ExecutionFailed(format!(
                "Queue '{}' does not exist",
                name
            )));
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would stop queue '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        run_cmd_ok(
            connection,
            &format!("qmgr -c \"set queue {} started=False\"", name),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Stopped queue '{}'", name))
                .with_data("name", serde_json::json!(name)),
        )
    }

    fn action_set_attributes(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let current = self.get_queue_info(connection, name, context)?.ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "Queue '{}' does not exist; cannot set attributes",
                name
            ))
        })?;

        // Build desired attributes and compute diff
        let desired = build_queue_desired_attributes(params)?;
        let changes = compute_queue_changes(&current, &desired);

        if changes.is_empty() {
            return Ok(
                ModuleOutput::ok(format!("Queue '{}' attributes are already up to date", name))
                    .with_data("queue", serde_json::json!(current)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would update queue '{}': {}",
                name,
                changes
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        for (key, value) in &changes {
            let cmd = format!("qmgr -c \"set queue {} {}={}\"", name, key, value);
            run_cmd_ok(connection, &cmd, context)?;
        }

        let updated = self.get_queue_info(connection, name, context)?;

        Ok(
            ModuleOutput::changed(format!("Updated queue '{}' attributes", name))
                .with_data("name", serde_json::json!(name))
                .with_data("changes", serde_json::json!(changes))
                .with_data("queue", serde_json::json!(updated)),
        )
    }
}

/// Parse PBS JSON output from `qstat -Q -f -F json` into the Queue object.
fn parse_pbs_json_queues(output: &str) -> serde_json::Value {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return serde_json::Value::Null;
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(parsed) => {
            // PBS JSON structure: { "Queue": { "qname": { ... }, ... } }
            if let Some(queues) = parsed.get("Queue") {
                queues.clone()
            } else {
                parsed
            }
        }
        Err(_) => serde_json::Value::Null,
    }
}

/// Build qmgr commands to set queue attributes from params.
fn build_queue_attribute_commands(
    name: &str,
    params: &ModuleParams,
) -> ModuleResult<Vec<String>> {
    let mut commands = Vec::new();

    let attr_map: &[(&str, &str)] = &[
        ("enabled", "enabled"),
        ("started", "started"),
        ("max_run", "max_run"),
        ("max_queued", "max_queued"),
        ("resources_max_walltime", "resources_max.walltime"),
        ("resources_max_ncpus", "resources_max.ncpus"),
        ("resources_max_mem", "resources_max.mem"),
        ("resources_default_walltime", "resources_default.walltime"),
        ("priority", "Priority"),
        ("acl_groups", "acl_groups"),
    ];

    for (param_name, pbs_attr) in attr_map {
        if let Some(value) = params.get_string(param_name)? {
            commands.push(format!(
                "qmgr -c \"set queue {} {}={}\"",
                name, pbs_attr, value
            ));
        }
    }

    // Handle arbitrary attributes from JSON object
    if let Some(serde_json::Value::Object(attrs)) = params.get("attributes") {
        for (key, value) in attrs {
            let val_str = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            commands.push(format!(
                "qmgr -c \"set queue {} {}={}\"",
                name, key, val_str
            ));
        }
    }

    Ok(commands)
}

/// Build a map of desired PBS attribute names to values from params.
fn build_queue_desired_attributes(
    params: &ModuleParams,
) -> ModuleResult<HashMap<String, String>> {
    let mut desired = HashMap::new();

    let attr_map: &[(&str, &str)] = &[
        ("enabled", "enabled"),
        ("started", "started"),
        ("max_run", "max_run"),
        ("max_queued", "max_queued"),
        ("resources_max_walltime", "resources_max.walltime"),
        ("resources_max_ncpus", "resources_max.ncpus"),
        ("resources_max_mem", "resources_max.mem"),
        ("resources_default_walltime", "resources_default.walltime"),
        ("priority", "Priority"),
        ("acl_groups", "acl_groups"),
    ];

    for (param_name, pbs_attr) in attr_map {
        if let Some(value) = params.get_string(param_name)? {
            desired.insert(pbs_attr.to_string(), value);
        }
    }

    // Handle arbitrary attributes from JSON object
    if let Some(serde_json::Value::Object(attrs)) = params.get("attributes") {
        for (key, value) in attrs {
            let val_str = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            desired.insert(key.clone(), val_str);
        }
    }

    Ok(desired)
}

/// Compare desired attributes against current queue info and return only differences.
fn compute_queue_changes(
    current: &serde_json::Value,
    desired: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut changes = HashMap::new();
    for (key, desired_val) in desired {
        let needs_change = match current.get(key).and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| Some(v.to_string()))
        }) {
            Some(current_val) => {
                // Strip quotes from JSON stringified values
                let clean = current_val.trim_matches('"');
                clean != desired_val.as_str()
            }
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
    fn test_parse_pbs_json_queues() {
        let json = r#"{
            "Queue": {
                "batch": {
                    "queue_type": "Execution",
                    "enabled": "True",
                    "started": "True",
                    "total_jobs": 42
                },
                "gpu": {
                    "queue_type": "Execution",
                    "enabled": "True",
                    "started": "False"
                }
            }
        }"#;
        let queues = parse_pbs_json_queues(json);
        assert!(queues.is_object());
        assert!(queues.get("batch").is_some());
        assert!(queues.get("gpu").is_some());
        assert_eq!(queues["batch"]["enabled"], "True");
    }

    #[test]
    fn test_parse_pbs_json_queues_empty() {
        let queues = parse_pbs_json_queues("");
        assert!(queues.is_null());
    }

    #[test]
    fn test_parse_pbs_json_queues_malformed() {
        let queues = parse_pbs_json_queues("not valid json {{{");
        assert!(queues.is_null());
    }

    #[test]
    fn test_build_queue_attribute_commands() {
        let mut params = ModuleParams::new();
        params.insert("enabled".to_string(), serde_json::json!("True"));
        params.insert("started".to_string(), serde_json::json!("True"));
        params.insert(
            "resources_max_walltime".to_string(),
            serde_json::json!("168:00:00"),
        );
        params.insert("priority".to_string(), serde_json::json!("100"));

        let cmds = build_queue_attribute_commands("batch", &params).unwrap();
        assert_eq!(cmds.len(), 4);
        assert!(cmds.iter().any(|c| c.contains("enabled=True")));
        assert!(cmds.iter().any(|c| c.contains("started=True")));
        assert!(cmds
            .iter()
            .any(|c| c.contains("resources_max.walltime=168:00:00")));
        assert!(cmds.iter().any(|c| c.contains("Priority=100")));
    }

    #[test]
    fn test_build_queue_attribute_commands_empty() {
        let params = ModuleParams::new();
        let cmds = build_queue_attribute_commands("batch", &params).unwrap();
        assert!(cmds.is_empty());
    }

    #[test]
    fn test_build_queue_attribute_commands_all_params() {
        let mut params = ModuleParams::new();
        params.insert("enabled".to_string(), serde_json::json!("True"));
        params.insert("started".to_string(), serde_json::json!("True"));
        params.insert("max_run".to_string(), serde_json::json!("50"));
        params.insert("max_queued".to_string(), serde_json::json!("200"));
        params.insert(
            "resources_max_walltime".to_string(),
            serde_json::json!("72:00:00"),
        );
        params.insert("resources_max_ncpus".to_string(), serde_json::json!("128"));
        params.insert(
            "resources_max_mem".to_string(),
            serde_json::json!("256gb"),
        );
        params.insert(
            "resources_default_walltime".to_string(),
            serde_json::json!("01:00:00"),
        );
        params.insert("priority".to_string(), serde_json::json!("50"));
        params.insert(
            "acl_groups".to_string(),
            serde_json::json!("admin,research"),
        );

        let cmds = build_queue_attribute_commands("batch", &params).unwrap();
        assert_eq!(cmds.len(), 10);
    }

    #[test]
    fn test_compute_queue_changes_no_changes() {
        let current = serde_json::json!({
            "enabled": "True",
            "started": "True",
            "Priority": "100"
        });
        let mut desired = HashMap::new();
        desired.insert("enabled".to_string(), "True".to_string());
        desired.insert("started".to_string(), "True".to_string());
        desired.insert("Priority".to_string(), "100".to_string());

        let changes = compute_queue_changes(&current, &desired);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_compute_queue_changes_with_changes() {
        let current = serde_json::json!({
            "enabled": "True",
            "started": "True",
            "Priority": "100"
        });
        let mut desired = HashMap::new();
        desired.insert("enabled".to_string(), "False".to_string());
        desired.insert("started".to_string(), "True".to_string());
        desired.insert("Priority".to_string(), "200".to_string());

        let changes = compute_queue_changes(&current, &desired);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes.get("enabled"), Some(&"False".to_string()));
        assert_eq!(changes.get("Priority"), Some(&"200".to_string()));
    }

    #[test]
    fn test_compute_queue_changes_new_attribute() {
        let current = serde_json::json!({
            "enabled": "True"
        });
        let mut desired = HashMap::new();
        desired.insert("max_run".to_string(), "50".to_string());

        let changes = compute_queue_changes(&current, &desired);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes.get("max_run"), Some(&"50".to_string()));
    }
}
