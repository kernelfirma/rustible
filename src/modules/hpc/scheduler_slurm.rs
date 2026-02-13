//! Slurm scheduler backend for the unified HPC abstraction layer.
//!
//! Implements [`HpcScheduler`] by calling Slurm CLI commands (`sbatch`,
//! `scancel`, `squeue`, `sacct`, `scontrol`).

use std::collections::HashMap;

use crate::modules::{
    ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
};

use super::scheduler::{
    run_cmd, run_cmd_ok, HpcScheduler, JobInfo, QueueInfo, ServerInfo,
    map_slurm_state,
};

/// Slurm backend for the unified HPC scheduler abstraction.
pub struct SlurmScheduler;

impl HpcScheduler for SlurmScheduler {
    fn scheduler_name(&self) -> &'static str {
        "slurm"
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

        // Idempotency: check if a job with this name is already pending/running
        if let Some(ref name) = job_name {
            let (ok, stdout, _) = run_cmd(
                connection,
                &format!(
                    "squeue --noheader --name={} -o '%i|%T' 2>/dev/null",
                    name
                ),
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

    fn cancel_job(&self, job_id: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

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

        run_cmd_ok(
            connection,
            &format!("scancel {}", job_id),
            context,
        )?;

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

        // Try squeue first (active jobs)
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!(
                "squeue --noheader -j {} -o '%i|%j|%u|%T|%P|%D|%C|%l|%M' 2>/dev/null",
                job_id
            ),
            context,
        )?;

        if ok && !stdout.trim().is_empty() {
            if let Some(line) = stdout.lines().find(|l| !l.trim().is_empty()) {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 9 {
                    return Ok(JobInfo {
                        id: parts[0].trim().to_string(),
                        name: Some(parts[1].trim().to_string()),
                        state: map_slurm_state(parts[3].trim()),
                        queue: Some(parts[4].trim().to_string()),
                        owner: Some(parts[2].trim().to_string()),
                        nodes: parts[5].trim().parse().ok(),
                        cpus: parts[6].trim().parse().ok(),
                        walltime_limit: Some(parts[7].trim().to_string()),
                        walltime_used: Some(parts[8].trim().to_string()),
                        raw: serde_json::json!({
                            "source": "squeue",
                            "raw_state": parts[3].trim(),
                        }),
                    });
                }
            }
        }

        // Fallback to sacct for completed jobs
        let (ok2, stdout2, _) = run_cmd(
            connection,
            &format!(
                "sacct --noheader --parsable2 -j {} --format=JobID,JobName,User,State,Partition,NNodes,NCPUs,Timelimit,Elapsed 2>/dev/null",
                job_id
            ),
            context,
        )?;

        if ok2 && !stdout2.trim().is_empty() {
            if let Some(line) = stdout2.lines().find(|l| !l.trim().is_empty()) {
                let parts: Vec<&str> = line.split('|').collect();
                if parts.len() >= 9 {
                    return Ok(JobInfo {
                        id: parts[0].trim().to_string(),
                        name: Some(parts[1].trim().to_string()),
                        state: map_slurm_state(parts[3].trim()),
                        queue: Some(parts[4].trim().to_string()),
                        owner: Some(parts[2].trim().to_string()),
                        nodes: parts[5].trim().parse().ok(),
                        cpus: parts[6].trim().parse().ok(),
                        walltime_limit: Some(parts[7].trim().to_string()),
                        walltime_used: Some(parts[8].trim().to_string()),
                        raw: serde_json::json!({
                            "source": "sacct",
                            "raw_state": parts[3].trim(),
                        }),
                    });
                }
            }
        }

        Err(ModuleError::ExecutionFailed(format!(
            "Job {} not found in squeue or sacct",
            job_id
        )))
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

        run_cmd_ok(
            connection,
            &format!("scontrol hold {}", job_id),
            context,
        )?;

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

        run_cmd_ok(
            connection,
            &format!("scontrol release {}", job_id),
            context,
        )?;

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
            "scontrol show partition -o 2>/dev/null",
            context,
        )?;

        let mut queues = Vec::new();
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let kv = parse_scontrol_oneliner(trimmed);
            let name = kv
                .get("PartitionName")
                .cloned()
                .unwrap_or_default();
            let state_raw = kv.get("State").cloned().unwrap_or_default();
            let state = if state_raw.to_uppercase() == "UP" {
                "active".to_string()
            } else {
                "inactive".to_string()
            };
            let total_jobs = kv
                .get("TotalNodes")
                .and_then(|v| v.parse::<u32>().ok());

            queues.push(QueueInfo {
                name,
                state,
                total_jobs,
                raw: serde_json::json!(kv),
            });
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

        // Check if partition already exists
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("scontrol show partition {} -o 2>/dev/null", name),
            context,
        )?;
        if ok && !stdout.trim().is_empty() && !stdout.contains("not found") {
            return Ok(
                ModuleOutput::ok(format!("Partition '{}' already exists", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would create partition '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        let mut props = Vec::new();
        if let Some(nodes) = params.get_string("nodes")? {
            props.push(format!("Nodes={}", nodes));
        }
        if let Some(state) = params.get_string("state")? {
            props.push(format!("State={}", state.to_uppercase()));
        }
        let props_str = props.join(" ");
        let cmd = format!("scontrol create PartitionName={} {}", name, props_str);
        run_cmd_ok(connection, &cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Created partition '{}'", name))
                .with_data("name", serde_json::json!(name)),
        )
    }

    fn delete_queue(&self, name: &str, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        // Idempotency
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("scontrol show partition {} -o 2>/dev/null", name),
            context,
        )?;
        if !ok || stdout.trim().is_empty() || stdout.contains("not found") {
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

    fn query_server(&self, context: &ModuleContext) -> ModuleResult<ServerInfo> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let stdout = run_cmd_ok(
            connection,
            "scontrol show config 2>/dev/null",
            context,
        )?;

        let mut attributes = HashMap::new();
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("Configuration") {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                attributes.insert(
                    key.trim().to_string(),
                    value.trim().to_string(),
                );
            }
        }

        Ok(ServerInfo {
            scheduler: "slurm".to_string(),
            attributes,
            raw: serde_json::json!(stdout),
        })
    }

    fn set_server_attributes(
        &self,
        _params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        Err(ModuleError::Unsupported(
            "Slurm does not support runtime server attribute changes via scontrol. \
             Modify slurm.conf and run 'scontrol reconfigure' instead."
                .to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an sbatch command from unified parameters.
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
    // Unified param is "queue", Slurm calls it "partition"
    if let Some(queue) = params.get_string("queue")? {
        args.push(format!("--partition={}", queue));
    }
    if let Some(nodes) = params.get_string("nodes")? {
        args.push(format!("--nodes={}", nodes));
    }
    if let Some(cpus) = params.get_string("cpus")? {
        args.push(format!("--ntasks={}", cpus));
    }
    if let Some(walltime) = params.get_string("walltime")? {
        args.push(format!("--time={}", walltime));
    }
    if let Some(output) = params.get_string("output_path")? {
        args.push(format!("--output={}", output));
    }
    if let Some(error) = params.get_string("error_path")? {
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
        let escaped = script_content.replace('\'', "'\\''");
        Ok(format!("sbatch {} --wrap '{}'", args_str, escaped)
            .trim()
            .to_string())
    }
}

/// Parse sbatch output to extract job ID.
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

/// Parse scontrol one-liner output into key-value HashMap.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slurm_scheduler_name() {
        let s = SlurmScheduler;
        assert_eq!(s.scheduler_name(), "slurm");
    }

    #[test]
    fn test_slurm_build_sbatch_command_basic() {
        let mut params = ModuleParams::new();
        params.insert("script_path".to_string(), serde_json::json!("/tmp/job.sh"));
        params.insert("job_name".to_string(), serde_json::json!("test_job"));
        params.insert("queue".to_string(), serde_json::json!("compute"));
        params.insert("nodes".to_string(), serde_json::json!("4"));
        params.insert("walltime".to_string(), serde_json::json!("2:00:00"));

        let cmd = build_sbatch_command(&params).unwrap();
        assert!(cmd.contains("sbatch"));
        assert!(cmd.contains("--job-name=test_job"));
        assert!(cmd.contains("--partition=compute"));
        assert!(cmd.contains("--nodes=4"));
        assert!(cmd.contains("--time=2:00:00"));
        assert!(cmd.contains("/tmp/job.sh"));
    }

    #[test]
    fn test_slurm_build_sbatch_command_no_script() {
        let params = ModuleParams::new();
        let result = build_sbatch_command(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_slurm_parse_sbatch_output() {
        assert_eq!(
            parse_sbatch_output("Submitted batch job 12345\n"),
            Some("12345".to_string())
        );
        assert_eq!(parse_sbatch_output(""), None);
    }

    #[test]
    fn test_slurm_parse_scontrol_oneliner() {
        let output = "PartitionName=compute State=UP TotalNodes=16";
        let map = parse_scontrol_oneliner(output);
        assert_eq!(map.get("PartitionName"), Some(&"compute".to_string()));
        assert_eq!(map.get("State"), Some(&"UP".to_string()));
    }
}
