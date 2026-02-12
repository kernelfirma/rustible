//! PBS Pro job management module
//!
//! Submit, cancel, hold, release, and query PBS jobs via qsub/qdel/qstat.
//!
//! # Parameters
//!
//! - `action` (required): "submit", "cancel", "status", "hold", or "release"
//! - `script` (optional): Inline job script content (for submit)
//! - `script_path` (optional): Path to job script file (for submit)
//! - `job_name` (optional): Job name (-N for qsub, used for idempotency)
//! - `queue` (optional): Target queue (-q)
//! - `nodes` (optional): Number of nodes (-l nodes=N)
//! - `ncpus` (optional): Number of CPUs per node (-l ncpus=N)
//! - `walltime` (optional): Wall time limit (-l walltime=HH:MM:SS)
//! - `output_path` (optional): stdout file path (-o)
//! - `error_path` (optional): stderr file path (-e)
//! - `extra_args` (optional): Additional qsub arguments as a string
//! - `job_id` (required for cancel/status/hold/release): Job ID to operate on
//! - `resource_list` (optional): Additional -l resource specifications

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

pub struct PbsJobModule;

impl Module for PbsJobModule {
    fn name(&self) -> &'static str {
        "pbs_job"
    }

    fn description(&self) -> &'static str {
        "Submit, cancel, hold, release, and query PBS Pro jobs (qsub/qdel/qstat)"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
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
            "submit" => self.action_submit(connection, params, context),
            "cancel" => self.action_cancel(connection, params, context),
            "status" => self.action_status(connection, params, context),
            "hold" => self.action_hold(connection, params, context),
            "release" => self.action_release(connection, params, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'submit', 'cancel', 'status', 'hold', or 'release'",
                action
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("script", serde_json::json!(null));
        m.insert("script_path", serde_json::json!(null));
        m.insert("job_name", serde_json::json!(null));
        m.insert("queue", serde_json::json!(null));
        m.insert("nodes", serde_json::json!(null));
        m.insert("ncpus", serde_json::json!(null));
        m.insert("walltime", serde_json::json!(null));
        m.insert("output_path", serde_json::json!(null));
        m.insert("error_path", serde_json::json!(null));
        m.insert("extra_args", serde_json::json!(null));
        m.insert("job_id", serde_json::json!(null));
        m.insert("resource_list", serde_json::json!(null));
        m
    }
}

impl PbsJobModule {
    fn action_submit(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
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

        // Build qsub command
        let cmd = build_qsub_command(params)?;
        let stdout = run_cmd_ok(connection, &cmd, context)?;

        // Parse job ID from qsub output
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

    fn action_cancel(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let job_id = params.get_string_required("job_id")?;

        // Check if job is in a terminal state or not found
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

        // Check job state — skip if terminal (F=Finished, X=Exiting)
        if let Some(state) = extract_job_state(&stdout, &job_id) {
            if state == "F" || state == "X" {
                return Ok(ModuleOutput::ok(format!(
                    "Job {} is already in terminal state '{}'",
                    job_id, state
                ))
                .with_data("job_id", serde_json::json!(job_id))
                .with_data("state", serde_json::json!(state)));
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

    fn action_status(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let job_id = params.get_string_required("job_id")?;

        let stdout = run_cmd_ok(
            connection,
            &format!("qstat -f -F json {} 2>/dev/null", job_id),
            context,
        )?;

        let jobs = parse_pbs_json_jobs(&stdout);

        if jobs.is_null() || jobs.as_object().is_none_or(|o| o.is_empty()) {
            return Err(ModuleError::ExecutionFailed(format!(
                "Job {} not found",
                job_id
            )));
        }

        Ok(ModuleOutput::ok(format!("Job {} status retrieved", job_id))
            .with_data("jobs", jobs)
            .with_data("job_id", serde_json::json!(job_id)))
    }

    fn action_hold(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let job_id = params.get_string_required("job_id")?;

        // Check current hold state
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("qstat -f -F json {} 2>/dev/null", job_id),
            context,
        )?;

        if !ok || stdout.trim().is_empty() {
            return Err(ModuleError::ExecutionFailed(format!(
                "Job {} not found",
                job_id
            )));
        }

        // Check Hold_Types — skip if already held
        if let Some(hold_types) = extract_job_attribute(&stdout, &job_id, "Hold_Types") {
            if hold_types != "n" && !hold_types.is_empty() {
                return Ok(ModuleOutput::ok(format!(
                    "Job {} is already held (Hold_Types={})",
                    job_id, hold_types
                ))
                .with_data("job_id", serde_json::json!(job_id))
                .with_data("hold_types", serde_json::json!(hold_types)));
            }
        }

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

    fn action_release(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let job_id = params.get_string_required("job_id")?;

        // Check current hold state
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("qstat -f -F json {} 2>/dev/null", job_id),
            context,
        )?;

        if !ok || stdout.trim().is_empty() {
            return Err(ModuleError::ExecutionFailed(format!(
                "Job {} not found",
                job_id
            )));
        }

        // Check Hold_Types — skip if no hold
        if let Some(hold_types) = extract_job_attribute(&stdout, &job_id, "Hold_Types") {
            if hold_types == "n" || hold_types.is_empty() {
                return Ok(ModuleOutput::ok(format!(
                    "Job {} is not held (Hold_Types={})",
                    job_id, hold_types
                ))
                .with_data("job_id", serde_json::json!(job_id))
                .with_data("hold_types", serde_json::json!(hold_types)));
            }
        }

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
}

/// Build a qsub command from parameters.
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

    // Build resource list items
    let mut resources = Vec::new();
    if let Some(nodes) = params.get_string("nodes")? {
        resources.push(format!("nodes={}", nodes));
    }
    if let Some(ncpus) = params.get_string("ncpus")? {
        resources.push(format!("ncpus={}", ncpus));
    }
    if let Some(walltime) = params.get_string("walltime")? {
        resources.push(format!("walltime={}", walltime));
    }
    if let Some(resource_list) = params.get_string("resource_list")? {
        resources.push(resource_list);
    }
    if !resources.is_empty() {
        args.push(format!("-l {}", resources.join(",")));
    }

    if let Some(output_path) = params.get_string("output_path")? {
        args.push(format!("-o {}", output_path));
    }
    if let Some(error_path) = params.get_string("error_path")? {
        args.push(format!("-e {}", error_path));
    }
    if let Some(extra) = params.get_string("extra_args")? {
        args.push(extra);
    }

    let args_str = args.join(" ");

    if let Some(path) = script_path {
        Ok(format!("qsub {} {}", args_str, path).trim().to_string())
    } else {
        let script_content = script.unwrap();
        // Pipe inline script via heredoc
        let escaped = script_content.replace('\'', "'\\''");
        Ok(format!(
            "echo '{}' | qsub {}",
            escaped, args_str
        )
        .trim()
        .to_string())
    }
}

/// Parse qsub output to extract job ID.
/// Expected format: "12345.server" or just "12345"
fn parse_qsub_output(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    // qsub typically outputs just the job ID on stdout
    // e.g., "12345.pbs-server" or "12345.hostname"
    let first_line = trimmed.lines().next()?;
    let first_line = first_line.trim();
    if first_line.is_empty() {
        return None;
    }
    // Validate it looks like a job ID (starts with a digit or contains a dot)
    if first_line.chars().next()?.is_ascii_digit() {
        Some(first_line.to_string())
    } else {
        None
    }
}

/// Parse PBS JSON output from `qstat -f -F json` into the Jobs object.
fn parse_pbs_json_jobs(output: &str) -> serde_json::Value {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return serde_json::Value::Null;
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(parsed) => {
            // PBS JSON structure: { "Jobs": { "jobid": { ... }, ... } }
            if let Some(jobs) = parsed.get("Jobs") {
                jobs.clone()
            } else {
                parsed
            }
        }
        Err(_) => serde_json::Value::Null,
    }
}

/// Find an active job by Job_Name in qstat JSON output.
/// Returns the job ID if found with a non-terminal state.
fn find_active_job_by_name(output: &str, name: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(output.trim()).ok()?;
    let jobs = parsed.get("Jobs")?.as_object()?;
    for (job_id, job_info) in jobs {
        let job_name = job_info.get("Job_Name")?.as_str()?;
        if job_name != name {
            continue;
        }
        // Check job state — active means not F (Finished) or X (Exiting)
        if let Some(state) = job_info.get("job_state").and_then(|s| s.as_str()) {
            if state != "F" && state != "X" {
                return Some(job_id.clone());
            }
        }
    }
    None
}

/// Extract a job attribute from qstat JSON output.
fn extract_job_attribute(output: &str, job_id: &str, attribute: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(output.trim()).ok()?;
    let jobs = parsed.get("Jobs")?.as_object()?;
    // Try exact job_id match first, then prefix match
    let job_info = jobs.get(job_id).or_else(|| {
        jobs.iter()
            .find(|(k, _)| k.starts_with(&format!("{}.", job_id)) || job_id.starts_with(&format!("{}.", k)))
            .map(|(_, v)| v)
    })?;
    job_info
        .get(attribute)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extract job state from qstat JSON output.
fn extract_job_state(output: &str, job_id: &str) -> Option<String> {
    extract_job_attribute(output, job_id, "job_state")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_qsub_output() {
        assert_eq!(
            parse_qsub_output("12345.pbs-server\n"),
            Some("12345.pbs-server".to_string())
        );
    }

    #[test]
    fn test_parse_qsub_output_bare_id() {
        assert_eq!(
            parse_qsub_output("12345\n"),
            Some("12345".to_string())
        );
    }

    #[test]
    fn test_parse_qsub_output_empty() {
        assert_eq!(parse_qsub_output(""), None);
        assert_eq!(parse_qsub_output("  \n  "), None);
    }

    #[test]
    fn test_parse_qsub_output_error() {
        assert_eq!(parse_qsub_output("qsub: Job rejected\n"), None);
    }

    #[test]
    fn test_build_qsub_command_script_path() {
        let mut params = ModuleParams::new();
        params.insert("script_path".to_string(), serde_json::json!("/tmp/job.sh"));
        params.insert("job_name".to_string(), serde_json::json!("test_job"));
        params.insert("queue".to_string(), serde_json::json!("batch"));
        params.insert("walltime".to_string(), serde_json::json!("02:00:00"));

        let cmd = build_qsub_command(&params).unwrap();
        assert!(cmd.contains("qsub"));
        assert!(cmd.contains("-N test_job"));
        assert!(cmd.contains("-q batch"));
        assert!(cmd.contains("-l walltime=02:00:00"));
        assert!(cmd.contains("/tmp/job.sh"));
    }

    #[test]
    fn test_build_qsub_command_inline_script() {
        let mut params = ModuleParams::new();
        params.insert(
            "script".to_string(),
            serde_json::json!("#!/bin/bash\necho hello"),
        );
        params.insert("job_name".to_string(), serde_json::json!("inline_job"));

        let cmd = build_qsub_command(&params).unwrap();
        assert!(cmd.contains("qsub"));
        assert!(cmd.contains("-N inline_job"));
        assert!(cmd.contains("echo hello"));
    }

    #[test]
    fn test_build_qsub_command_no_script() {
        let params = ModuleParams::new();
        let result = build_qsub_command(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_qsub_command_all_params() {
        let mut params = ModuleParams::new();
        params.insert("script_path".to_string(), serde_json::json!("/job.sh"));
        params.insert("job_name".to_string(), serde_json::json!("full_job"));
        params.insert("queue".to_string(), serde_json::json!("gpu"));
        params.insert("nodes".to_string(), serde_json::json!("4"));
        params.insert("ncpus".to_string(), serde_json::json!("32"));
        params.insert("walltime".to_string(), serde_json::json!("04:00:00"));
        params.insert(
            "output_path".to_string(),
            serde_json::json!("/logs/out.log"),
        );
        params.insert(
            "error_path".to_string(),
            serde_json::json!("/logs/err.log"),
        );

        let cmd = build_qsub_command(&params).unwrap();
        assert!(cmd.contains("-N full_job"));
        assert!(cmd.contains("-q gpu"));
        assert!(cmd.contains("nodes=4"));
        assert!(cmd.contains("ncpus=32"));
        assert!(cmd.contains("walltime=04:00:00"));
        assert!(cmd.contains("-o /logs/out.log"));
        assert!(cmd.contains("-e /logs/err.log"));
    }

    #[test]
    fn test_build_qsub_command_extra_args() {
        let mut params = ModuleParams::new();
        params.insert("script_path".to_string(), serde_json::json!("/job.sh"));
        params.insert(
            "extra_args".to_string(),
            serde_json::json!("-V -m abe"),
        );

        let cmd = build_qsub_command(&params).unwrap();
        assert!(cmd.contains("-V -m abe"));
        assert!(cmd.contains("/job.sh"));
    }

    #[test]
    fn test_parse_pbs_json_jobs() {
        let json = r#"{
            "Jobs": {
                "12345.server": {
                    "Job_Name": "test",
                    "job_state": "R",
                    "queue": "batch"
                }
            }
        }"#;
        let jobs = parse_pbs_json_jobs(json);
        assert!(jobs.is_object());
        assert!(jobs.get("12345.server").is_some());
        assert_eq!(jobs["12345.server"]["Job_Name"], "test");
    }

    #[test]
    fn test_parse_pbs_json_jobs_empty() {
        let jobs = parse_pbs_json_jobs("");
        assert!(jobs.is_null());
    }

    #[test]
    fn test_parse_pbs_json_jobs_malformed() {
        let jobs = parse_pbs_json_jobs("not json at all {{{");
        assert!(jobs.is_null());
    }
}
