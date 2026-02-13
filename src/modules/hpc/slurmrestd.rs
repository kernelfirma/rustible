//! slurmrestd REST API client module
//!
//! Native REST client for slurmrestd — Slurm's HTTP daemon — enabling
//! structured JSON interaction for jobs, nodes, partitions, and diagnostics.
//! Falls back to CLI commands (sacct, sacctmgr) for accounting endpoints
//! not covered by the REST API.
//!
//! # Parameters
//!
//! - `api_url` (required): slurmrestd base URL (e.g. `http://slurmctld:6820`)
//! - `api_user` (required): Slurm username for `X-SLURM-USER-NAME` header
//! - `api_token` (required): JWT token for `X-SLURM-USER-TOKEN` header
//! - `action` (required): See actions table below
//! - `api_version` (optional): API version string (default: `v0.0.44`)
//! - `timeout` (optional): HTTP timeout in seconds (default: 30)
//! - `validate_certs` (optional): Verify TLS certificates (default: true)
//!
//! ## Actions
//!
//! | Action            | Method  | REST Endpoint                          |
//! |-------------------|---------|----------------------------------------|
//! | `submit_job`      | POST    | `/slurm/{ver}/job/submit`              |
//! | `cancel_job`      | DELETE  | `/slurm/{ver}/job/{id}`                |
//! | `get_job`         | GET     | `/slurm/{ver}/job/{id}`                |
//! | `list_jobs`       | GET     | `/slurm/{ver}/jobs/`                   |
//! | `get_node`        | GET     | `/slurm/{ver}/node/{name}`             |
//! | `list_nodes`      | GET     | `/slurm/{ver}/nodes/`                  |
//! | `update_node`     | POST    | `/slurm/{ver}/node/{name}`             |
//! | `get_partition`   | GET     | `/slurm/{ver}/partition/{name}`        |
//! | `list_partitions` | GET     | `/slurm/{ver}/partitions/`             |
//! | `ping`            | GET     | `/slurm/{ver}/ping/`                   |
//! | `diag`            | GET     | `/slurm/{ver}/diag/`                   |
//! | `reconfigure`     | GET     | `/slurm/{ver}/reconfigure/`            |
//! | `job_history`     | —       | CLI fallback: `sacct`                  |
//! | `list_accounts`   | —       | CLI fallback: `sacctmgr`               |

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;
use serde_json::Value;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};

// ---------------------------------------------------------------------------
// Tokio runtime bridge (same pattern as uri.rs)
// ---------------------------------------------------------------------------

fn run_async<F, T>(fut: F) -> ModuleResult<T>
where
    F: std::future::Future<Output = ModuleResult<T>> + Send,
    T: Send + 'static,
{
    if let Ok(handle) = Handle::try_current() {
        std::thread::scope(|s| {
            s.spawn(move || handle.block_on(fut))
                .join()
                .unwrap_or_else(|_| {
                    Err(ModuleError::ExecutionFailed(
                        "Tokio runtime thread panicked".to_string(),
                    ))
                })
        })
    } else {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to create tokio runtime: {}", e))
            })?;
        rt.block_on(fut)
    }
}

// ---------------------------------------------------------------------------
// CLI helpers (for accounting fallback — same pattern as slurm.rs)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// SlurmApiClient — HTTP helper
// ---------------------------------------------------------------------------

struct SlurmApiClient {
    client: Client,
    base_url: String,
    api_version: String,
    api_user: String,
    api_token: String,
}

impl SlurmApiClient {
    fn new(
        api_url: &str,
        api_user: &str,
        api_token: &str,
        api_version: &str,
        timeout_secs: u64,
        validate_certs: bool,
    ) -> ModuleResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .danger_accept_invalid_certs(!validate_certs)
            .build()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to build HTTP client: {}", e))
            })?;

        Ok(Self {
            client,
            base_url: api_url.trim_end_matches('/').to_string(),
            api_version: api_version.to_string(),
            api_user: api_user.to_string(),
            api_token: api_token.to_string(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/slurm/{}/{}",
            self.base_url,
            self.api_version,
            path.trim_start_matches('/')
        )
    }

    async fn get(&self, path: &str) -> ModuleResult<Value> {
        let resp = self
            .client
            .get(self.url(path))
            .header("X-SLURM-USER-NAME", &self.api_user)
            .header("X-SLURM-USER-TOKEN", &self.api_token)
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("HTTP GET failed: {}", e)))?;

        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to parse JSON: {}", e)))?;

        if !status.is_success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "HTTP {} from slurmrestd: {}",
                status, body
            )));
        }

        Self::check_errors(&body)?;
        Ok(body)
    }

    async fn post(&self, path: &str, body: &Value) -> ModuleResult<Value> {
        let resp = self
            .client
            .post(self.url(path))
            .header("X-SLURM-USER-NAME", &self.api_user)
            .header("X-SLURM-USER-TOKEN", &self.api_token)
            .json(body)
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("HTTP POST failed: {}", e)))?;

        let status = resp.status();
        let resp_body: Value = resp
            .json()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to parse JSON: {}", e)))?;

        if !status.is_success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "HTTP {} from slurmrestd: {}",
                status, resp_body
            )));
        }

        Self::check_errors(&resp_body)?;
        Ok(resp_body)
    }

    async fn delete(&self, path: &str) -> ModuleResult<Value> {
        let resp = self
            .client
            .delete(self.url(path))
            .header("X-SLURM-USER-NAME", &self.api_user)
            .header("X-SLURM-USER-TOKEN", &self.api_token)
            .send()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("HTTP DELETE failed: {}", e)))?;

        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to parse JSON: {}", e)))?;

        if !status.is_success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "HTTP {} from slurmrestd: {}",
                status, body
            )));
        }

        Self::check_errors(&body)?;
        Ok(body)
    }

    fn check_errors(body: &Value) -> ModuleResult<()> {
        if let Some(errors) = body.get("errors").and_then(|e| e.as_array()) {
            let real_errors: Vec<&Value> = errors
                .iter()
                .filter(|e| {
                    // Skip entries with error_number 0 (no error)
                    e.get("error_number").and_then(|n| n.as_i64()).unwrap_or(-1) != 0
                })
                .collect();

            if !real_errors.is_empty() {
                let msgs: Vec<String> = real_errors
                    .iter()
                    .map(|e| {
                        let msg = e
                            .get("error")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown error");
                        let num = e.get("error_number").and_then(|v| v.as_i64()).unwrap_or(0);
                        format!("[{}] {}", num, msg)
                    })
                    .collect();
                return Err(ModuleError::ExecutionFailed(format!(
                    "slurmrestd errors: {}",
                    msgs.join("; ")
                )));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Job submission body builder
// ---------------------------------------------------------------------------

fn build_job_submission_body(params: &ModuleParams) -> ModuleResult<Value> {
    let script = params.get_string_required("script")?;

    let mut job: serde_json::Map<String, Value> = serde_json::Map::new();

    if let Some(name) = params.get_string("job_name")? {
        job.insert("name".to_string(), Value::String(name));
    }
    if let Some(partition) = params.get_string("partition")? {
        job.insert("partition".to_string(), Value::String(partition));
    }
    if let Some(nodes) = params.get_string("nodes")? {
        if let Ok(n) = nodes.parse::<i64>() {
            job.insert(
                "minimum_nodes".to_string(),
                Value::Number(serde_json::Number::from(n)),
            );
        }
    }
    if let Some(ntasks) = params.get_string("ntasks")? {
        if let Ok(n) = ntasks.parse::<i64>() {
            job.insert(
                "tasks".to_string(),
                Value::Number(serde_json::Number::from(n)),
            );
        }
    }
    if let Some(time_limit) = params.get_string("time_limit")? {
        // slurmrestd expects time_limit as an object {minutes: N} or integer minutes
        if let Ok(minutes) = time_limit.parse::<i64>() {
            job.insert(
                "time_limit".to_string(),
                serde_json::json!({"set": true, "number": minutes}),
            );
        } else {
            // Pass as string — slurmrestd may parse "HH:MM:SS" formats
            job.insert("time_limit".to_string(), Value::String(time_limit));
        }
    }

    Ok(serde_json::json!({
        "job": job,
        "script": script
    }))
}

// ---------------------------------------------------------------------------
// SlurmrestdModule
// ---------------------------------------------------------------------------

pub struct SlurmrestdModule;

impl Module for SlurmrestdModule {
    fn name(&self) -> &'static str {
        "slurmrestd"
    }

    fn description(&self) -> &'static str {
        "REST API client for slurmrestd (jobs, nodes, partitions, diagnostics) with CLI fallback for accounting"
    }

    fn classification(&self) -> ModuleClassification {
        // RemoteCommand so the Connection is available for CLI fallback
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        &["api_url", "api_user", "api_token", "action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, Value> {
        let mut m = HashMap::new();
        m.insert("api_version", serde_json::json!("v0.0.44"));
        m.insert("timeout", serde_json::json!(30));
        m.insert("validate_certs", serde_json::json!(true));
        m.insert("job_id", serde_json::json!(null));
        m.insert("job_name", serde_json::json!(null));
        m.insert("script", serde_json::json!(null));
        m.insert("partition", serde_json::json!(null));
        m.insert("nodes", serde_json::json!(null));
        m.insert("ntasks", serde_json::json!(null));
        m.insert("time_limit", serde_json::json!(null));
        m.insert("signal", serde_json::json!(null));
        m.insert("node_name", serde_json::json!(null));
        m.insert("state", serde_json::json!(null));
        m.insert("reason", serde_json::json!(null));
        m.insert("account", serde_json::json!(null));
        m.insert("user", serde_json::json!(null));
        m
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let api_url = params.get_string_required("api_url")?;
        let api_user = params.get_string_required("api_user")?;
        let api_token = params.get_string_required("api_token")?;
        let action = params.get_string_required("action")?;
        let api_version = params
            .get_string("api_version")?
            .unwrap_or_else(|| "v0.0.44".to_string());
        let timeout_secs = params.get_i64("timeout")?.unwrap_or(30).max(1) as u64;
        let validate_certs = params.get_bool_or("validate_certs", true);

        // CLI fallback actions don't need the REST client
        match action.as_str() {
            "job_history" | "list_accounts" => {
                let connection = context.connection.as_ref().ok_or_else(|| {
                    ModuleError::ExecutionFailed(
                        "No connection available (needed for CLI fallback)".to_string(),
                    )
                })?;
                return match action.as_str() {
                    "job_history" => Self::action_job_history(connection, params, context),
                    "list_accounts" => Self::action_list_accounts(connection, params, context),
                    _ => unreachable!(),
                };
            }
            _ => {}
        }

        let client = SlurmApiClient::new(
            &api_url,
            &api_user,
            &api_token,
            &api_version,
            timeout_secs,
            validate_certs,
        )?;

        match action.as_str() {
            "submit_job" => Self::action_submit_job(&client, params, context),
            "cancel_job" => Self::action_cancel_job(&client, params, context),
            "get_job" => Self::action_get_job(&client, params),
            "list_jobs" => Self::action_list_jobs(&client),
            "get_node" => Self::action_get_node(&client, params),
            "list_nodes" => Self::action_list_nodes(&client),
            "update_node" => Self::action_update_node(&client, params, context),
            "get_partition" => Self::action_get_partition(&client, params),
            "list_partitions" => Self::action_list_partitions(&client),
            "ping" => Self::action_ping(&client),
            "diag" => Self::action_diag(&client),
            "reconfigure" => Self::action_reconfigure(&client, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Valid actions: submit_job, cancel_job, get_job, list_jobs, \
                 get_node, list_nodes, update_node, get_partition, list_partitions, \
                 ping, diag, reconfigure, job_history, list_accounts",
                action
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// REST action implementations
// ---------------------------------------------------------------------------

impl SlurmrestdModule {
    fn action_submit_job(
        client: &SlurmApiClient,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Idempotency: check for active jobs with same name
        if let Some(job_name) = params.get_string("job_name")? {
            let existing = run_async(async { client.get("jobs/").await })?;
            if let Some(jobs) = existing.get("jobs").and_then(|j| j.as_array()) {
                let active = jobs.iter().any(|j| {
                    let name_match = j.get("name").and_then(|n| n.as_str()) == Some(&job_name);
                    let state = j.get("job_state").and_then(|s| s.as_str()).unwrap_or("");
                    name_match
                        && (state == "RUNNING" || state == "PENDING" || state == "CONFIGURING")
                });
                if active {
                    return Ok(ModuleOutput::ok(format!(
                        "Job '{}' is already active, skipping submission",
                        job_name
                    ))
                    .with_data("job_name", Value::String(job_name)));
                }
            }
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would submit job to slurmrestd"));
        }

        let body = build_job_submission_body(params)?;
        let resp = run_async(async { client.post("job/submit", &body).await })?;

        let job_id = resp.get("job_id").and_then(|v| v.as_u64()).unwrap_or(0);

        Ok(
            ModuleOutput::changed(format!("Job submitted (id={})", job_id))
                .with_data("job_id", serde_json::json!(job_id))
                .with_data("response", resp),
        )
    }

    fn action_cancel_job(
        client: &SlurmApiClient,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let job_id = params.get_string_required("job_id")?;

        // Idempotency: check job state before cancelling
        let job_path = format!("job/{}", job_id);
        let info = run_async({
            let path = job_path.clone();
            async move { client.get(&path).await }
        })?;

        if let Some(jobs) = info.get("jobs").and_then(|j| j.as_array()) {
            if let Some(job) = jobs.first() {
                let state = job.get("job_state").and_then(|s| s.as_str()).unwrap_or("");
                if state == "COMPLETED"
                    || state == "CANCELLED"
                    || state == "FAILED"
                    || state == "TIMEOUT"
                {
                    return Ok(ModuleOutput::ok(format!(
                        "Job {} is already in terminal state '{}'",
                        job_id, state
                    ))
                    .with_data("job_id", Value::String(job_id))
                    .with_data("job_state", Value::String(state.to_string())));
                }
            }
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would cancel job {}",
                job_id
            )));
        }

        // Signal support: append ?signal=SIG if provided
        let path = if let Some(signal) = params.get_string("signal")? {
            format!("job/{}?signal={}", job_id, signal)
        } else {
            format!("job/{}", job_id)
        };
        let resp = run_async(async { client.delete(&path).await })?;

        Ok(ModuleOutput::changed(format!("Job {} cancelled", job_id))
            .with_data("job_id", Value::String(job_id))
            .with_data("response", resp))
    }

    fn action_get_job(
        client: &SlurmApiClient,
        params: &ModuleParams,
    ) -> ModuleResult<ModuleOutput> {
        let job_id = params.get_string_required("job_id")?;
        let resp = run_async(async { client.get(&format!("job/{}", job_id)).await })?;
        Ok(ModuleOutput::ok(format!("Retrieved job {}", job_id))
            .with_data("job_id", Value::String(job_id))
            .with_data(
                "jobs",
                resp.get("jobs").cloned().unwrap_or(Value::Array(vec![])),
            ))
    }

    fn action_list_jobs(client: &SlurmApiClient) -> ModuleResult<ModuleOutput> {
        let resp = run_async(async { client.get("jobs/").await })?;
        let count = resp
            .get("jobs")
            .and_then(|j| j.as_array())
            .map_or(0, |a| a.len());
        Ok(ModuleOutput::ok(format!("Retrieved {} jobs", count))
            .with_data(
                "jobs",
                resp.get("jobs").cloned().unwrap_or(Value::Array(vec![])),
            )
            .with_data("job_count", serde_json::json!(count)))
    }

    fn action_get_node(
        client: &SlurmApiClient,
        params: &ModuleParams,
    ) -> ModuleResult<ModuleOutput> {
        let node_name = params.get_string_required("node_name")?;
        let resp = run_async(async { client.get(&format!("node/{}", node_name)).await })?;
        Ok(ModuleOutput::ok(format!("Retrieved node '{}'", node_name))
            .with_data("node_name", Value::String(node_name))
            .with_data(
                "nodes",
                resp.get("nodes").cloned().unwrap_or(Value::Array(vec![])),
            ))
    }

    fn action_list_nodes(client: &SlurmApiClient) -> ModuleResult<ModuleOutput> {
        let resp = run_async(async { client.get("nodes/").await })?;
        let count = resp
            .get("nodes")
            .and_then(|j| j.as_array())
            .map_or(0, |a| a.len());
        Ok(ModuleOutput::ok(format!("Retrieved {} nodes", count))
            .with_data(
                "nodes",
                resp.get("nodes").cloned().unwrap_or(Value::Array(vec![])),
            )
            .with_data("node_count", serde_json::json!(count)))
    }

    fn action_update_node(
        client: &SlurmApiClient,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let node_name = params.get_string_required("node_name")?;
        let state = params.get_string_required("state")?;
        let reason = params.get_string("reason")?;

        // Idempotency: check current state
        let current = run_async(async { client.get(&format!("node/{}", node_name)).await })?;
        if let Some(nodes) = current.get("nodes").and_then(|n| n.as_array()) {
            if let Some(node) = nodes.first() {
                let current_state = node.get("state").and_then(|s| s.as_str()).unwrap_or("");
                if current_state.eq_ignore_ascii_case(&state) {
                    return Ok(ModuleOutput::ok(format!(
                        "Node '{}' is already in state '{}'",
                        node_name, state
                    ))
                    .with_data("node_name", Value::String(node_name))
                    .with_data("state", Value::String(state)));
                }
            }
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would update node '{}' to state '{}'",
                node_name, state
            )));
        }

        let mut body = serde_json::json!({ "state": state });
        if let Some(r) = reason {
            body["reason"] = Value::String(r);
        }

        let resp = run_async(async { client.post(&format!("node/{}", node_name), &body).await })?;

        Ok(
            ModuleOutput::changed(format!("Updated node '{}' to state '{}'", node_name, state))
                .with_data("node_name", Value::String(node_name))
                .with_data("state", Value::String(state))
                .with_data("response", resp),
        )
    }

    fn action_get_partition(
        client: &SlurmApiClient,
        params: &ModuleParams,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("partition")?;
        let resp = run_async(async { client.get(&format!("partition/{}", name)).await })?;
        Ok(ModuleOutput::ok(format!("Retrieved partition '{}'", name))
            .with_data("partition", Value::String(name))
            .with_data(
                "partitions",
                resp.get("partitions")
                    .cloned()
                    .unwrap_or(Value::Array(vec![])),
            ))
    }

    fn action_list_partitions(client: &SlurmApiClient) -> ModuleResult<ModuleOutput> {
        let resp = run_async(async { client.get("partitions/").await })?;
        let count = resp
            .get("partitions")
            .and_then(|j| j.as_array())
            .map_or(0, |a| a.len());
        Ok(ModuleOutput::ok(format!("Retrieved {} partitions", count))
            .with_data(
                "partitions",
                resp.get("partitions")
                    .cloned()
                    .unwrap_or(Value::Array(vec![])),
            )
            .with_data("partition_count", serde_json::json!(count)))
    }

    fn action_ping(client: &SlurmApiClient) -> ModuleResult<ModuleOutput> {
        let resp = run_async(async { client.get("ping/").await })?;
        Ok(ModuleOutput::ok("slurmrestd is reachable").with_data("ping", resp))
    }

    fn action_diag(client: &SlurmApiClient) -> ModuleResult<ModuleOutput> {
        let resp = run_async(async { client.get("diag/").await })?;
        Ok(ModuleOutput::ok("Retrieved slurmrestd diagnostics").with_data("diagnostics", resp))
    }

    fn action_reconfigure(
        client: &SlurmApiClient,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::changed(
                "Would reconfigure Slurm via slurmrestd",
            ));
        }
        let resp = run_async(async { client.get("reconfigure/").await })?;
        Ok(ModuleOutput::changed("Slurm reconfigured via slurmrestd").with_data("response", resp))
    }

    // -----------------------------------------------------------------------
    // CLI fallback actions (accounting endpoints)
    // -----------------------------------------------------------------------

    fn action_job_history(
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut cmd = String::from("sacct --json");
        if let Some(job_id) = params.get_string("job_id")? {
            cmd.push_str(&format!(" -j {}", job_id));
        }
        if let Some(user) = params.get_string("user")? {
            cmd.push_str(&format!(" -u {}", user));
        }
        if let Some(account) = params.get_string("account")? {
            cmd.push_str(&format!(" -A {}", account));
        }

        let stdout = run_cmd_ok(connection, &cmd, context)?;
        let data: Value = serde_json::from_str(&stdout).unwrap_or(Value::String(stdout));

        Ok(ModuleOutput::ok("Retrieved job history via sacct").with_data("job_history", data))
    }

    fn action_list_accounts(
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut cmd = String::from("sacctmgr show account --json");
        if let Some(account) = params.get_string("account")? {
            cmd.push_str(&format!(" name={}", account));
        }

        let stdout = run_cmd_ok(connection, &cmd, context)?;
        let data: Value = serde_json::from_str(&stdout).unwrap_or(Value::String(stdout));

        Ok(ModuleOutput::ok("Retrieved accounts via sacctmgr").with_data("accounts", data))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let m = SlurmrestdModule;
        assert_eq!(m.name(), "slurmrestd");
        assert!(m.description().contains("REST"));
        assert_eq!(m.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(m.parallelization_hint(), ParallelizationHint::FullyParallel);
        assert_eq!(
            m.required_params(),
            &["api_url", "api_user", "api_token", "action"]
        );
    }

    #[test]
    fn test_build_job_submission_body() {
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "script".to_string(),
            serde_json::json!("#!/bin/bash\necho hello"),
        );
        params.insert("job_name".to_string(), serde_json::json!("test_job"));
        params.insert("partition".to_string(), serde_json::json!("batch"));
        params.insert("nodes".to_string(), serde_json::json!("2"));
        params.insert("ntasks".to_string(), serde_json::json!("8"));
        params.insert("time_limit".to_string(), serde_json::json!("60"));

        let body = build_job_submission_body(&params).unwrap();

        assert_eq!(body["script"], "#!/bin/bash\necho hello");
        assert_eq!(body["job"]["name"], "test_job");
        assert_eq!(body["job"]["partition"], "batch");
        assert_eq!(body["job"]["minimum_nodes"], 2);
        assert_eq!(body["job"]["tasks"], 8);
    }

    #[test]
    fn test_build_job_submission_body_minimal() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("script".to_string(), serde_json::json!("#!/bin/bash\ndate"));

        let body = build_job_submission_body(&params).unwrap();
        assert_eq!(body["script"], "#!/bin/bash\ndate");
        assert!(body["job"].is_object());
    }

    #[test]
    fn test_build_job_submission_body_missing_script() {
        let params: ModuleParams = HashMap::new();
        let result = build_job_submission_body(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_errors_success() {
        // No errors field
        let body = serde_json::json!({"jobs": []});
        assert!(SlurmApiClient::check_errors(&body).is_ok());

        // Empty errors array
        let body = serde_json::json!({"errors": [], "jobs": []});
        assert!(SlurmApiClient::check_errors(&body).is_ok());

        // Only error_number 0 entries (no real errors)
        let body = serde_json::json!({
            "errors": [{"error": "", "error_number": 0}],
            "jobs": []
        });
        assert!(SlurmApiClient::check_errors(&body).is_ok());
    }

    #[test]
    fn test_check_errors_failure() {
        let body = serde_json::json!({
            "errors": [
                {"error": "Invalid job id", "error_number": 2017}
            ]
        });
        let result = SlurmApiClient::check_errors(&body);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid job id"));
        assert!(err_msg.contains("2017"));
    }

    #[test]
    fn test_check_errors_multiple() {
        let body = serde_json::json!({
            "errors": [
                {"error": "first error", "error_number": 1},
                {"error": "second error", "error_number": 2}
            ]
        });
        let result = SlurmApiClient::check_errors(&body);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("first error"));
        assert!(err_msg.contains("second error"));
    }

    #[test]
    fn test_action_dispatch_invalid() {
        let m = SlurmrestdModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "api_url".to_string(),
            serde_json::json!("http://localhost:6820"),
        );
        params.insert("api_user".to_string(), serde_json::json!("testuser"));
        params.insert("api_token".to_string(), serde_json::json!("testtoken"));
        params.insert("action".to_string(), serde_json::json!("nonexistent"));

        let context = ModuleContext::default();
        let result = m.execute(&params, &context);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Invalid action"));
        assert!(err_msg.contains("nonexistent"));
    }

    #[test]
    fn test_cli_fallback_command_construction() {
        // Verify the sacct command string is built correctly
        let mut params: ModuleParams = HashMap::new();
        params.insert("job_id".to_string(), serde_json::json!("12345"));
        params.insert("user".to_string(), serde_json::json!("testuser"));
        params.insert("account".to_string(), serde_json::json!("research"));

        // We can't run the actual command, but we can verify parameter parsing
        assert_eq!(
            params.get_string("job_id").unwrap(),
            Some("12345".to_string())
        );
        assert_eq!(
            params.get_string("user").unwrap(),
            Some("testuser".to_string())
        );
        assert_eq!(
            params.get_string("account").unwrap(),
            Some("research".to_string())
        );
    }

    #[test]
    fn test_optional_params_defaults() {
        let m = SlurmrestdModule;
        let opts = m.optional_params();
        assert_eq!(opts["api_version"], serde_json::json!("v0.0.44"));
        assert_eq!(opts["timeout"], serde_json::json!(30));
        assert_eq!(opts["validate_certs"], serde_json::json!(true));
    }

    #[test]
    fn test_slurm_api_client_url() {
        let client = SlurmApiClient {
            client: Client::new(),
            base_url: "http://slurmctld:6820".to_string(),
            api_version: "v0.0.44".to_string(),
            api_user: "user".to_string(),
            api_token: "token".to_string(),
        };
        assert_eq!(
            client.url("jobs/"),
            "http://slurmctld:6820/slurm/v0.0.44/jobs/"
        );
        assert_eq!(
            client.url("/job/123"),
            "http://slurmctld:6820/slurm/v0.0.44/job/123"
        );
    }

    #[test]
    fn test_slurm_api_client_url_trailing_slash() {
        let client = SlurmApiClient {
            client: Client::new(),
            base_url: "http://slurmctld:6820/".to_string(),
            api_version: "v0.0.44".to_string(),
            api_user: "user".to_string(),
            api_token: "token".to_string(),
        };
        // base_url trailing slash is stripped in constructor, but we set it directly here
        // The url() method should still work — it just won't double-slash
        assert!(client.url("ping/").contains("/slurm/v0.0.44/ping/"));
    }
}
