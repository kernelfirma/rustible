//! Slurm job management module
//!
//! Submit, cancel, and query Slurm jobs via sbatch/scancel/squeue/sacct.
//!
//! # Parameters
//!
//! - `action` (required): "submit", "cancel", or "status"
//! - `script` (optional): Inline job script content (for submit)
//! - `script_path` (optional): Path to job script file (for submit)
//! - `job_name` (optional): Job name (--job-name for sbatch, used for idempotency)
//! - `partition` (optional): Target partition (--partition)
//! - `nodes` (optional): Number of nodes (--nodes)
//! - `ntasks` (optional): Number of tasks (--ntasks)
//! - `time_limit` (optional): Wall time limit (--time)
//! - `output` (optional): stdout file path (--output)
//! - `error` (optional): stderr file path (--error)
//! - `extra_args` (optional): Additional sbatch arguments as a string
//! - `job_id` (required for cancel/status): Job ID to operate on
//! - `signal` (optional): Signal to send on cancel (e.g. "SIGTERM")

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

pub struct SlurmJobModule;

impl Module for SlurmJobModule {
    fn name(&self) -> &'static str {
        "slurm_job"
    }

    fn description(&self) -> &'static str {
        "Submit, cancel, and query Slurm jobs (sbatch/scancel/squeue/sacct)"
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
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'submit', 'cancel', or 'status'",
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
        m.insert("partition", serde_json::json!(null));
        m.insert("nodes", serde_json::json!(null));
        m.insert("ntasks", serde_json::json!(null));
        m.insert("time_limit", serde_json::json!(null));
        m.insert("output", serde_json::json!(null));
        m.insert("error", serde_json::json!(null));
        m.insert("extra_args", serde_json::json!(null));
        m.insert("job_id", serde_json::json!(null));
        m.insert("signal", serde_json::json!(null));
        m
    }
}

impl SlurmJobModule {
    fn action_submit(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let job_name = params.get_string("job_name")?;

        // Idempotency: check if a job with this name is already pending/running
        if let Some(ref name) = job_name {
            let (ok, stdout, _) = run_cmd(
                connection,
                &format!("squeue --noheader --name={} -o '%i|%T' 2>/dev/null", name),
                context,
            )?;
            if ok && !stdout.trim().is_empty() {
                let active_jobs: Vec<&str> = stdout
                    .lines()
                    .filter(|l| {
                        let parts: Vec<&str> = l.split('|').collect();
                        parts.len() >= 2
                            && (parts[1].contains("PENDING") || parts[1].contains("RUNNING"))
                    })
                    .collect();
                if !active_jobs.is_empty() {
                    let first_id = active_jobs[0].split('|').next().unwrap_or("").trim();
                    return Ok(ModuleOutput::ok(format!(
                        "Job '{}' is already active (job_id={})",
                        name, first_id
                    ))
                    .with_data("job_id", serde_json::json!(first_id))
                    .with_data("job_name", serde_json::json!(name))
                    .with_data("already_active", serde_json::json!(true)));
                }
            }
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would submit Slurm job").with_data(
                "job_name",
                serde_json::json!(job_name.as_deref().unwrap_or("")),
            ));
        }

        // Build sbatch command
        let cmd = build_sbatch_command(params)?;
        let stdout = run_cmd_ok(connection, &cmd, context)?;

        // Parse "Submitted batch job <ID>"
        let job_id = parse_sbatch_output(&stdout).ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "Could not parse job ID from sbatch output: {}",
                stdout.trim()
            ))
        })?;

        Ok(
            ModuleOutput::changed(format!("Submitted batch job {}", job_id))
                .with_data("job_id", serde_json::json!(job_id))
                .with_data(
                    "job_name",
                    serde_json::json!(job_name.as_deref().unwrap_or("")),
                ),
        )
    }

    fn action_cancel(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let job_id = params.get_string_required("job_id")?;

        // Check if job is still active
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("squeue --noheader -j {} -o '%T' 2>/dev/null", job_id),
            context,
        )?;

        if !ok || stdout.trim().is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "Job {} is not active (already completed or unknown)",
                job_id
            ))
            .with_data("job_id", serde_json::json!(job_id)));
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would cancel job {}", job_id))
                    .with_data("job_id", serde_json::json!(job_id)),
            );
        }

        let mut cmd = format!("scancel {}", job_id);
        if let Some(signal) = params.get_string("signal")? {
            cmd = format!("scancel --signal={} {}", signal, job_id);
        }

        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!("Cancelled job {}", job_id))
            .with_data("job_id", serde_json::json!(job_id)))
    }

    fn action_status(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let job_id = params.get_string_required("job_id")?;

        // Try squeue first (active jobs)
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!(
                "squeue --noheader -j {} -o '%i|%j|%u|%T|%P|%D|%C|%l|%M|%R' 2>/dev/null",
                job_id
            ),
            context,
        )?;

        if ok && !stdout.trim().is_empty() {
            let fields = [
                "job_id",
                "name",
                "user",
                "state",
                "partition",
                "nodes",
                "cpus",
                "time_limit",
                "time_used",
                "reason",
            ];
            let jobs = parse_pipe_delimited(&stdout, &fields);
            if let Some(job) = jobs.into_iter().next() {
                return Ok(ModuleOutput::ok(format!("Job {} is active", job_id))
                    .with_data("job", job)
                    .with_data("source", serde_json::json!("squeue")));
            }
        }

        // Fallback to sacct for completed jobs
        let (ok2, stdout2, _) = run_cmd(
            connection,
            &format!(
                "sacct --noheader --parsable2 -j {} --format=JobID,JobName,User,State,Partition,NNodes,NCPUs,Timelimit,Elapsed,ExitCode 2>/dev/null",
                job_id
            ),
            context,
        )?;

        if ok2 && !stdout2.trim().is_empty() {
            let fields = [
                "job_id",
                "name",
                "user",
                "state",
                "partition",
                "nodes",
                "cpus",
                "time_limit",
                "elapsed",
                "exit_code",
            ];
            let jobs = parse_pipe_delimited(&stdout2, &fields);
            if let Some(job) = jobs.into_iter().next() {
                return Ok(
                    ModuleOutput::ok(format!("Job {} found in accounting", job_id))
                        .with_data("job", job)
                        .with_data("source", serde_json::json!("sacct")),
                );
            }
        }

        Err(ModuleError::ExecutionFailed(format!(
            "Job {} not found in squeue or sacct",
            job_id
        )))
    }
}

/// Build an sbatch command from parameters.
fn build_sbatch_command(params: &ModuleParams) -> ModuleResult<String> {
    let script = params.get_string("script")?;
    let script_path = params.get_string("script_path")?;

    if script.is_none() && script_path.is_none() {
        return Err(ModuleError::MissingParameter(
            "Either 'script' or 'script_path' is required for submit".to_string(),
        ));
    }

    let mut args = Vec::new();

    if let Some(name) = params.get_string("job_name")? {
        args.push(format!("--job-name={}", name));
    }
    if let Some(partition) = params.get_string("partition")? {
        args.push(format!("--partition={}", partition));
    }
    if let Some(nodes) = params.get_string("nodes")? {
        args.push(format!("--nodes={}", nodes));
    }
    if let Some(ntasks) = params.get_string("ntasks")? {
        args.push(format!("--ntasks={}", ntasks));
    }
    if let Some(time_limit) = params.get_string("time_limit")? {
        args.push(format!("--time={}", time_limit));
    }
    if let Some(output) = params.get_string("output")? {
        args.push(format!("--output={}", output));
    }
    if let Some(error) = params.get_string("error")? {
        args.push(format!("--error={}", error));
    }
    if let Some(extra) = params.get_string("extra_args")? {
        args.push(extra);
    }

    let args_str = args.join(" ");

    if let Some(path) = script_path {
        Ok(format!("sbatch {} {}", args_str, path).trim().to_string())
    } else {
        let script_content = script.unwrap();
        // Use heredoc for inline script
        let escaped = script_content.replace('\'', "'\\''");
        Ok(format!("sbatch {} --wrap '{}'", args_str, escaped)
            .trim()
            .to_string())
    }
}

/// Parse sbatch output to extract job ID.
/// Expected format: "Submitted batch job 12345"
fn parse_sbatch_output(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Submitted batch job ") {
            return trimmed
                .strip_prefix("Submitted batch job ")
                .map(|s| s.trim().to_string());
        }
    }
    None
}

/// Generic pipe-delimited output parser.
fn parse_pipe_delimited(output: &str, fields: &[&str]) -> Vec<serde_json::Value> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() < fields.len() {
                return None;
            }
            let mut map = serde_json::Map::new();
            for (i, &field) in fields.iter().enumerate() {
                map.insert(
                    field.to_string(),
                    serde_json::Value::String(parts[i].trim().to_string()),
                );
            }
            Some(serde_json::Value::Object(map))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sbatch_output() {
        assert_eq!(
            parse_sbatch_output("Submitted batch job 12345\n"),
            Some("12345".to_string())
        );
    }

    #[test]
    fn test_parse_sbatch_output_with_prefix() {
        let output = "Some warning message\nSubmitted batch job 99999\n";
        assert_eq!(parse_sbatch_output(output), Some("99999".to_string()));
    }

    #[test]
    fn test_parse_sbatch_output_empty() {
        assert_eq!(parse_sbatch_output(""), None);
    }

    #[test]
    fn test_parse_sbatch_output_no_match() {
        assert_eq!(parse_sbatch_output("Error: invalid job\n"), None);
    }

    #[test]
    fn test_build_sbatch_command_script_path() {
        let mut params = ModuleParams::new();
        params.insert("script_path".to_string(), serde_json::json!("/tmp/job.sh"));
        params.insert("job_name".to_string(), serde_json::json!("test_job"));
        params.insert("partition".to_string(), serde_json::json!("compute"));
        params.insert("nodes".to_string(), serde_json::json!("4"));
        params.insert("time_limit".to_string(), serde_json::json!("2:00:00"));

        let cmd = build_sbatch_command(&params).unwrap();
        assert!(cmd.contains("sbatch"));
        assert!(cmd.contains("--job-name=test_job"));
        assert!(cmd.contains("--partition=compute"));
        assert!(cmd.contains("--nodes=4"));
        assert!(cmd.contains("--time=2:00:00"));
        assert!(cmd.contains("/tmp/job.sh"));
    }

    #[test]
    fn test_build_sbatch_command_inline_script() {
        let mut params = ModuleParams::new();
        params.insert(
            "script".to_string(),
            serde_json::json!("#!/bin/bash\necho hello"),
        );
        params.insert("job_name".to_string(), serde_json::json!("inline_job"));

        let cmd = build_sbatch_command(&params).unwrap();
        assert!(cmd.contains("sbatch"));
        assert!(cmd.contains("--job-name=inline_job"));
        assert!(cmd.contains("--wrap"));
    }

    #[test]
    fn test_build_sbatch_command_no_script() {
        let params = ModuleParams::new();
        let result = build_sbatch_command(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_build_sbatch_command_extra_args() {
        let mut params = ModuleParams::new();
        params.insert("script_path".to_string(), serde_json::json!("/job.sh"));
        params.insert(
            "extra_args".to_string(),
            serde_json::json!("--mem=4G --gres=gpu:1"),
        );

        let cmd = build_sbatch_command(&params).unwrap();
        assert!(cmd.contains("--mem=4G --gres=gpu:1"));
        assert!(cmd.contains("/job.sh"));
    }

    #[test]
    fn test_build_sbatch_command_all_params() {
        let mut params = ModuleParams::new();
        params.insert("script_path".to_string(), serde_json::json!("/job.sh"));
        params.insert("job_name".to_string(), serde_json::json!("full_job"));
        params.insert("partition".to_string(), serde_json::json!("gpu"));
        params.insert("nodes".to_string(), serde_json::json!("2"));
        params.insert("ntasks".to_string(), serde_json::json!("8"));
        params.insert("time_limit".to_string(), serde_json::json!("4:00:00"));
        params.insert("output".to_string(), serde_json::json!("/logs/out_%j.log"));
        params.insert("error".to_string(), serde_json::json!("/logs/err_%j.log"));

        let cmd = build_sbatch_command(&params).unwrap();
        assert!(cmd.contains("--job-name=full_job"));
        assert!(cmd.contains("--partition=gpu"));
        assert!(cmd.contains("--nodes=2"));
        assert!(cmd.contains("--ntasks=8"));
        assert!(cmd.contains("--time=4:00:00"));
        assert!(cmd.contains("--output=/logs/out_%j.log"));
        assert!(cmd.contains("--error=/logs/err_%j.log"));
    }

    #[test]
    fn test_parse_pipe_delimited_jobs() {
        let output = "12345|my_job|alice|RUNNING|compute|4|128|2-00:00:00|0:05:30|(Resources)\n";
        let fields = [
            "job_id",
            "name",
            "user",
            "state",
            "partition",
            "nodes",
            "cpus",
            "time_limit",
            "time_used",
            "reason",
        ];
        let jobs = parse_pipe_delimited(output, &fields);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["job_id"], "12345");
        assert_eq!(jobs[0]["state"], "RUNNING");
    }

    #[test]
    fn test_parse_pipe_delimited_empty() {
        let fields = ["a", "b"];
        let result = parse_pipe_delimited("", &fields);
        assert!(result.is_empty());
    }
}
