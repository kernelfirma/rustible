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

        let props = build_account_properties(params)?;
        if props.is_empty() {
            return Ok(ModuleOutput::ok(format!(
                "No properties to update for account '{}'",
                account
            ))
            .with_data("account", serde_json::json!(account)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would update account '{}' with: {}",
                account, props
            ))
            .with_data("account", serde_json::json!(account)));
        }

        let mut cmd = format!(
            "sacctmgr --immediate modify account where name={} set {}",
            account, props
        );
        if let Some(cluster) = params.get_string("cluster")? {
            cmd = format!(
                "sacctmgr --immediate modify account where name={} cluster={} set {}",
                account, cluster, props
            );
        }

        run_cmd_ok(connection, &cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Updated account '{}'", account))
                .with_data("account", serde_json::json!(account))
                .with_data("properties", serde_json::json!(props)),
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
}
