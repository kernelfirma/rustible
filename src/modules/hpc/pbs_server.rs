//! PBS Pro server configuration module
//!
//! Query and set PBS server attributes via qmgr, and manage custom resources.
//!
//! # Parameters
//!
//! - `action` (required): "query", "set_attributes", or "manage_resources"
//! - `attributes` (optional): JSON object of server attributes to set
//! - `default_queue` (optional): Default queue name
//! - `scheduling` (optional): Enable/disable scheduling ("True"/"False")
//! - `node_fail_requeue` (optional): Requeue on node failure ("True"/"False")
//! - `max_run` (optional): Maximum running jobs across server
//! - `max_queued` (optional): Maximum queued jobs across server
//! - `query_other_jobs` (optional): Allow users to query other jobs ("True"/"False")
//! - `resources_default_walltime` (optional): Default walltime for server

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

pub struct PbsServerModule;

impl Module for PbsServerModule {
    fn name(&self) -> &'static str {
        "pbs_server"
    }

    fn description(&self) -> &'static str {
        "Query and configure PBS Pro server attributes (qmgr print/set server)"
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

        match action.as_str() {
            "query" => self.action_query(connection, context),
            "set_attributes" => self.action_set_attributes(connection, params, context),
            "manage_resources" => self.action_manage_resources(connection, params, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'query', 'set_attributes', or 'manage_resources'",
                action
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("attributes", serde_json::json!(null));
        m.insert("default_queue", serde_json::json!(null));
        m.insert("scheduling", serde_json::json!(null));
        m.insert("node_fail_requeue", serde_json::json!(null));
        m.insert("max_run", serde_json::json!(null));
        m.insert("max_queued", serde_json::json!(null));
        m.insert("query_other_jobs", serde_json::json!(null));
        m.insert("resources_default_walltime", serde_json::json!(null));
        m
    }
}

impl PbsServerModule {
    fn action_query(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let stdout = run_cmd_ok(
            connection,
            "qmgr -c \"print server\" 2>/dev/null",
            context,
        )?;

        let server_attrs = parse_qmgr_server_output(&stdout);

        Ok(ModuleOutput::ok(format!(
            "Retrieved {} server attribute(s)",
            server_attrs.len()
        ))
        .with_data("server", serde_json::json!(server_attrs)))
    }

    fn action_set_attributes(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Get current server state
        let stdout = run_cmd_ok(
            connection,
            "qmgr -c \"print server\" 2>/dev/null",
            context,
        )?;
        let current = parse_qmgr_server_output(&stdout);

        // Build desired attributes
        let desired = build_server_attribute_pairs(params)?;
        let changes = compute_server_changes(&current, &desired);

        if changes.is_empty() {
            return Ok(
                ModuleOutput::ok("Server attributes are already up to date")
                    .with_data("server", serde_json::json!(current)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would update server: {}",
                changes
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        for (key, value) in &changes {
            let cmd = format!("qmgr -c \"set server {}={}\"", key, value);
            run_cmd_ok(connection, &cmd, context)?;
        }

        // Re-read current state
        let (_, new_stdout, _) = run_cmd(
            connection,
            "qmgr -c \"print server\" 2>/dev/null",
            context,
        )?;
        let updated = parse_qmgr_server_output(&new_stdout);

        Ok(ModuleOutput::changed("Updated server attributes")
            .with_data("changes", serde_json::json!(changes))
            .with_data("server", serde_json::json!(updated)))
    }

    fn action_manage_resources(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let attrs = params.get("attributes").ok_or_else(|| {
            ModuleError::MissingParameter(
                "attributes is required for manage_resources action".to_string(),
            )
        })?;

        let resources = attrs.as_object().ok_or_else(|| {
            ModuleError::InvalidParameter(
                "attributes must be a JSON object mapping resource_name -> resource_type"
                    .to_string(),
            )
        })?;

        // Query existing resources
        let (_, stdout, _) = run_cmd(
            connection,
            "qmgr -c \"print server\" 2>/dev/null",
            context,
        )?;
        let current = parse_qmgr_server_output(&stdout);

        let mut created = Vec::new();
        let mut skipped = Vec::new();

        for (name, type_val) in resources {
            let resource_type = match type_val {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string().trim_matches('"').to_string(),
            };

            // Check if resource already exists (appears as "resources" line in print server)
            let resource_key = format!("resources {}", name);
            if current.contains_key(&resource_key) {
                skipped.push(name.clone());
                continue;
            }

            if context.check_mode {
                created.push(name.clone());
                continue;
            }

            let cmd = format!(
                "qmgr -c \"create resource {} type={}\"",
                name, resource_type
            );
            run_cmd_ok(connection, &cmd, context)?;
            created.push(name.clone());
        }

        if created.is_empty() {
            return Ok(
                ModuleOutput::ok("All resources already exist")
                    .with_data("skipped", serde_json::json!(skipped)),
            );
        }

        let msg = if context.check_mode {
            format!("Would create {} resource(s)", created.len())
        } else {
            format!("Created {} resource(s)", created.len())
        };

        Ok(ModuleOutput::changed(msg)
            .with_data("created", serde_json::json!(created))
            .with_data("skipped", serde_json::json!(skipped)))
    }
}

/// Parse `qmgr -c "print server"` text output into key-value pairs.
///
/// Lines look like:
///   set server scheduling = True
///   set server default_queue = batch
///   set server resources_default.walltime = 01:00:00
///
/// Comment lines (starting with #) and blank lines are skipped.
fn parse_qmgr_server_output(output: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in output.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Parse "set server key = value" lines
        if let Some(rest) = trimmed.strip_prefix("set server ") {
            if let Some((key, value)) = rest.split_once('=') {
                map.insert(
                    key.trim().to_string(),
                    value.trim().to_string(),
                );
            }
        }

        // Also handle "create resource name type=string" style lines
        if let Some(rest) = trimmed.strip_prefix("create resource ") {
            if let Some((name, _type_info)) = rest.split_once(' ') {
                map.insert(
                    format!("resources {}", name.trim()),
                    rest.trim().to_string(),
                );
            }
        }
    }
    map
}

/// Build server attribute key-value pairs from params.
fn build_server_attribute_pairs(
    params: &ModuleParams,
) -> ModuleResult<HashMap<String, String>> {
    let mut desired = HashMap::new();

    let attr_map: &[(&str, &str)] = &[
        ("default_queue", "default_queue"),
        ("scheduling", "scheduling"),
        ("node_fail_requeue", "node_fail_requeue"),
        ("max_run", "max_run"),
        ("max_queued", "max_queued"),
        ("query_other_jobs", "query_other_jobs"),
        ("resources_default_walltime", "resources_default.walltime"),
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

/// Compare desired attributes against current server state and return only differences.
fn compute_server_changes(
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
    fn test_parse_qmgr_server_output() {
        let output = r#"#
# Set server attributes.
#
set server scheduling = True
set server default_queue = batch
set server log_events = 511
set server resources_default.walltime = 01:00:00
set server node_fail_requeue = True
"#;
        let attrs = parse_qmgr_server_output(output);
        assert_eq!(attrs.get("scheduling"), Some(&"True".to_string()));
        assert_eq!(attrs.get("default_queue"), Some(&"batch".to_string()));
        assert_eq!(attrs.get("log_events"), Some(&"511".to_string()));
        assert_eq!(
            attrs.get("resources_default.walltime"),
            Some(&"01:00:00".to_string())
        );
        assert_eq!(attrs.get("node_fail_requeue"), Some(&"True".to_string()));
    }

    #[test]
    fn test_parse_qmgr_server_output_empty() {
        let attrs = parse_qmgr_server_output("");
        assert!(attrs.is_empty());
    }

    #[test]
    fn test_parse_qmgr_server_output_comments() {
        let output = "# This is a comment\n# Another comment\n";
        let attrs = parse_qmgr_server_output(output);
        assert!(attrs.is_empty());
    }

    #[test]
    fn test_build_server_attribute_commands() {
        let mut params = ModuleParams::new();
        params.insert(
            "default_queue".to_string(),
            serde_json::json!("batch"),
        );
        params.insert("scheduling".to_string(), serde_json::json!("True"));
        params.insert(
            "resources_default_walltime".to_string(),
            serde_json::json!("02:00:00"),
        );

        let pairs = build_server_attribute_pairs(&params).unwrap();
        assert_eq!(pairs.get("default_queue"), Some(&"batch".to_string()));
        assert_eq!(pairs.get("scheduling"), Some(&"True".to_string()));
        assert_eq!(
            pairs.get("resources_default.walltime"),
            Some(&"02:00:00".to_string())
        );
    }

    #[test]
    fn test_build_server_attribute_commands_empty() {
        let params = ModuleParams::new();
        let pairs = build_server_attribute_pairs(&params).unwrap();
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_compute_server_changes_no_changes() {
        let mut current = HashMap::new();
        current.insert("scheduling".to_string(), "True".to_string());
        current.insert("default_queue".to_string(), "batch".to_string());

        let mut desired = HashMap::new();
        desired.insert("scheduling".to_string(), "True".to_string());
        desired.insert("default_queue".to_string(), "batch".to_string());

        let changes = compute_server_changes(&current, &desired);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_compute_server_changes_with_changes() {
        let mut current = HashMap::new();
        current.insert("scheduling".to_string(), "True".to_string());
        current.insert("default_queue".to_string(), "batch".to_string());
        current.insert("log_events".to_string(), "511".to_string());

        let mut desired = HashMap::new();
        desired.insert("scheduling".to_string(), "False".to_string());
        desired.insert("default_queue".to_string(), "batch".to_string());
        desired.insert("log_events".to_string(), "255".to_string());

        let changes = compute_server_changes(&current, &desired);
        assert_eq!(changes.len(), 2);
        assert_eq!(changes.get("scheduling"), Some(&"False".to_string()));
        assert_eq!(changes.get("log_events"), Some(&"255".to_string()));
        assert!(!changes.contains_key("default_queue"));
    }

    #[test]
    fn test_compute_server_changes_new_attribute() {
        let current = HashMap::new();
        let mut desired = HashMap::new();
        desired.insert("max_run".to_string(), "100".to_string());

        let changes = compute_server_changes(&current, &desired);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes.get("max_run"), Some(&"100".to_string()));
    }
}
