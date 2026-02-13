//! Slurm cluster information gathering module
//!
//! Gathers cluster state as structured facts from Slurm commands.
//!
//! # Parameters
//!
//! - `gather` (required): What to gather — "nodes", "jobs", "partitions", "accounts", "cluster"
//! - `partition` (optional): Filter by partition name
//! - `node` (optional): Filter by node name
//! - `user` (optional): Filter by user name
//! - `state` (optional): Filter by state (e.g. "idle", "running")

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

pub struct SlurmInfoModule;

impl Module for SlurmInfoModule {
    fn name(&self) -> &'static str {
        "slurm_info"
    }

    fn description(&self) -> &'static str {
        "Gather Slurm cluster state as structured facts (nodes, jobs, partitions, accounts)"
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

        let gather = params.get_string_required("gather")?;

        match gather.as_str() {
            "nodes" => self.gather_nodes(connection, params, context),
            "jobs" => self.gather_jobs(connection, params, context),
            "partitions" => self.gather_partitions(connection, params, context),
            "accounts" => self.gather_accounts(connection, params, context),
            "cluster" => self.gather_cluster(connection, params, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid gather type '{}'. Must be 'nodes', 'jobs', 'partitions', 'accounts', or 'cluster'",
                gather
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["gather"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("partition", serde_json::json!(null));
        m.insert("node", serde_json::json!(null));
        m.insert("user", serde_json::json!(null));
        m.insert("state", serde_json::json!(null));
        m
    }
}

impl SlurmInfoModule {
    fn gather_nodes(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut cmd = "sinfo --noheader -o '%n|%T|%c|%m|%P|%E|%O|%e'".to_string();
        if let Some(partition) = params.get_string("partition")? {
            cmd.push_str(&format!(" -p {}", partition));
        }
        if let Some(node) = params.get_string("node")? {
            cmd.push_str(&format!(" -n {}", node));
        }

        let stdout = run_cmd_ok(connection, &cmd, context)?;
        let nodes = parse_sinfo_nodes(&stdout);

        Ok(
            ModuleOutput::ok(format!("Gathered {} node(s)", nodes.len()))
                .with_data("nodes", serde_json::json!(nodes))
                .with_data("count", serde_json::json!(nodes.len())),
        )
    }

    fn gather_jobs(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut cmd = "squeue --noheader -o '%i|%j|%u|%T|%P|%D|%C|%l|%M|%R'".to_string();
        if let Some(partition) = params.get_string("partition")? {
            cmd.push_str(&format!(" -p {}", partition));
        }
        if let Some(user) = params.get_string("user")? {
            cmd.push_str(&format!(" -u {}", user));
        }
        if let Some(state) = params.get_string("state")? {
            cmd.push_str(&format!(" -t {}", state));
        }

        let stdout = run_cmd_ok(connection, &cmd, context)?;
        let jobs = parse_squeue_jobs(&stdout);

        Ok(ModuleOutput::ok(format!("Gathered {} job(s)", jobs.len()))
            .with_data("jobs", serde_json::json!(jobs))
            .with_data("count", serde_json::json!(jobs.len())))
    }

    fn gather_partitions(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut cmd = "sinfo --noheader -o '%R|%a|%F|%c|%m|%l|%G'".to_string();
        if let Some(partition) = params.get_string("partition")? {
            cmd.push_str(&format!(" -p {}", partition));
        }

        let stdout = run_cmd_ok(connection, &cmd, context)?;
        let partitions = parse_sinfo_partitions(&stdout);

        Ok(
            ModuleOutput::ok(format!("Gathered {} partition(s)", partitions.len()))
                .with_data("partitions", serde_json::json!(partitions))
                .with_data("count", serde_json::json!(partitions.len())),
        )
    }

    fn gather_accounts(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        _params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let stdout = run_cmd_ok(
            connection,
            "sacctmgr --noheader --parsable2 list accounts format=Account,Description,Organization",
            context,
        )?;
        let accounts = parse_sacctmgr_accounts(&stdout);

        Ok(
            ModuleOutput::ok(format!("Gathered {} account(s)", accounts.len()))
                .with_data("accounts", serde_json::json!(accounts))
                .with_data("count", serde_json::json!(accounts.len())),
        )
    }

    fn gather_cluster(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        _params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Gather aggregated cluster summary from multiple commands
        let (_, node_out, _) = run_cmd(
            connection,
            "sinfo --noheader -o '%T' | sort | uniq -c",
            context,
        )?;
        let (_, job_out, _) = run_cmd(
            connection,
            "squeue --noheader -o '%T' | sort | uniq -c",
            context,
        )?;
        let (_, part_out, _) = run_cmd(connection, "sinfo --noheader -o '%R' | sort -u", context)?;

        let node_states = parse_uniq_counts(&node_out);
        let job_states = parse_uniq_counts(&job_out);
        let partitions: Vec<String> = part_out
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        Ok(ModuleOutput::ok("Gathered cluster summary")
            .with_data("node_states", serde_json::json!(node_states))
            .with_data("job_states", serde_json::json!(job_states))
            .with_data("partitions", serde_json::json!(partitions))
            .with_data("partition_count", serde_json::json!(partitions.len())))
    }
}

/// Parse sinfo node output (pipe-delimited).
/// Format: NodeName|State|CPUs|Memory|Partition|Reason|Load|FreeMem
fn parse_sinfo_nodes(output: &str) -> Vec<serde_json::Value> {
    let fields = [
        "name",
        "state",
        "cpus",
        "memory",
        "partition",
        "reason",
        "load",
        "free_mem",
    ];
    parse_pipe_delimited(output, &fields)
}

/// Parse squeue job output (pipe-delimited).
/// Format: JobID|Name|User|State|Partition|Nodes|CPUs|TimeLimit|TimeUsed|Reason
fn parse_squeue_jobs(output: &str) -> Vec<serde_json::Value> {
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
    parse_pipe_delimited(output, &fields)
}

/// Parse sinfo partition output (pipe-delimited).
/// Format: Partition|Avail|Nodes(A/I/O/T)|CPUs|Memory|TimeLimit|GRES
fn parse_sinfo_partitions(output: &str) -> Vec<serde_json::Value> {
    let fields = [
        "name",
        "avail",
        "nodes_aiot",
        "cpus",
        "memory",
        "time_limit",
        "gres",
    ];
    parse_pipe_delimited(output, &fields)
}

/// Parse sacctmgr account output (pipe-delimited via --parsable2).
/// Format: Account|Description|Organization
fn parse_sacctmgr_accounts(output: &str) -> Vec<serde_json::Value> {
    let fields = ["account", "description", "organization"];
    parse_pipe_delimited(output, &fields)
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

/// Parse `uniq -c` output into a map of value -> count.
fn parse_uniq_counts(output: &str) -> HashMap<String, i64> {
    let mut map = HashMap::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((count_str, value)) = trimmed.split_once(char::is_whitespace) {
            if let Ok(count) = count_str.trim().parse::<i64>() {
                map.insert(value.trim().to_string(), count);
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sinfo_nodes() {
        let output = "node01|idle|32|128000|compute|none|0.50|120000\nnode02|allocated|64|256000|gpu|none|45.2|100000\n";
        let nodes = parse_sinfo_nodes(output);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0]["name"], "node01");
        assert_eq!(nodes[0]["state"], "idle");
        assert_eq!(nodes[0]["cpus"], "32");
        assert_eq!(nodes[0]["memory"], "128000");
        assert_eq!(nodes[0]["partition"], "compute");
        assert_eq!(nodes[1]["name"], "node02");
        assert_eq!(nodes[1]["state"], "allocated");
        assert_eq!(nodes[1]["cpus"], "64");
    }

    #[test]
    fn test_parse_sinfo_nodes_empty() {
        let nodes = parse_sinfo_nodes("");
        assert!(nodes.is_empty());
    }

    #[test]
    fn test_parse_squeue_jobs() {
        let output = "12345|my_job|alice|RUNNING|compute|4|128|2-00:00:00|0:05:30|(Resources)\n";
        let jobs = parse_squeue_jobs(output);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["job_id"], "12345");
        assert_eq!(jobs[0]["name"], "my_job");
        assert_eq!(jobs[0]["user"], "alice");
        assert_eq!(jobs[0]["state"], "RUNNING");
        assert_eq!(jobs[0]["partition"], "compute");
        assert_eq!(jobs[0]["nodes"], "4");
        assert_eq!(jobs[0]["cpus"], "128");
    }

    #[test]
    fn test_parse_squeue_jobs_multiple() {
        let output = "100|job_a|bob|PENDING|gpu|1|8|1:00:00|0:00:00|(Priority)\n200|job_b|carol|RUNNING|compute|2|16|4:00:00|1:30:00|(None)\n";
        let jobs = parse_squeue_jobs(output);
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0]["job_id"], "100");
        assert_eq!(jobs[1]["job_id"], "200");
        assert_eq!(jobs[1]["state"], "RUNNING");
    }

    #[test]
    fn test_parse_sinfo_partitions() {
        let output = "compute|up|10/5/0/15|32|128000|infinite|(null)\ngpu|up|2/0/0/2|64|256000|7-00:00:00|gpu:4\n";
        let parts = parse_sinfo_partitions(output);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["name"], "compute");
        assert_eq!(parts[0]["avail"], "up");
        assert_eq!(parts[0]["nodes_aiot"], "10/5/0/15");
        assert_eq!(parts[1]["name"], "gpu");
        assert_eq!(parts[1]["gres"], "gpu:4");
    }

    #[test]
    fn test_parse_sacctmgr_accounts() {
        let output = "research|Research group|Physics\nengineering|Engineering team|CS\n";
        let accounts = parse_sacctmgr_accounts(output);
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0]["account"], "research");
        assert_eq!(accounts[0]["description"], "Research group");
        assert_eq!(accounts[0]["organization"], "Physics");
        assert_eq!(accounts[1]["account"], "engineering");
    }

    #[test]
    fn test_parse_uniq_counts() {
        let output = "     10 idle\n      5 allocated\n      2 drained\n";
        let counts = parse_uniq_counts(output);
        assert_eq!(counts.get("idle"), Some(&10));
        assert_eq!(counts.get("allocated"), Some(&5));
        assert_eq!(counts.get("drained"), Some(&2));
    }

    #[test]
    fn test_parse_uniq_counts_empty() {
        let counts = parse_uniq_counts("");
        assert!(counts.is_empty());
    }

    #[test]
    fn test_parse_pipe_delimited_short_line() {
        let output = "only_one_field\n";
        let fields = ["a", "b", "c"];
        let result = parse_pipe_delimited(output, &fields);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_pipe_delimited_extra_fields() {
        let output = "a|b|c|d|e\n";
        let fields = ["f1", "f2", "f3"];
        let result = parse_pipe_delimited(output, &fields);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["f1"], "a");
        assert_eq!(result[0]["f3"], "c");
    }
}
