//! PBS Pro scheduler backend for the unified HPC abstraction layer.
//!
//! Implements [`HpcScheduler`] by calling PBS CLI commands (`qsub`, `qdel`,
//! `qstat`, `qhold`, `qrls`, `qmgr`).

use std::collections::HashMap;

use crate::modules::{
    ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
};

use super::scheduler::{
    run_cmd, run_cmd_ok, HpcScheduler, JobInfo, JobState, QueueInfo, ServerInfo,
    map_pbs_state,
};

/// PBS Pro backend for the unified HPC scheduler abstraction.
pub struct PbsScheduler;

impl HpcScheduler for PbsScheduler {
    fn scheduler_name(&self) -> &'static str {
        "pbs"
    }

    fn submit_job(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let job_name = params.get_string("job_name")?;

        // Idempotency: check if a job with this name is already active
        if let Some(ref name) = job_name {
            let (ok, stdout, _) = run_cmd(
                connection,
                "qstat -f -F json 2>/dev/null",
                context,
            )?;
            if ok && !stdout.trim().is_empty() {
                if let Some(existing_id) = find_active_job_by_name(&stdout, name) {
                    return Ok(ModuleOutput::ok(format!(
                        "Job '{}' is already active (job_id={})",
                        name, existing_id
                    ))
                    .with_data("job_id", serde_json::json!(existing_id))
                    .with_data("job_name", serde_json::json!(name))
                    .with_data("already_active", serde_json::json!(true)));
                }
            }
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would submit PBS job").with_data(
                "job_name",
                serde_json::json!(job_name.as_deref().unwrap_or("")),
            ));
        }

        let cmd = build_qsub_command(params)?;
        let stdout = run_cmd_ok(connection, &cmd, context)?;

        let job_id = parse_qsub_output(&stdout).ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "Could not parse job ID from qsub output: {}",
                stdout.trim()
            ))
        })?;

        Ok(ModuleOutput::changed(format!("Submitted job {}", job_id))
            .with_data("job_id", serde_json::json!(job_id))
            .with_data(
                "job_name",
                serde_json::json!(job_name.as_deref().unwrap_or("")),
            ))
    }

    fn cancel_job(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("qstat -f -F json {} 2>/dev/null", job_id),
            context,
        )?;

        if !ok || stdout.trim().is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "Job {} is not found (already completed or unknown)",
                job_id
            ))
            .with_data("job_id", serde_json::json!(job_id)));
        }

        // Check if already in terminal state
        if let Some(state) = extract_job_state(&stdout, job_id) {
            if state == "F" || state == "X" {
                return Ok(ModuleOutput::ok(format!(
                    "Job {} is already in terminal state '{}'",
                    job_id, state
                ))
                .with_data("job_id", serde_json::json!(job_id)));
            }
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would cancel job {}", job_id))
                    .with_data("job_id", serde_json::json!(job_id)),
            );
        }

        run_cmd_ok(connection, &format!("qdel {}", job_id), context)?;

        Ok(
            ModuleOutput::changed(format!("Cancelled job {}", job_id))
                .with_data("job_id", serde_json::json!(job_id)),
        )
    }

    fn job_status(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<JobInfo> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let stdout = run_cmd_ok(
            connection,
            &format!("qstat -f -F json {} 2>/dev/null", job_id),
            context,
        )?;

        let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to parse qstat JSON: {}", e)))?;

        let jobs = parsed
            .get("Jobs")
            .and_then(|j| j.as_object())
            .ok_or_else(|| {
                ModuleError::ExecutionFailed(format!("Job {} not found", job_id))
            })?;

        // Find the job (try exact match first, then prefix match)
        let (found_id, job_info) = jobs
            .get(job_id)
            .map(|v| (job_id.to_string(), v))
            .or_else(|| {
                jobs.iter()
                    .find(|(k, _)| {
                        k.starts_with(&format!("{}.", job_id))
                            || job_id.starts_with(&format!("{}.", k))
                    })
                    .map(|(k, v)| (k.clone(), v))
            })
            .ok_or_else(|| {
                ModuleError::ExecutionFailed(format!("Job {} not found", job_id))
            })?;

        let state_str = job_info
            .get("job_state")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mut state = map_pbs_state(state_str);

        // Refine "F" state: check exit_status to distinguish Completed vs Failed
        if state == JobState::Completed {
            if let Some(exit) = job_info.get("Exit_status").and_then(|v| v.as_i64()) {
                if exit != 0 {
                    state = JobState::Failed;
                }
            }
        }

        Ok(JobInfo {
            id: found_id,
            name: job_info
                .get("Job_Name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            state,
            queue: job_info
                .get("queue")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            owner: job_info
                .get("Job_Owner")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            nodes: job_info
                .get("Resource_List")
                .and_then(|r| r.get("nodect"))
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok()),
            cpus: job_info
                .get("Resource_List")
                .and_then(|r| r.get("ncpus"))
                .and_then(|v| v.as_u64())
                .and_then(|v| u32::try_from(v).ok()),
            walltime_limit: job_info
                .get("Resource_List")
                .and_then(|r| r.get("walltime"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            walltime_used: job_info
                .get("resources_used")
                .and_then(|r| r.get("walltime"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            raw: job_info.clone(),
        })
    }

    fn hold_job(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would hold job {}", job_id))
                    .with_data("job_id", serde_json::json!(job_id)),
            );
        }

        run_cmd_ok(connection, &format!("qhold {}", job_id), context)?;

        Ok(
            ModuleOutput::changed(format!("Held job {}", job_id))
                .with_data("job_id", serde_json::json!(job_id)),
        )
    }

    fn release_job(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would release job {}", job_id))
                    .with_data("job_id", serde_json::json!(job_id)),
            );
        }

        run_cmd_ok(connection, &format!("qrls {}", job_id), context)?;

        Ok(
            ModuleOutput::changed(format!("Released job {}", job_id))
                .with_data("job_id", serde_json::json!(job_id)),
        )
    }

    fn list_queues(&self, context: &ModuleContext) -> ModuleResult<Vec<QueueInfo>> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let stdout = run_cmd_ok(
            connection,
            "qstat -Q -f -F json 2>/dev/null",
            context,
        )?;

        let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
            .unwrap_or(serde_json::Value::Null);

        let mut queues = Vec::new();
        if let Some(queue_obj) = parsed.get("Queue").and_then(|q| q.as_object()) {
            for (name, info) in queue_obj {
                let enabled = info
                    .get("enabled")
                    .and_then(|v| v.as_str())
                    .unwrap_or("False");
                let started = info
                    .get("started")
                    .and_then(|v| v.as_str())
                    .unwrap_or("False");
                let state = if enabled == "True" && started == "True" {
                    "active".to_string()
                } else {
                    "inactive".to_string()
                };
                let total_jobs = info
                    .get("total_jobs")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u32::try_from(v).ok());

                queues.push(QueueInfo {
                    name: name.clone(),
                    state,
                    total_jobs,
                    raw: info.clone(),
                });
            }
        }

        Ok(queues)
    }

    fn create_queue(
        &self,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        // Idempotency: check if queue exists
        let (ok, stdout, _) = run_cmd(
            connection,
            "qstat -Q -f -F json 2>/dev/null",
            context,
        )?;
        if ok && !stdout.trim().is_empty() {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                if let Some(queues) = parsed.get("Queue").and_then(|q| q.as_object()) {
                    if queues.contains_key(name) {
                        return Ok(
                            ModuleOutput::ok(format!("Queue '{}' already exists", name))
                                .with_data("name", serde_json::json!(name)),
                        );
                    }
                }
            }
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

        run_cmd_ok(
            connection,
            &format!("qmgr -c \"create queue {} queue_type={}\"", name, queue_type),
            context,
        )?;

        // Apply optional attributes
        if let Some(enabled) = params.get_string("enabled")? {
            run_cmd_ok(
                connection,
                &format!("qmgr -c \"set queue {} enabled={}\"", name, enabled),
                context,
            )?;
        }
        if let Some(started) = params.get_string("started")? {
            run_cmd_ok(
                connection,
                &format!("qmgr -c \"set queue {} started={}\"", name, started),
                context,
            )?;
        }

        Ok(
            ModuleOutput::changed(format!("Created queue '{}'", name))
                .with_data("name", serde_json::json!(name)),
        )
    }

    fn delete_queue(&self, name: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        // Idempotency: check if queue exists
        let (ok, stdout, _) = run_cmd(
            connection,
            "qstat -Q -f -F json 2>/dev/null",
            context,
        )?;
        if ok && !stdout.trim().is_empty() {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                if let Some(queues) = parsed.get("Queue").and_then(|q| q.as_object()) {
                    if !queues.contains_key(name) {
                        return Ok(
                            ModuleOutput::ok(format!("Queue '{}' does not exist", name))
                                .with_data("name", serde_json::json!(name)),
                        );
                    }
                }
            }
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

    fn query_server(&self, context: &ModuleContext) -> ModuleResult<ServerInfo> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let stdout = run_cmd_ok(
            connection,
            "qmgr -c \"print server\" 2>/dev/null",
            context,
        )?;

        let attributes = parse_qmgr_server_output(&stdout);

        Ok(ServerInfo {
            scheduler: "pbs".to_string(),
            attributes: attributes.clone(),
            raw: serde_json::json!(attributes),
        })
    }

    fn set_server_attributes(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        // Get attributes from params
        let attrs = match params.get("attributes") {
            Some(serde_json::Value::Object(obj)) => obj.clone(),
            _ => {
                return Err(ModuleError::MissingParameter(
                    "attributes (JSON object) is required for set_attributes".to_string(),
                ));
            }
        };

        if attrs.is_empty() {
            return Ok(ModuleOutput::ok("No attributes to set"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would set {} server attribute(s)",
                attrs.len()
            ))
            .with_data("attributes", serde_json::json!(attrs)));
        }

        for (key, value) in &attrs {
            let val_str = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            run_cmd_ok(
                connection,
                &format!("qmgr -c \"set server {}={}\"", key, val_str),
                context,
            )?;
        }

        Ok(ModuleOutput::changed(format!(
            "Set {} server attribute(s)",
            attrs.len()
        ))
        .with_data("attributes", serde_json::json!(attrs)))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a qsub command from unified parameters.
fn build_qsub_command(params: &ModuleParams) -> ModuleResult<String> {
    let script = params.get_string("script")?;
    let script_path = params.get_string("script_path")?;

    if script.is_none() && script_path.is_none() {
        return Err(ModuleError::MissingParameter(
            "Either 'script' or 'script_path' is required for submit".to_string(),
        ));
    }

    let mut args = Vec::new();

    if let Some(name) = params.get_string("job_name")? {
        args.push(format!("-N {}", name));
    }
    if let Some(queue) = params.get_string("queue")? {
        args.push(format!("-q {}", queue));
    }

    let mut resources = Vec::new();
    if let Some(nodes) = params.get_string("nodes")? {
        resources.push(format!("nodes={}", nodes));
    }
    if let Some(cpus) = params.get_string("cpus")? {
        resources.push(format!("ncpus={}", cpus));
    }
    if let Some(walltime) = params.get_string("walltime")? {
        resources.push(format!("walltime={}", walltime));
    }
    if !resources.is_empty() {
        args.push(format!("-l {}", resources.join(",")));
    }

    if let Some(output) = params.get_string("output_path")? {
        args.push(format!("-o {}", output));
    }
    if let Some(error) = params.get_string("error_path")? {
        args.push(format!("-e {}", error));
    }
    if let Some(extra) = params.get_string("extra_args")? {
        args.push(extra);
    }

    let args_str = args.join(" ");

    if let Some(path) = script_path {
        Ok(format!("qsub {} {}", args_str, path).trim().to_string())
    } else {
        let script_content = script.unwrap();
        let escaped = script_content.replace('\'', "'\\''");
        Ok(format!("echo '{}' | qsub {}", escaped, args_str)
            .trim()
            .to_string())
    }
}

/// Parse qsub output to extract job ID.
fn parse_qsub_output(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let first_line = trimmed.lines().next()?.trim();
    if first_line.is_empty() {
        return None;
    }
    if first_line.chars().next()?.is_ascii_digit() {
        Some(first_line.to_string())
    } else {
        None
    }
}

/// Find an active job by Job_Name in qstat JSON output.
fn find_active_job_by_name(output: &str, name: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(output.trim()).ok()?;
    let jobs = parsed.get("Jobs")?.as_object()?;
    for (job_id, job_info) in jobs {
        let job_name = job_info.get("Job_Name")?.as_str()?;
        if job_name != name {
            continue;
        }
        if let Some(state) = job_info.get("job_state").and_then(|s| s.as_str()) {
            if state != "F" && state != "X" {
                return Some(job_id.clone());
            }
        }
    }
    None
}

/// Extract job state from qstat JSON output.
fn extract_job_state(output: &str, job_id: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(output.trim()).ok()?;
    let jobs = parsed.get("Jobs")?.as_object()?;
    let job_info = jobs.get(job_id).or_else(|| {
        jobs.iter()
            .find(|(k, _)| {
                k.starts_with(&format!("{}.", job_id))
                    || job_id.starts_with(&format!("{}.", k))
            })
            .map(|(_, v)| v)
    })?;
    job_info
        .get("job_state")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Parse `qmgr -c "print server"` output into key-value pairs.
fn parse_qmgr_server_output(output: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("set server ") {
            if let Some((key, value)) = rest.split_once('=') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pbs_scheduler_name() {
        let s = PbsScheduler;
        assert_eq!(s.scheduler_name(), "pbs");
    }

    #[test]
    fn test_pbs_build_qsub_command_basic() {
        let mut params = ModuleParams::new();
        params.insert("script_path".to_string(), serde_json::json!("/tmp/job.sh"));
        params.insert("job_name".to_string(), serde_json::json!("test_job"));
        params.insert("queue".to_string(), serde_json::json!("batch"));
        params.insert("walltime".to_string(), serde_json::json!("02:00:00"));
        params.insert("nodes".to_string(), serde_json::json!("4"));

        let cmd = build_qsub_command(&params).unwrap();
        assert!(cmd.contains("qsub"));
        assert!(cmd.contains("-N test_job"));
        assert!(cmd.contains("-q batch"));
        assert!(cmd.contains("walltime=02:00:00"));
        assert!(cmd.contains("nodes=4"));
        assert!(cmd.contains("/tmp/job.sh"));
    }

    #[test]
    fn test_pbs_build_qsub_command_no_script() {
        let params = ModuleParams::new();
        let result = build_qsub_command(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_pbs_parse_qsub_output() {
        assert_eq!(
            parse_qsub_output("12345.pbs-server\n"),
            Some("12345.pbs-server".to_string())
        );
        assert_eq!(parse_qsub_output(""), None);
        assert_eq!(parse_qsub_output("qsub: error\n"), None);
    }

    #[test]
    fn test_pbs_parse_qmgr_server_output() {
        let output = r#"#
# Set server attributes.
#
set server scheduling = True
set server default_queue = batch
"#;
        let attrs = parse_qmgr_server_output(output);
        assert_eq!(attrs.get("scheduling"), Some(&"True".to_string()));
        assert_eq!(attrs.get("default_queue"), Some(&"batch".to_string()));
    }
}
