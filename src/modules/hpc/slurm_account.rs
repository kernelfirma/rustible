//! Slurm account management module
//!
//! Manage Slurm accounts and user associations via sacctmgr.
//!
//! # Parameters
//!
//! - `action` (required): "create", "update", "delete", "add_user", or "remove_user"
//! - `account` (required): Account name
//! - `user` (optional, required for add_user/remove_user): User name
//! - `organization` (optional): Organization name
//! - `description` (optional): Account description
//! - `parent` (optional): Parent account name
//! - `max_jobs` (optional): Maximum concurrent jobs
//! - `max_submit` (optional): Maximum submitted jobs
//! - `max_wall` (optional): Maximum wall time per job
//! - `fairshare` (optional): Fairshare value
//! - `cluster` (optional): Cluster name (defaults to current)
//! - `validate_policies` (optional, default true): Run policy validation preflight

use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of policy validation preflight checks.
#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single field that differs between desired and actual state.
#[derive(Debug, serde::Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

/// Result of post-change verification.
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

pub struct SlurmAccountModule;

impl Module for SlurmAccountModule {
    fn name(&self) -> &'static str {
        "slurm_account"
    }

    fn description(&self) -> &'static str {
        "Manage Slurm accounts and user associations (sacctmgr)"
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
        let account = params.get_string_required("account")?;

        // Policy validation preflight for create/update actions
        let should_validate = params.get_bool_or("validate_policies", true);
        if should_validate && (action == "create" || action == "update") {
            let preflight = validate_policies(params)?;
            if !preflight.passed {
                return Err(ModuleError::InvalidParameter(format!(
                    "Policy validation failed: {}",
                    preflight.errors.join("; ")
                )));
            }
        }

        match action.as_str() {
            "create" => self.action_create(connection, &account, params, context),
            "update" => self.action_update(connection, &account, params, context),
            "delete" => self.action_delete(connection, &account, params, context),
            "add_user" => self.action_add_user(connection, &account, params, context),
            "remove_user" => self.action_remove_user(connection, &account, params, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'create', 'update', 'delete', 'add_user', or 'remove_user'",
                action
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action", "account"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("user", serde_json::json!(null));
        m.insert("organization", serde_json::json!(null));
        m.insert("description", serde_json::json!(null));
        m.insert("parent", serde_json::json!(null));
        m.insert("max_jobs", serde_json::json!(null));
        m.insert("max_submit", serde_json::json!(null));
        m.insert("max_wall", serde_json::json!(null));
        m.insert("fairshare", serde_json::json!(null));
        m.insert("cluster", serde_json::json!(null));
        m.insert("validate_policies", serde_json::json!(true));
        m
    }
}

impl SlurmAccountModule {
    /// Check if an account exists.
    fn account_exists(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        account: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!(
                "sacctmgr --noheader --parsable2 list accounts where name={} format=Account",
                account
            ),
            context,
        )?;
        Ok(ok && !stdout.trim().is_empty())
    }

    /// Check if a user association exists for an account.
    fn user_association_exists(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        user: &str,
        account: &str,
        cluster: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let mut cmd = format!(
            "sacctmgr --noheader --parsable2 list associations where user={} account={}",
            user, account
        );
        if let Some(c) = cluster {
            cmd.push_str(&format!(" cluster={}", c));
        }
        cmd.push_str(" format=User,Account");
        let (ok, stdout, _) = run_cmd(connection, &cmd, context)?;
        Ok(ok && !stdout.trim().is_empty())
    }

    fn action_create(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        account: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Idempotency check
        if self.account_exists(connection, account, context)? {
            return Ok(
                ModuleOutput::ok(format!("Account '{}' already exists", account))
                    .with_data("account", serde_json::json!(account)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would create account '{}'", account))
                    .with_data("account", serde_json::json!(account)),
            );
        }

        let props = build_account_properties(params)?;
        let mut cmd = format!("sacctmgr --immediate add account {}", account);
        if !props.is_empty() {
            cmd.push(' ');
            cmd.push_str(&props);
        }
        if let Some(cluster) = params.get_string("cluster")? {
            cmd.push_str(&format!(" cluster={}", cluster));
        }

        run_cmd_ok(connection, &cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Created account '{}'", account))
                .with_data("account", serde_json::json!(account)),
        )
    }

    fn action_update(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        account: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if !self.account_exists(connection, account, context)? {
            return Err(ModuleError::ExecutionFailed(format!(
                "Account '{}' does not exist; cannot update",
                account
            )));
        }

        // Get current properties from the cluster for drift detection
        let current_props = get_account_properties(connection, account, context)?;

        // Build desired properties map from params
        let desired = build_desired_properties(params)?;

        if desired.is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "No properties to update for account '{}'",
                account
            ))
            .with_data("account", serde_json::json!(account)));
        }

        // Drift-aware reconciliation: only modify fields that actually differ
        let drift = reconcile_account(&desired, &current_props);

        if drift.is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "Account '{}' is already in desired state",
                account
            ))
            .with_data("account", serde_json::json!(account)));
        }

        // Build effective diff summary for output
        let diff_summary = compute_effective_diff(&drift, &current_props);

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would update account '{}': {}",
                account, diff_summary
            ))
            .with_data("account", serde_json::json!(account))
            .with_data("drift", serde_json::json!(drift))
            .with_data("diff_summary", serde_json::json!(diff_summary)));
        }

        // Build sacctmgr set clause only from drifted fields
        let set_clause = build_set_clause_from_drift(&drift);
        let cmd = if let Some(cluster) = params.get_string("cluster")? {
            format!(
                "sacctmgr --immediate modify account where name={} cluster={} set {}",
                account, cluster, set_clause
            )
        } else {
            format!(
                "sacctmgr --immediate modify account where name={} set {}",
                account, set_clause
            )
        };

        run_cmd_ok(connection, &cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Updated account '{}'", account))
                .with_data("account", serde_json::json!(account))
                .with_data("drift", serde_json::json!(drift))
                .with_data("diff_summary", serde_json::json!(diff_summary)),
        )
    }

    fn action_delete(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        account: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Idempotency check
        if !self.account_exists(connection, account, context)? {
            return Ok(
                ModuleOutput::ok(format!("Account '{}' does not exist", account))
                    .with_data("account", serde_json::json!(account)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would delete account '{}'", account))
                    .with_data("account", serde_json::json!(account)),
            );
        }

        let mut cmd = format!("sacctmgr --immediate delete account where name={}", account);
        if let Some(cluster) = params.get_string("cluster")? {
            cmd.push_str(&format!(" cluster={}", cluster));
        }

        run_cmd_ok(connection, &cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Deleted account '{}'", account))
                .with_data("account", serde_json::json!(account)),
        )
    }

    fn action_add_user(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        account: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let user = params.get_string_required("user")?;
        let cluster = params.get_string("cluster")?;

        // Idempotency check
        if self.user_association_exists(connection, &user, account, cluster.as_deref(), context)? {
            return Ok(ModuleOutput::ok(format!(
                "User '{}' is already associated with account '{}'",
                user, account
            ))
            .with_data("user", serde_json::json!(user))
            .with_data("account", serde_json::json!(account)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would add user '{}' to account '{}'",
                user, account
            ))
            .with_data("user", serde_json::json!(user))
            .with_data("account", serde_json::json!(account)));
        }

        let mut cmd = format!("sacctmgr --immediate add user {} account={}", user, account);
        if let Some(ref c) = cluster {
            cmd.push_str(&format!(" cluster={}", c));
        }
        // Add optional user-level limits
        let user_props = build_user_properties(params)?;
        if !user_props.is_empty() {
            cmd.push(' ');
            cmd.push_str(&user_props);
        }

        run_cmd_ok(connection, &cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Added user '{}' to account '{}'", user, account))
                .with_data("user", serde_json::json!(user))
                .with_data("account", serde_json::json!(account)),
        )
    }

    fn action_remove_user(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        account: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let user = params.get_string_required("user")?;
        let cluster = params.get_string("cluster")?;

        // Idempotency check
        if !self.user_association_exists(connection, &user, account, cluster.as_deref(), context)? {
            return Ok(ModuleOutput::ok(format!(
                "User '{}' is not associated with account '{}'",
                user, account
            ))
            .with_data("user", serde_json::json!(user))
            .with_data("account", serde_json::json!(account)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would remove user '{}' from account '{}'",
                user, account
            ))
            .with_data("user", serde_json::json!(user))
            .with_data("account", serde_json::json!(account)));
        }

        let mut cmd = format!(
            "sacctmgr --immediate delete user where name={} account={}",
            user, account
        );
        if let Some(ref c) = cluster {
            cmd.push_str(&format!(" cluster={}", c));
        }

        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!(
            "Removed user '{}' from account '{}'",
            user, account
        ))
        .with_data("user", serde_json::json!(user))
        .with_data("account", serde_json::json!(account)))
    }
}

/// Build sacctmgr account property string from params.
fn build_account_properties(params: &ModuleParams) -> ModuleResult<String> {
    let mut props = Vec::new();

    if let Some(org) = params.get_string("organization")? {
        props.push(format!("Organization={}", org));
    }
    if let Some(desc) = params.get_string("description")? {
        props.push(format!("Description={}", desc));
    }
    if let Some(parent) = params.get_string("parent")? {
        props.push(format!("parent={}", parent));
    }
    if let Some(fairshare) = params.get_string("fairshare")? {
        props.push(format!("fairshare={}", fairshare));
    }
    if let Some(max_jobs) = params.get_string("max_jobs")? {
        props.push(format!("MaxJobs={}", max_jobs));
    }
    if let Some(max_submit) = params.get_string("max_submit")? {
        props.push(format!("MaxSubmitJobs={}", max_submit));
    }
    if let Some(max_wall) = params.get_string("max_wall")? {
        props.push(format!("MaxWall={}", max_wall));
    }

    Ok(props.join(" "))
}

/// Build sacctmgr user-level property string.
fn build_user_properties(params: &ModuleParams) -> ModuleResult<String> {
    let mut props = Vec::new();
    if let Some(fairshare) = params.get_string("fairshare")? {
        props.push(format!("fairshare={}", fairshare));
    }
    if let Some(max_jobs) = params.get_string("max_jobs")? {
        props.push(format!("MaxJobs={}", max_jobs));
    }
    if let Some(max_submit) = params.get_string("max_submit")? {
        props.push(format!("MaxSubmitJobs={}", max_submit));
    }
    if let Some(max_wall) = params.get_string("max_wall")? {
        props.push(format!("MaxWall={}", max_wall));
    }
    Ok(props.join(" "))
}

/// Parse sacctmgr show account output into a property map.
///
/// Uses `--noheader --parsable2` format which outputs fields separated by `|`
/// without a trailing separator. Fields:
/// Account|Description|Organization|ParentName|Fairshare|MaxJobs|MaxSubmitJobs|MaxWall
fn get_account_properties(
    connection: &Arc<dyn Connection + Send + Sync>,
    account: &str,
    context: &ModuleContext,
) -> ModuleResult<HashMap<String, String>> {
    let cmd = format!(
        "sacctmgr --noheader --parsable2 show account where name={} \
         format=Account,Description,Organization,ParentName,Fairshare,MaxJobs,MaxSubmitJobs,MaxWall \
         withassoc",
        account
    );
    let (ok, stdout, _) = run_cmd(connection, &cmd, context)?;

    let mut result = HashMap::new();
    if !ok || stdout.trim().is_empty() {
        return Ok(result);
    }

    // parsable2 format uses '|' as separator, no trailing '|'
    let headers = [
        "account",
        "description",
        "organization",
        "parent",
        "fairshare",
        "maxjobs",
        "maxsubmitjobs",
        "maxwall",
    ];

    // Take the first non-empty line (first association row)
    if let Some(line) = stdout.lines().find(|l| !l.trim().is_empty()) {
        let fields: Vec<&str> = line.split('|').collect();
        for (i, header) in headers.iter().enumerate() {
            if let Some(val) = fields.get(i) {
                let val = val.trim();
                if !val.is_empty() {
                    result.insert(header.to_string(), val.to_string());
                }
            }
        }
    }

    Ok(result)
}

/// Build a map of desired properties from module params.
///
/// Returns normalized field names (lowercase) mapped to values, matching
/// the keys produced by `get_account_properties`.
fn build_desired_properties(params: &ModuleParams) -> ModuleResult<HashMap<String, String>> {
    let mut desired = HashMap::new();

    if let Some(org) = params.get_string("organization")? {
        desired.insert("organization".to_string(), org);
    }
    if let Some(desc) = params.get_string("description")? {
        desired.insert("description".to_string(), desc);
    }
    if let Some(parent) = params.get_string("parent")? {
        desired.insert("parent".to_string(), parent);
    }
    if let Some(fairshare) = params.get_string("fairshare")? {
        desired.insert("fairshare".to_string(), fairshare);
    }
    if let Some(max_jobs) = params.get_string("max_jobs")? {
        desired.insert("maxjobs".to_string(), max_jobs);
    }
    if let Some(max_submit) = params.get_string("max_submit")? {
        desired.insert("maxsubmitjobs".to_string(), max_submit);
    }
    if let Some(max_wall) = params.get_string("max_wall")? {
        desired.insert("maxwall".to_string(), max_wall);
    }

    Ok(desired)
}

/// Compare desired properties against current properties and return drift items.
///
/// Only properties that actually differ are returned. Fields that match
/// (case-insensitive comparison for text fields) are not included.
fn reconcile_account(
    desired: &HashMap<String, String>,
    current: &HashMap<String, String>,
) -> Vec<DriftItem> {
    let mut drift = Vec::new();

    for (field, desired_val) in desired {
        let actual_val = current.get(field).cloned().unwrap_or_default();
        if !values_match(desired_val, &actual_val) {
            drift.push(DriftItem {
                field: field.clone(),
                desired: desired_val.clone(),
                actual: actual_val,
            });
        }
    }

    // Sort for deterministic output
    drift.sort_by(|a, b| a.field.cmp(&b.field));
    drift
}

/// Check if two property values match (case-insensitive for text fields).
fn values_match(desired: &str, actual: &str) -> bool {
    desired.eq_ignore_ascii_case(actual)
}

/// Map normalized field names back to sacctmgr property names for the set clause.
fn field_to_sacctmgr(field: &str) -> &str {
    match field {
        "organization" => "Organization",
        "description" => "Description",
        "parent" => "parent",
        "fairshare" => "fairshare",
        "maxjobs" => "MaxJobs",
        "maxsubmitjobs" => "MaxSubmitJobs",
        "maxwall" => "MaxWall",
        _ => field,
    }
}

/// Build a sacctmgr `set` clause from drift items only.
fn build_set_clause_from_drift(drift: &[DriftItem]) -> String {
    drift
        .iter()
        .map(|d| format!("{}={}", field_to_sacctmgr(&d.field), d.desired))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Validate accounting policy parameters.
///
/// Checks:
/// - `max_jobs`: must be a positive integer
/// - `max_submit`: must be a positive integer, and >= max_jobs if both set
/// - `max_wall`: must match time format `\d+(-\d+:\d+(:\d+)?)?` or be -1 (unlimited)
/// - `fairshare`: must be a positive integer or 1 (default)
fn validate_policies(params: &ModuleParams) -> ModuleResult<PreflightResult> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let max_jobs_val = params.get_string("max_jobs")?;
    let max_submit_val = params.get_string("max_submit")?;
    let max_wall_val = params.get_string("max_wall")?;
    let fairshare_val = params.get_string("fairshare")?;

    // Validate max_jobs
    let mut max_jobs_num: Option<i64> = None;
    if let Some(ref val) = max_jobs_val {
        match val.parse::<i64>() {
            Ok(n) if n > 0 => {
                max_jobs_num = Some(n);
            }
            Ok(n) => {
                errors.push(format!("max_jobs must be a positive integer, got {}", n));
            }
            Err(_) => {
                errors.push(format!(
                    "max_jobs must be a positive integer, got '{}'",
                    val
                ));
            }
        }
    }

    // Validate max_submit
    if let Some(ref val) = max_submit_val {
        match val.parse::<i64>() {
            Ok(n) if n > 0 => {
                // Check max_submit >= max_jobs if both set
                if let Some(mj) = max_jobs_num {
                    if n < mj {
                        errors.push(format!("max_submit ({}) must be >= max_jobs ({})", n, mj));
                    }
                }
            }
            Ok(n) => {
                errors.push(format!("max_submit must be a positive integer, got {}", n));
            }
            Err(_) => {
                errors.push(format!(
                    "max_submit must be a positive integer, got '{}'",
                    val
                ));
            }
        }
    }

    // Validate max_wall
    if let Some(ref val) = max_wall_val {
        if val != "-1" {
            // Slurm time formats: minutes, minutes:seconds, hours:minutes:seconds,
            // days-hours, days-hours:minutes, days-hours:minutes:seconds
            let wall_re = Regex::new(r"^\d+(-\d+:\d+(:\d+)?)?$").expect("invalid regex");
            if !wall_re.is_match(val) {
                errors.push(format!(
                    "max_wall must match time format (e.g. '60', '7-00:00:00') or '-1' for unlimited, got '{}'",
                    val
                ));
            }
        }
    }

    // Validate fairshare
    if let Some(ref val) = fairshare_val {
        match val.parse::<i64>() {
            Ok(n) if n > 0 => {}
            Ok(n) => {
                errors.push(format!("fairshare must be a positive integer, got {}", n));
            }
            Err(_) => {
                errors.push(format!(
                    "fairshare must be a positive integer, got '{}'",
                    val
                ));
            }
        }
    }

    // Warnings for common misconfigurations
    if max_jobs_val.is_some() && max_submit_val.is_none() {
        warnings.push(
            "max_jobs is set but max_submit is not; users may be unable to queue jobs beyond max_jobs"
                .to_string(),
        );
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Compute an effective diff summary showing before/after for each changed field.
///
/// Includes inherited parent context when available in current properties.
fn compute_effective_diff(drift: &[DriftItem], current_props: &HashMap<String, String>) -> String {
    let mut lines = Vec::new();

    // Include parent context if available
    if let Some(parent) = current_props.get("parent") {
        lines.push(format!("parent_account: {}", parent));
    }

    for item in drift {
        let actual_display = if item.actual.is_empty() {
            "(unset)".to_string()
        } else {
            item.actual.clone()
        };
        lines.push(format!(
            "{}: '{}' -> '{}'",
            item.field, actual_display, item.desired
        ));
    }

    lines.join(", ")
}

/// Slurm QoS (Quality of Service) management module.
///
/// Manage Slurm QoS definitions via sacctmgr.
///
/// # Parameters
///
/// - `name` (required): QoS name
/// - `state` (optional): "present" (default) or "absent"
/// - `priority` (optional): QoS priority value
/// - `max_jobs_per_user` (optional): Max concurrent jobs per user
/// - `max_submit_per_user` (optional): Max submitted jobs per user
/// - `max_wall` (optional): Max wall time (e.g., "7-00:00:00")
/// - `max_tres_per_user` (optional): Max TRES per user (e.g., "cpu=100,mem=500G")
/// - `preempt` (optional): Comma-separated QoS names that this QoS can preempt
/// - `preempt_mode` (optional): Preempt mode (e.g., "cancel", "requeue", "suspend")
/// - `grace_time` (optional): Grace time in seconds before preemption
pub struct SlurmQosModule;

impl Module for SlurmQosModule {
    fn name(&self) -> &'static str {
        "slurm_qos"
    }

    fn description(&self) -> &'static str {
        "Manage Slurm QoS definitions (sacctmgr)"
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

        // Check if QoS exists
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!(
                "sacctmgr --noheader --parsable2 list qos where name={} format=Name",
                name
            ),
            context,
        )?;
        let qos_exists = ok && !stdout.trim().is_empty();

        if state == "absent" {
            if !qos_exists {
                return Ok(ModuleOutput::ok(format!("QoS '{}' does not exist", name))
                    .with_data("name", serde_json::json!(name)));
            }
            if context.check_mode {
                return Ok(
                    ModuleOutput::changed(format!("Would delete QoS '{}'", name))
                        .with_data("name", serde_json::json!(name)),
                );
            }
            run_cmd_ok(
                connection,
                &format!("sacctmgr --immediate delete qos where name={}", name),
                context,
            )?;
            return Ok(ModuleOutput::changed(format!("Deleted QoS '{}'", name))
                .with_data("name", serde_json::json!(name)));
        }

        // state=present
        let props = build_qos_properties(params)?;

        if qos_exists {
            if props.is_empty() {
                return Ok(ModuleOutput::ok(format!("QoS '{}' already exists", name))
                    .with_data("name", serde_json::json!(name)));
            }
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would update QoS '{}' with: {}",
                    name, props
                ))
                .with_data("name", serde_json::json!(name)));
            }
            run_cmd_ok(
                connection,
                &format!(
                    "sacctmgr --immediate modify qos where name={} set {}",
                    name, props
                ),
                context,
            )?;
            return Ok(ModuleOutput::changed(format!("Updated QoS '{}'", name))
                .with_data("name", serde_json::json!(name))
                .with_data("properties", serde_json::json!(props)));
        }

        // Create new QoS
        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would create QoS '{}'", name))
                    .with_data("name", serde_json::json!(name)),
            );
        }

        let mut cmd = format!("sacctmgr --immediate add qos {}", name);
        if !props.is_empty() {
            cmd.push(' ');
            cmd.push_str(&props);
        }
        run_cmd_ok(connection, &cmd, context)?;

        Ok(ModuleOutput::changed(format!("Created QoS '{}'", name))
            .with_data("name", serde_json::json!(name)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("priority", serde_json::json!(null));
        m.insert("max_jobs_per_user", serde_json::json!(null));
        m.insert("max_submit_per_user", serde_json::json!(null));
        m.insert("max_wall", serde_json::json!(null));
        m.insert("max_tres_per_user", serde_json::json!(null));
        m.insert("preempt", serde_json::json!(null));
        m.insert("preempt_mode", serde_json::json!(null));
        m.insert("grace_time", serde_json::json!(null));
        m
    }
}

/// Build sacctmgr QoS property string from params.
fn build_qos_properties(params: &ModuleParams) -> ModuleResult<String> {
    let mut props = Vec::new();
    if let Some(v) = params.get_string("priority")? {
        props.push(format!("Priority={}", v));
    }
    if let Some(v) = params.get_string("max_jobs_per_user")? {
        props.push(format!("MaxJobsPerUser={}", v));
    }
    if let Some(v) = params.get_string("max_submit_per_user")? {
        props.push(format!("MaxSubmitJobsPerUser={}", v));
    }
    if let Some(v) = params.get_string("max_wall")? {
        props.push(format!("MaxWall={}", v));
    }
    if let Some(v) = params.get_string("max_tres_per_user")? {
        props.push(format!("MaxTRESPerUser={}", v));
    }
    if let Some(v) = params.get_string("preempt")? {
        props.push(format!("Preempt={}", v));
    }
    if let Some(v) = params.get_string("preempt_mode")? {
        props.push(format!("PreemptMode={}", v));
    }
    if let Some(v) = params.get_string("grace_time")? {
        props.push(format!("GraceTime={}", v));
    }
    Ok(props.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_account_properties_full() {
        let mut params = ModuleParams::new();
        params.insert("organization".to_string(), serde_json::json!("Physics"));
        params.insert(
            "description".to_string(),
            serde_json::json!("Physics department"),
        );
        params.insert("parent".to_string(), serde_json::json!("root"));
        params.insert("fairshare".to_string(), serde_json::json!("100"));
        params.insert("max_jobs".to_string(), serde_json::json!("50"));
        params.insert("max_submit".to_string(), serde_json::json!("200"));
        params.insert("max_wall".to_string(), serde_json::json!("7-00:00:00"));

        let props = build_account_properties(&params).unwrap();
        assert!(props.contains("Organization=Physics"));
        assert!(props.contains("Description=Physics department"));
        assert!(props.contains("parent=root"));
        assert!(props.contains("fairshare=100"));
        assert!(props.contains("MaxJobs=50"));
        assert!(props.contains("MaxSubmitJobs=200"));
        assert!(props.contains("MaxWall=7-00:00:00"));
    }

    #[test]
    fn test_build_account_properties_empty() {
        let params = ModuleParams::new();
        let props = build_account_properties(&params).unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn test_build_account_properties_partial() {
        let mut params = ModuleParams::new();
        params.insert("organization".to_string(), serde_json::json!("CS"));
        params.insert("fairshare".to_string(), serde_json::json!("50"));

        let props = build_account_properties(&params).unwrap();
        assert!(props.contains("Organization=CS"));
        assert!(props.contains("fairshare=50"));
        assert!(!props.contains("Description"));
        assert!(!props.contains("MaxJobs"));
    }

    #[test]
    fn test_build_user_properties() {
        let mut params = ModuleParams::new();
        params.insert("fairshare".to_string(), serde_json::json!("10"));
        params.insert("max_jobs".to_string(), serde_json::json!("5"));
        params.insert("max_submit".to_string(), serde_json::json!("20"));
        params.insert("max_wall".to_string(), serde_json::json!("1-00:00:00"));

        let props = build_user_properties(&params).unwrap();
        assert!(props.contains("fairshare=10"));
        assert!(props.contains("MaxJobs=5"));
        assert!(props.contains("MaxSubmitJobs=20"));
        assert!(props.contains("MaxWall=1-00:00:00"));
    }

    #[test]
    fn test_build_user_properties_empty() {
        let params = ModuleParams::new();
        let props = build_user_properties(&params).unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn test_qos_module_metadata() {
        let module = SlurmQosModule;
        assert_eq!(module.name(), "slurm_qos");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_qos_required_params() {
        let module = SlurmQosModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_qos_optional_params() {
        let module = SlurmQosModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("priority"));
        assert!(optional.contains_key("max_jobs_per_user"));
        assert!(optional.contains_key("preempt"));
        assert!(optional.contains_key("preempt_mode"));
        assert!(optional.contains_key("grace_time"));
    }

    #[test]
    fn test_build_qos_properties() {
        let mut params = ModuleParams::new();
        params.insert("priority".to_string(), serde_json::json!("100"));
        params.insert("max_jobs_per_user".to_string(), serde_json::json!("10"));
        params.insert("max_wall".to_string(), serde_json::json!("2-00:00:00"));
        params.insert("preempt_mode".to_string(), serde_json::json!("cancel"));

        let props = build_qos_properties(&params).unwrap();
        assert!(props.contains("Priority=100"));
        assert!(props.contains("MaxJobsPerUser=10"));
        assert!(props.contains("MaxWall=2-00:00:00"));
        assert!(props.contains("PreemptMode=cancel"));
    }

    #[test]
    fn test_build_qos_properties_empty() {
        let params = ModuleParams::new();
        let props = build_qos_properties(&params).unwrap();
        assert!(props.is_empty());
    }

    // --- SCH-03 enhancement tests ---

    #[test]
    fn test_account_property_parsing() {
        // Simulate parsable2 output from sacctmgr with '|' separator
        // Format: Account|Description|Organization|ParentName|Fairshare|MaxJobs|MaxSubmitJobs|MaxWall
        let output = "physics|Physics department|Physics|root|100|50|200|7-00:00:00";

        let headers = [
            "account",
            "description",
            "organization",
            "parent",
            "fairshare",
            "maxjobs",
            "maxsubmitjobs",
            "maxwall",
        ];

        let mut result = HashMap::new();
        let fields: Vec<&str> = output.split('|').collect();
        for (i, header) in headers.iter().enumerate() {
            if let Some(val) = fields.get(i) {
                let val = val.trim();
                if !val.is_empty() {
                    result.insert(header.to_string(), val.to_string());
                }
            }
        }

        assert_eq!(result.get("account").unwrap(), "physics");
        assert_eq!(result.get("description").unwrap(), "Physics department");
        assert_eq!(result.get("organization").unwrap(), "Physics");
        assert_eq!(result.get("parent").unwrap(), "root");
        assert_eq!(result.get("fairshare").unwrap(), "100");
        assert_eq!(result.get("maxjobs").unwrap(), "50");
        assert_eq!(result.get("maxsubmitjobs").unwrap(), "200");
        assert_eq!(result.get("maxwall").unwrap(), "7-00:00:00");
    }

    #[test]
    fn test_account_property_parsing_partial() {
        // Some fields may be empty in parsable2 output
        let output = "physics||Physics||1|||";

        let headers = [
            "account",
            "description",
            "organization",
            "parent",
            "fairshare",
            "maxjobs",
            "maxsubmitjobs",
            "maxwall",
        ];

        let mut result = HashMap::new();
        let fields: Vec<&str> = output.split('|').collect();
        for (i, header) in headers.iter().enumerate() {
            if let Some(val) = fields.get(i) {
                let val = val.trim();
                if !val.is_empty() {
                    result.insert(header.to_string(), val.to_string());
                }
            }
        }

        assert_eq!(result.get("account").unwrap(), "physics");
        assert_eq!(result.get("organization").unwrap(), "Physics");
        assert_eq!(result.get("fairshare").unwrap(), "1");
        assert!(!result.contains_key("description"));
        assert!(!result.contains_key("parent"));
        assert!(!result.contains_key("maxjobs"));
    }

    #[test]
    fn test_policy_validation_valid() {
        let mut params = ModuleParams::new();
        params.insert("max_jobs".to_string(), serde_json::json!("50"));
        params.insert("max_submit".to_string(), serde_json::json!("200"));
        params.insert("max_wall".to_string(), serde_json::json!("7-00:00:00"));
        params.insert("fairshare".to_string(), serde_json::json!("100"));

        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_policy_validation_max_jobs_negative() {
        let mut params = ModuleParams::new();
        params.insert("max_jobs".to_string(), serde_json::json!("-5"));

        let result = validate_policies(&params).unwrap();
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("max_jobs")));
    }

    #[test]
    fn test_policy_validation_max_jobs_zero() {
        let mut params = ModuleParams::new();
        params.insert("max_jobs".to_string(), serde_json::json!("0"));

        let result = validate_policies(&params).unwrap();
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("max_jobs")));
    }

    #[test]
    fn test_policy_validation_max_jobs_non_numeric() {
        let mut params = ModuleParams::new();
        params.insert("max_jobs".to_string(), serde_json::json!("abc"));

        let result = validate_policies(&params).unwrap();
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("max_jobs")));
    }

    #[test]
    fn test_policy_validation_max_submit_less_than_max_jobs() {
        let mut params = ModuleParams::new();
        params.insert("max_jobs".to_string(), serde_json::json!("100"));
        params.insert("max_submit".to_string(), serde_json::json!("50"));

        let result = validate_policies(&params).unwrap();
        assert!(!result.passed);
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("max_submit") && e.contains("max_jobs")));
    }

    #[test]
    fn test_policy_validation_max_submit_equal_max_jobs() {
        let mut params = ModuleParams::new();
        params.insert("max_jobs".to_string(), serde_json::json!("50"));
        params.insert("max_submit".to_string(), serde_json::json!("50"));

        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_policy_validation_max_wall_invalid_format() {
        let mut params = ModuleParams::new();
        params.insert("max_wall".to_string(), serde_json::json!("invalid"));

        let result = validate_policies(&params).unwrap();
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("max_wall")));
    }

    #[test]
    fn test_policy_validation_max_wall_unlimited() {
        let mut params = ModuleParams::new();
        params.insert("max_wall".to_string(), serde_json::json!("-1"));

        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_policy_validation_max_wall_minutes_only() {
        let mut params = ModuleParams::new();
        params.insert("max_wall".to_string(), serde_json::json!("120"));

        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_policy_validation_max_wall_days_hours_minutes() {
        let mut params = ModuleParams::new();
        params.insert("max_wall".to_string(), serde_json::json!("7-00:00:00"));

        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_policy_validation_max_wall_days_hours_minutes_no_seconds() {
        let mut params = ModuleParams::new();
        params.insert("max_wall".to_string(), serde_json::json!("1-12:30"));

        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_policy_validation_fairshare_invalid() {
        let mut params = ModuleParams::new();
        params.insert("fairshare".to_string(), serde_json::json!("0"));

        let result = validate_policies(&params).unwrap();
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("fairshare")));
    }

    #[test]
    fn test_policy_validation_fairshare_default() {
        let mut params = ModuleParams::new();
        params.insert("fairshare".to_string(), serde_json::json!("1"));

        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
    }

    #[test]
    fn test_policy_validation_warning_max_jobs_without_max_submit() {
        let mut params = ModuleParams::new();
        params.insert("max_jobs".to_string(), serde_json::json!("10"));

        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
        assert!(!result.warnings.is_empty());
        assert!(result.warnings.iter().any(|w| w.contains("max_submit")));
    }

    #[test]
    fn test_policy_validation_empty_params() {
        let params = ModuleParams::new();
        let result = validate_policies(&params).unwrap();
        assert!(result.passed);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_drift_detection_no_drift() {
        let mut desired = HashMap::new();
        desired.insert("organization".to_string(), "Physics".to_string());
        desired.insert("fairshare".to_string(), "100".to_string());

        let mut current = HashMap::new();
        current.insert("organization".to_string(), "Physics".to_string());
        current.insert("fairshare".to_string(), "100".to_string());

        let drift = reconcile_account(&desired, &current);
        assert!(drift.is_empty());
    }

    #[test]
    fn test_drift_detection_case_insensitive() {
        let mut desired = HashMap::new();
        desired.insert("organization".to_string(), "physics".to_string());

        let mut current = HashMap::new();
        current.insert("organization".to_string(), "Physics".to_string());

        let drift = reconcile_account(&desired, &current);
        assert!(drift.is_empty());
    }

    #[test]
    fn test_drift_detection_with_changes() {
        let mut desired = HashMap::new();
        desired.insert("organization".to_string(), "Chemistry".to_string());
        desired.insert("fairshare".to_string(), "200".to_string());
        desired.insert("maxjobs".to_string(), "50".to_string());

        let mut current = HashMap::new();
        current.insert("organization".to_string(), "Physics".to_string());
        current.insert("fairshare".to_string(), "100".to_string());
        current.insert("maxjobs".to_string(), "50".to_string());

        let drift = reconcile_account(&desired, &current);
        assert_eq!(drift.len(), 2);
        assert!(drift.iter().any(|d| d.field == "organization"
            && d.desired == "Chemistry"
            && d.actual == "Physics"));
        assert!(drift
            .iter()
            .any(|d| d.field == "fairshare" && d.desired == "200" && d.actual == "100"));
    }

    #[test]
    fn test_drift_detection_new_field() {
        let mut desired = HashMap::new();
        desired.insert("maxjobs".to_string(), "50".to_string());

        let current = HashMap::new(); // No current properties

        let drift = reconcile_account(&desired, &current);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].field, "maxjobs");
        assert_eq!(drift[0].desired, "50");
        assert!(drift[0].actual.is_empty());
    }

    #[test]
    fn test_build_set_clause_from_drift() {
        let drift = vec![
            DriftItem {
                field: "fairshare".to_string(),
                desired: "200".to_string(),
                actual: "100".to_string(),
            },
            DriftItem {
                field: "maxjobs".to_string(),
                desired: "50".to_string(),
                actual: "25".to_string(),
            },
        ];

        let clause = build_set_clause_from_drift(&drift);
        assert!(clause.contains("fairshare=200"));
        assert!(clause.contains("MaxJobs=50"));
    }

    #[test]
    fn test_compute_effective_diff() {
        let drift = vec![DriftItem {
            field: "fairshare".to_string(),
            desired: "200".to_string(),
            actual: "100".to_string(),
        }];

        let mut current = HashMap::new();
        current.insert("parent".to_string(), "root".to_string());

        let summary = compute_effective_diff(&drift, &current);
        assert!(summary.contains("parent_account: root"));
        assert!(summary.contains("fairshare: '100' -> '200'"));
    }

    #[test]
    fn test_compute_effective_diff_unset_field() {
        let drift = vec![DriftItem {
            field: "maxjobs".to_string(),
            desired: "50".to_string(),
            actual: String::new(),
        }];

        let current = HashMap::new();

        let summary = compute_effective_diff(&drift, &current);
        assert!(summary.contains("maxjobs: '(unset)' -> '50'"));
    }

    #[test]
    fn test_build_desired_properties() {
        let mut params = ModuleParams::new();
        params.insert("organization".to_string(), serde_json::json!("Physics"));
        params.insert("max_jobs".to_string(), serde_json::json!("50"));
        params.insert("max_submit".to_string(), serde_json::json!("200"));

        let desired = build_desired_properties(&params).unwrap();
        assert_eq!(desired.get("organization").unwrap(), "Physics");
        assert_eq!(desired.get("maxjobs").unwrap(), "50");
        assert_eq!(desired.get("maxsubmitjobs").unwrap(), "200");
        assert!(!desired.contains_key("fairshare"));
    }

    #[test]
    fn test_validate_policies_param_in_optional_params() {
        let module = SlurmAccountModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("validate_policies"));
    }
}
