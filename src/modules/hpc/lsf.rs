//! IBM Spectrum LSF configuration and queue lifecycle management
//!
//! Provides modules for managing LSF queues, hosts, and scheduling policies.
//!
//! # Modules
//!
//! - `lsf_queue`: Queue CRUD via bqueues/badmin
//! - `lsf_host`: Host open/close state management
//! - `lsf_policy`: Fairshare and preemption policy management

use std::collections::HashMap;
use std::sync::Arc;

use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of a preflight check before applying changes.
#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single configuration drift item between desired and actual state.
#[derive(Debug, serde::Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

/// Result of a post-apply verification step.
#[derive(Debug, serde::Serialize)]
struct VerifyResult {
    verified: bool,
    details: Vec<String>,
    warnings: Vec<String>,
}

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
// Queue state parsing helpers
// ---------------------------------------------------------------------------

/// Parsed state of an LSF queue from `bqueues -l` output.
#[derive(Debug, Clone)]
struct LsfQueueState {
    exists: bool,
    status: String,
    priority: Option<String>,
    hosts: Option<String>,
    description: Option<String>,
    max_jobs: Option<String>,
}

/// Parse `bqueues -l <name>` output to detect queue existence and properties.
fn parse_bqueues_output(output: &str) -> LsfQueueState {
    let trimmed = output.trim();
    if trimmed.is_empty() || trimmed.contains("No matching queue") {
        return LsfQueueState {
            exists: false,
            status: String::new(),
            priority: None,
            hosts: None,
            description: None,
            max_jobs: None,
        };
    }

    let mut status = String::from("Open");
    let mut priority = None;
    let mut hosts = None;
    let mut description = None;
    let mut max_jobs = None;

    for line in trimmed.lines() {
        let line = line.trim();
        if line.starts_with("STATUS:") || line.starts_with("STATUS ") {
            let val = extract_value(line);
            if !val.is_empty() {
                status = val;
            }
        } else if line.starts_with("PRIO:") || line.starts_with("PRIO ") {
            let val = extract_value(line);
            if !val.is_empty() {
                priority = Some(val);
            }
        } else if line.starts_with("HOSTS:") || line.starts_with("HOSTS ") {
            let val = extract_value(line);
            if !val.is_empty() {
                hosts = Some(val);
            }
        } else if line.starts_with("DESCRIPTION:") || line.starts_with("DESCRIPTION ") {
            let val = extract_value(line);
            if !val.is_empty() {
                description = Some(val);
            }
        } else if line.starts_with("MAX:") || line.starts_with("MAX ") {
            let val = extract_value(line);
            if !val.is_empty() {
                max_jobs = Some(val);
            }
        }
    }

    LsfQueueState {
        exists: true,
        status,
        priority,
        hosts,
        description,
        max_jobs,
    }
}

/// Extract the value portion after the first colon or whitespace separator.
fn extract_value(line: &str) -> String {
    if let Some(idx) = line.find(':') {
        line[idx + 1..].trim().to_string()
    } else {
        // Try splitting on whitespace after the key
        let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
        if parts.len() > 1 {
            parts[1].trim().to_string()
        } else {
            String::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Host state parsing helpers
// ---------------------------------------------------------------------------

/// Parsed state of an LSF host from `bhosts` output.
#[derive(Debug, Clone)]
struct LsfHostState {
    exists: bool,
    status: String,
    max_slots: Option<String>,
    njobs: Option<String>,
}

/// Parse `bhosts <name>` output to detect host existence and status.
///
/// bhosts output format (tabular):
/// ```text
/// HOST_NAME          STATUS       JL/U    MAX  NJOBS    RUN  SSUSP  USUSP    RSV
/// hostA              ok              -     16      0      0      0      0      0
/// ```
fn parse_bhosts_output(output: &str) -> LsfHostState {
    let trimmed = output.trim();
    if trimmed.is_empty() || trimmed.contains("No matching host") || trimmed.contains("not found") {
        return LsfHostState {
            exists: false,
            status: String::new(),
            max_slots: None,
            njobs: None,
        };
    }

    // Skip the header line and parse the data line
    let mut data_line: Option<&str> = None;
    for line in trimmed.lines() {
        let line = line.trim();
        if line.starts_with("HOST_NAME") || line.is_empty() {
            continue;
        }
        data_line = Some(line);
        break;
    }

    if let Some(line) = data_line {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // Fields: HOST_NAME STATUS JL/U MAX NJOBS RUN SSUSP USUSP RSV
        if fields.len() >= 5 {
            return LsfHostState {
                exists: true,
                status: fields[1].to_string(),
                max_slots: Some(fields[3].to_string()),
                njobs: Some(fields[4].to_string()),
            };
        }
    }

    LsfHostState {
        exists: true,
        status: "unknown".to_string(),
        max_slots: None,
        njobs: None,
    }
}

// ---------------------------------------------------------------------------
// LsfQueueModule
// ---------------------------------------------------------------------------

/// Manage IBM Spectrum LSF queues via bqueues/badmin.
///
/// Supports creating, removing, and configuring LSF queues with full
/// idempotency via `bqueues -l` state detection.
pub struct LsfQueueModule;

impl Module for LsfQueueModule {
    fn name(&self) -> &'static str {
        "lsf_queue"
    }

    fn description(&self) -> &'static str {
        "Manage IBM Spectrum LSF queues (create, remove, configure via bqueues/badmin)"
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
        m.insert("priority", serde_json::json!(null));
        m.insert("hosts", serde_json::json!(null));
        m.insert("description", serde_json::json!(null));
        m.insert("max_jobs", serde_json::json!(null));
        m
    }
}

impl LsfQueueModule {
    /// Get current queue state by running `bqueues -l <name>`.
    fn get_queue_state(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<LsfQueueState> {
        let cmd = format!("bqueues -l {} 2>/dev/null", name);
        let (ok, stdout, _) = run_cmd(connection, &cmd, context)?;
        if !ok {
            return Ok(LsfQueueState {
                exists: false,
                status: String::new(),
                priority: None,
                hosts: None,
                description: None,
                max_jobs: None,
            });
        }
        Ok(parse_bqueues_output(&stdout))
    }

    /// Ensure queue is present with the desired configuration.
    fn ensure_present(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let current = self.get_queue_state(connection, name, context)?;

        // Check for drift items if queue already exists
        if current.exists {
            let drift = self.compute_drift(&current, params)?;
            if drift.is_empty() {
                return Ok(ModuleOutput::ok(format!(
                    "Queue '{}' already exists with desired config",
                    name
                ))
                .with_data("name", serde_json::json!(name))
                .with_data("status", serde_json::json!(current.status)));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would update queue '{}': {}",
                    name,
                    drift
                        .iter()
                        .map(|d| format!("{}: {} -> {}", d.field, d.actual, d.desired))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .with_data("drift", serde_json::json!(drift)));
            }

            // Apply configuration updates
            self.apply_queue_config(connection, name, params, context)?;

            return Ok(
                ModuleOutput::changed(format!("Updated queue '{}' configuration", name))
                    .with_data("name", serde_json::json!(name))
                    .with_data("drift", serde_json::json!(drift)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would create queue '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        // Create new queue: use badmin to add queue, then open it
        let cmd = format!("echo 'y' | badmin qopen {}", name);
        let _ = run_cmd(connection, &cmd, context);

        // Apply configuration
        self.apply_queue_config(connection, name, params, context)?;

        Ok(ModuleOutput::changed(format!("Created queue '{}'", name))
            .with_data("name", serde_json::json!(name)))
    }

    /// Ensure queue is absent.
    fn ensure_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let current = self.get_queue_state(connection, name, context)?;

        if !current.exists {
            return Ok(
                ModuleOutput::ok(format!("Queue '{}' is already absent", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would remove queue '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        // Close the queue first, then remove it
        let close_cmd = format!("badmin qclose {}", name);
        let _ = run_cmd(connection, &close_cmd, context);

        // Inactivate the queue
        let inact_cmd = format!("badmin qinact {}", name);
        run_cmd_ok(connection, &inact_cmd, context)?;

        Ok(ModuleOutput::changed(format!("Removed queue '{}'", name))
            .with_data("name", serde_json::json!(name)))
    }

    /// Compute drift between current state and desired parameters.
    fn compute_drift(
        &self,
        current: &LsfQueueState,
        params: &ModuleParams,
    ) -> ModuleResult<Vec<DriftItem>> {
        let mut drift = Vec::new();

        if let Some(priority) = params.get_string("priority")? {
            let actual = current.priority.clone().unwrap_or_default();
            if actual != priority {
                drift.push(DriftItem {
                    field: "priority".to_string(),
                    desired: priority,
                    actual,
                });
            }
        }

        if let Some(hosts) = params.get_string("hosts")? {
            let actual = current.hosts.clone().unwrap_or_default();
            if actual != hosts {
                drift.push(DriftItem {
                    field: "hosts".to_string(),
                    desired: hosts,
                    actual,
                });
            }
        }

        if let Some(description) = params.get_string("description")? {
            let actual = current.description.clone().unwrap_or_default();
            if actual != description {
                drift.push(DriftItem {
                    field: "description".to_string(),
                    desired: description,
                    actual,
                });
            }
        }

        if let Some(max_jobs) = params.get_string("max_jobs")? {
            let actual = current.max_jobs.clone().unwrap_or_default();
            if actual != max_jobs {
                drift.push(DriftItem {
                    field: "max_jobs".to_string(),
                    desired: max_jobs,
                    actual,
                });
            }
        }

        Ok(drift)
    }

    /// Apply queue configuration parameters via LSF configuration commands.
    fn apply_queue_config(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // LSF queues are configured via lsb.queues file or bconf.
        // For each parameter, write or update the configuration.
        let mut reconfig_needed = false;

        if let Some(priority) = params.get_string("priority")? {
            let cmd = format!("bconf -o set -a PRIORITY={} -q {}", priority, name);
            let _ = run_cmd(connection, &cmd, context);
            reconfig_needed = true;
        }

        if let Some(hosts) = params.get_string("hosts")? {
            let cmd = format!("bconf -o set -a HOSTS={} -q {}", hosts, name);
            let _ = run_cmd(connection, &cmd, context);
            reconfig_needed = true;
        }

        if let Some(description) = params.get_string("description")? {
            let cmd = format!("bconf -o set -a DESCRIPTION='{}' -q {}", description, name);
            let _ = run_cmd(connection, &cmd, context);
            reconfig_needed = true;
        }

        if let Some(max_jobs) = params.get_string("max_jobs")? {
            let cmd = format!("bconf -o set -a QJOB_LIMIT={} -q {}", max_jobs, name);
            let _ = run_cmd(connection, &cmd, context);
            reconfig_needed = true;
        }

        // Reconfigure the cluster to apply changes
        if reconfig_needed {
            let reconfig_cmd = "badmin reconfig";
            run_cmd_ok(connection, reconfig_cmd, context)?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LsfHostModule
// ---------------------------------------------------------------------------

/// Manage IBM Spectrum LSF host open/close state.
///
/// Controls whether an LSF host accepts new jobs via `badmin hopen`
/// and `badmin hclose`, with job-aware guards to prevent disruption.
pub struct LsfHostModule;

impl Module for LsfHostModule {
    fn name(&self) -> &'static str {
        "lsf_host"
    }

    fn description(&self) -> &'static str {
        "Manage IBM Spectrum LSF host open/close state (badmin hopen/hclose)"
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
            .unwrap_or_else(|| "open".to_string());

        match state.as_str() {
            "open" => self.ensure_open(connection, &name, context),
            "closed" => self.ensure_closed(connection, &name, params, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'open' or 'closed'",
                state
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("open"));
        m.insert("force", serde_json::json!(false));
        m
    }
}

impl LsfHostModule {
    /// Get current host state by running `bhosts <name>`.
    fn get_host_state(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<LsfHostState> {
        let cmd = format!("bhosts {} 2>/dev/null", name);
        let (ok, stdout, _) = run_cmd(connection, &cmd, context)?;
        if !ok {
            return Ok(LsfHostState {
                exists: false,
                status: String::new(),
                max_slots: None,
                njobs: None,
            });
        }
        Ok(parse_bhosts_output(&stdout))
    }

    /// Check if there are running jobs on this host.
    fn has_running_jobs(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = format!("bjobs -u all -m {} -noheader 2>/dev/null | wc -l", name);
        let (ok, stdout, _) = run_cmd(connection, &cmd, context)?;
        if !ok {
            return Ok(false);
        }
        let count: i64 = stdout.trim().parse().unwrap_or(0);
        Ok(count > 0)
    }

    /// Ensure host is open (accepting jobs).
    fn ensure_open(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let current = self.get_host_state(connection, name, context)?;

        if !current.exists {
            return Err(ModuleError::ExecutionFailed(format!(
                "Host '{}' not found in LSF cluster",
                name
            )));
        }

        if current.status == "ok" {
            return Ok(ModuleOutput::ok(format!("Host '{}' is already open", name))
                .with_data("name", serde_json::json!(name))
                .with_data("status", serde_json::json!(current.status)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would open host '{}' (current status: {})",
                name, current.status
            ))
            .with_data("name", serde_json::json!(name)));
        }

        let cmd = format!("badmin hopen {}", name);
        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!("Opened host '{}'", name))
            .with_data("name", serde_json::json!(name))
            .with_data("previous_status", serde_json::json!(current.status)))
    }

    /// Ensure host is closed (not accepting new jobs).
    fn ensure_closed(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let current = self.get_host_state(connection, name, context)?;

        if !current.exists {
            return Err(ModuleError::ExecutionFailed(format!(
                "Host '{}' not found in LSF cluster",
                name
            )));
        }

        if current.status == "closed" || current.status == "closed_Full" {
            return Ok(
                ModuleOutput::ok(format!("Host '{}' is already closed", name))
                    .with_data("name", serde_json::json!(name))
                    .with_data("status", serde_json::json!(current.status)),
            );
        }

        let force = params.get_bool_or("force", false);

        // Job-aware guard: check for running jobs before closing
        if !force {
            let has_jobs = self.has_running_jobs(connection, name, context)?;
            if has_jobs {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Host '{}' has running jobs. Use force=true to close anyway",
                    name
                )));
            }
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would close host '{}' (current status: {})",
                name, current.status
            ))
            .with_data("name", serde_json::json!(name)));
        }

        let cmd = format!("badmin hclose {}", name);
        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!("Closed host '{}'", name))
            .with_data("name", serde_json::json!(name))
            .with_data("previous_status", serde_json::json!(current.status)))
    }
}

// ---------------------------------------------------------------------------
// LsfPolicyModule
// ---------------------------------------------------------------------------

/// Manage IBM Spectrum LSF scheduling policies.
///
/// Supports fairshare and preemption policy configuration via
/// lsb.params and `badmin reconfig`.
pub struct LsfPolicyModule;

impl Module for LsfPolicyModule {
    fn name(&self) -> &'static str {
        "lsf_policy"
    }

    fn description(&self) -> &'static str {
        "Manage IBM Spectrum LSF scheduling policies (fairshare, preemption)"
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

        let policy_type = params.get_string_required("policy_type")?;
        let validate = params.get_bool_or("validate", true);

        match policy_type.as_str() {
            "fairshare" => self.apply_fairshare(connection, params, validate, context),
            "preemption" => self.apply_preemption(connection, params, validate, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid policy_type '{}'. Must be 'fairshare' or 'preemption'",
                policy_type
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["policy_type"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user_shares", serde_json::json!(null));
        m.insert("preemption_priority", serde_json::json!(null));
        m.insert("validate", serde_json::json!(true));
        m
    }
}

impl LsfPolicyModule {
    /// Validate fairshare policy parameters.
    fn validate_fairshare(&self, params: &ModuleParams) -> ModuleResult<PreflightResult> {
        let mut result = PreflightResult {
            passed: true,
            warnings: Vec::new(),
            errors: Vec::new(),
        };

        if let Some(user_shares) = params.get_string("user_shares")? {
            // user_shares format: "user1:share1,user2:share2" or
            // "group1:share1,group2:share2"
            for entry in user_shares.split(',') {
                let entry = entry.trim();
                if entry.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = entry.split(':').collect();
                if parts.len() != 2 {
                    result.passed = false;
                    result.errors.push(format!(
                        "Invalid user_shares entry '{}': expected 'name:share' format",
                        entry
                    ));
                    continue;
                }
                let name = parts[0].trim();
                let share_str = parts[1].trim();
                if name.is_empty() {
                    result.passed = false;
                    result.errors.push(format!(
                        "Empty user/group name in user_shares entry '{}'",
                        entry
                    ));
                }
                if share_str.parse::<u32>().is_err() {
                    result.passed = false;
                    result.errors.push(format!(
                        "Invalid share value '{}' in entry '{}': must be a positive integer",
                        share_str, entry
                    ));
                }
            }

            if user_shares.trim().is_empty() {
                result
                    .warnings
                    .push("user_shares is empty; fairshare will use default shares".to_string());
            }
        } else {
            result.warnings.push(
                "No user_shares specified; fairshare configuration may be incomplete".to_string(),
            );
        }

        Ok(result)
    }

    /// Validate preemption policy parameters.
    fn validate_preemption(&self, params: &ModuleParams) -> ModuleResult<PreflightResult> {
        let mut result = PreflightResult {
            passed: true,
            warnings: Vec::new(),
            errors: Vec::new(),
        };

        if let Some(priority) = params.get_string("preemption_priority")? {
            // preemption_priority format: "queue1:priority1,queue2:priority2"
            for entry in priority.split(',') {
                let entry = entry.trim();
                if entry.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = entry.split(':').collect();
                if parts.len() != 2 {
                    result.passed = false;
                    result.errors.push(format!(
                        "Invalid preemption_priority entry '{}': expected 'queue:priority' format",
                        entry
                    ));
                    continue;
                }
                let queue_name = parts[0].trim();
                let prio_str = parts[1].trim();
                if queue_name.is_empty() {
                    result.passed = false;
                    result.errors.push(format!(
                        "Empty queue name in preemption_priority entry '{}'",
                        entry
                    ));
                }
                if prio_str.parse::<i64>().is_err() {
                    result.passed = false;
                    result.errors.push(format!(
                        "Invalid priority value '{}' in entry '{}': must be an integer",
                        prio_str, entry
                    ));
                }
            }
        } else {
            result.warnings.push(
                "No preemption_priority specified; preemption may not be effective".to_string(),
            );
        }

        Ok(result)
    }

    /// Apply fairshare policy configuration.
    fn apply_fairshare(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        validate: bool,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if validate {
            let preflight = self.validate_fairshare(params)?;
            if !preflight.passed {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Fairshare validation failed: {}",
                    preflight.errors.join("; ")
                )));
            }
        }

        let user_shares = params.get_string("user_shares")?.unwrap_or_default();

        if user_shares.is_empty() {
            return Ok(ModuleOutput::ok(
                "No fairshare user_shares specified; nothing to apply".to_string(),
            ));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would apply fairshare policy with user_shares: {}",
                user_shares
            ))
            .with_data("policy_type", serde_json::json!("fairshare"))
            .with_data("user_shares", serde_json::json!(user_shares)));
        }

        // Check current fairshare configuration
        let (_, current_stdout, _) =
            run_cmd(connection, "badmin showconf mbd 2>/dev/null", context)?;
        let already_configured =
            current_stdout.contains("FAIRSHARE") && current_stdout.contains(&user_shares);

        if already_configured {
            return Ok(ModuleOutput::ok(
                "Fairshare policy is already configured as desired".to_string(),
            )
            .with_data("policy_type", serde_json::json!("fairshare"))
            .with_data("user_shares", serde_json::json!(user_shares)));
        }

        // Write fairshare configuration to lsb.params via bconf or direct file update
        let cmd = format!("bconf -o set -a 'FAIRSHARE=USER_SHARES[{}]'", user_shares);
        let _ = run_cmd(connection, &cmd, context);

        // Reconfigure to apply
        run_cmd_ok(connection, "badmin reconfig", context)?;

        // Verify the configuration took effect
        let verify = self.verify_policy(connection, "fairshare", context)?;

        Ok(
            ModuleOutput::changed("Applied fairshare policy".to_string())
                .with_data("policy_type", serde_json::json!("fairshare"))
                .with_data("user_shares", serde_json::json!(user_shares))
                .with_data("verification", serde_json::json!(verify)),
        )
    }

    /// Apply preemption policy configuration.
    fn apply_preemption(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        validate: bool,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if validate {
            let preflight = self.validate_preemption(params)?;
            if !preflight.passed {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Preemption validation failed: {}",
                    preflight.errors.join("; ")
                )));
            }
        }

        let preemption_priority = params
            .get_string("preemption_priority")?
            .unwrap_or_default();

        if preemption_priority.is_empty() {
            return Ok(ModuleOutput::ok(
                "No preemption_priority specified; nothing to apply".to_string(),
            ));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would apply preemption policy with priority: {}",
                preemption_priority
            ))
            .with_data("policy_type", serde_json::json!("preemption"))
            .with_data(
                "preemption_priority",
                serde_json::json!(preemption_priority),
            ));
        }

        // Check current preemption configuration
        let (_, current_stdout, _) =
            run_cmd(connection, "badmin showconf mbd 2>/dev/null", context)?;
        let already_configured =
            current_stdout.contains("PREEMPTION") && current_stdout.contains(&preemption_priority);

        if already_configured {
            return Ok(ModuleOutput::ok(
                "Preemption policy is already configured as desired".to_string(),
            )
            .with_data("policy_type", serde_json::json!("preemption"))
            .with_data(
                "preemption_priority",
                serde_json::json!(preemption_priority),
            ));
        }

        // Write preemption configuration
        let cmd = format!(
            "bconf -o set -a 'PREEMPTION=PREEMPT[{}]'",
            preemption_priority
        );
        let _ = run_cmd(connection, &cmd, context);

        // Reconfigure to apply
        run_cmd_ok(connection, "badmin reconfig", context)?;

        // Verify
        let verify = self.verify_policy(connection, "preemption", context)?;

        Ok(
            ModuleOutput::changed("Applied preemption policy".to_string())
                .with_data("policy_type", serde_json::json!("preemption"))
                .with_data(
                    "preemption_priority",
                    serde_json::json!(preemption_priority),
                )
                .with_data("verification", serde_json::json!(verify)),
        )
    }

    /// Verify that a policy has been applied by checking LSF configuration.
    fn verify_policy(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        policy_type: &str,
        context: &ModuleContext,
    ) -> ModuleResult<VerifyResult> {
        let (ok, stdout, _) = run_cmd(connection, "badmin showconf mbd 2>/dev/null", context)?;

        let keyword = match policy_type {
            "fairshare" => "FAIRSHARE",
            "preemption" => "PREEMPTION",
            _ => {
                return Ok(VerifyResult {
                    verified: false,
                    details: vec![format!("Unknown policy type: {}", policy_type)],
                    warnings: Vec::new(),
                })
            }
        };

        if ok && stdout.contains(keyword) {
            Ok(VerifyResult {
                verified: true,
                details: vec![format!("{} policy is active in MBD configuration", keyword)],
                warnings: Vec::new(),
            })
        } else {
            Ok(VerifyResult {
                verified: false,
                details: vec![format!(
                    "{} policy not detected in MBD configuration output",
                    keyword
                )],
                warnings: vec![
                    "Policy may require manual verification via 'badmin showconf mbd'".to_string(),
                ],
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsf_queue_metadata() {
        let module = LsfQueueModule;
        assert_eq!(module.name(), "lsf_queue");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params(), &["name"]);
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("priority"));
        assert!(optional.contains_key("hosts"));
        assert!(optional.contains_key("description"));
        assert!(optional.contains_key("max_jobs"));
    }

    #[test]
    fn test_lsf_host_metadata() {
        let module = LsfHostModule;
        assert_eq!(module.name(), "lsf_host");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params(), &["name"]);
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("force"));
    }

    #[test]
    fn test_lsf_policy_metadata() {
        let module = LsfPolicyModule;
        assert_eq!(module.name(), "lsf_policy");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params(), &["policy_type"]);
        let optional = module.optional_params();
        assert!(optional.contains_key("user_shares"));
        assert!(optional.contains_key("preemption_priority"));
        assert!(optional.contains_key("validate"));
    }

    #[test]
    fn test_queue_state_parsing() {
        // Test parsing bqueues -l output for an existing queue
        let output = r#"QUEUE: normal
  -- High-priority batch queue

PARAMETERS/STATISTICS
STATUS: Open
PRIO: 40
MAX: 100
HOSTS: hostA hostB hostC
DESCRIPTION: Default batch queue

"#;
        let state = parse_bqueues_output(output);
        assert!(state.exists);
        assert_eq!(state.status, "Open");
        assert_eq!(state.priority, Some("40".to_string()));
        assert_eq!(state.max_jobs, Some("100".to_string()));
        assert_eq!(state.hosts, Some("hostA hostB hostC".to_string()));
        assert_eq!(state.description, Some("Default batch queue".to_string()));

        // Test parsing empty output (queue does not exist)
        let empty_state = parse_bqueues_output("");
        assert!(!empty_state.exists);

        // Test parsing "No matching queue" output
        let not_found = parse_bqueues_output("No matching queue found");
        assert!(!not_found.exists);
    }

    #[test]
    fn test_host_state_parsing() {
        // Test parsing bhosts output for a running host
        let output = "HOST_NAME          STATUS       JL/U    MAX  NJOBS    RUN  SSUSP  USUSP    RSV\nhostA              ok              -     16      3      3      0      0      0\n";
        let state = parse_bhosts_output(output);
        assert!(state.exists);
        assert_eq!(state.status, "ok");
        assert_eq!(state.max_slots, Some("16".to_string()));
        assert_eq!(state.njobs, Some("3".to_string()));

        // Test parsing closed host
        let closed_output = "HOST_NAME          STATUS       JL/U    MAX  NJOBS    RUN  SSUSP  USUSP    RSV\nhostB              closed          -     32      0      0      0      0      0\n";
        let closed_state = parse_bhosts_output(closed_output);
        assert!(closed_state.exists);
        assert_eq!(closed_state.status, "closed");

        // Test parsing unavailable host
        let unavail_output = "HOST_NAME          STATUS       JL/U    MAX  NJOBS    RUN  SSUSP  USUSP    RSV\nhostC              unavail         -      0      0      0      0      0      0\n";
        let unavail_state = parse_bhosts_output(unavail_output);
        assert!(unavail_state.exists);
        assert_eq!(unavail_state.status, "unavail");

        // Test parsing empty output
        let empty_state = parse_bhosts_output("");
        assert!(!empty_state.exists);

        // Test parsing "not found" output
        let not_found = parse_bhosts_output("Host not found");
        assert!(!not_found.exists);
    }

    #[test]
    fn test_policy_validation() {
        let module = LsfPolicyModule;

        // Test valid fairshare
        let mut params = ModuleParams::new();
        params.insert(
            "user_shares".to_string(),
            serde_json::json!("user1:10,user2:20,user3:30"),
        );
        let result = module.validate_fairshare(&params).unwrap();
        assert!(result.passed);
        assert!(result.errors.is_empty());

        // Test invalid fairshare - missing share value
        let mut params_bad = ModuleParams::new();
        params_bad.insert(
            "user_shares".to_string(),
            serde_json::json!("user1:10,invalid_entry,user3:30"),
        );
        let result_bad = module.validate_fairshare(&params_bad).unwrap();
        assert!(!result_bad.passed);
        assert!(!result_bad.errors.is_empty());

        // Test invalid fairshare - non-numeric share
        let mut params_nan = ModuleParams::new();
        params_nan.insert("user_shares".to_string(), serde_json::json!("user1:abc"));
        let result_nan = module.validate_fairshare(&params_nan).unwrap();
        assert!(!result_nan.passed);

        // Test valid preemption
        let mut params_preempt = ModuleParams::new();
        params_preempt.insert(
            "preemption_priority".to_string(),
            serde_json::json!("high:100,low:10"),
        );
        let result_preempt = module.validate_preemption(&params_preempt).unwrap();
        assert!(result_preempt.passed);
        assert!(result_preempt.errors.is_empty());

        // Test invalid preemption - bad format
        let mut params_preempt_bad = ModuleParams::new();
        params_preempt_bad.insert(
            "preemption_priority".to_string(),
            serde_json::json!("bad_entry"),
        );
        let result_preempt_bad = module.validate_preemption(&params_preempt_bad).unwrap();
        assert!(!result_preempt_bad.passed);

        // Test invalid preemption - non-numeric priority
        let mut params_preempt_nan = ModuleParams::new();
        params_preempt_nan.insert(
            "preemption_priority".to_string(),
            serde_json::json!("queue1:notanumber"),
        );
        let result_preempt_nan = module.validate_preemption(&params_preempt_nan).unwrap();
        assert!(!result_preempt_nan.passed);

        // Test empty user_shares warning
        let params_empty = ModuleParams::new();
        let result_empty = module.validate_fairshare(&params_empty).unwrap();
        assert!(result_empty.passed);
        assert!(!result_empty.warnings.is_empty());
    }
}
