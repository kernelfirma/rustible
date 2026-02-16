//! InfiniBand partition management module
//!
//! Manage IB partition keys via partitions.conf for OpenSM.
//!
//! # Parameters
//!
//! - `pkey` (required): Partition key (hex format, e.g., "0x7fff")
//! - `members` (optional): List of node GUIDs/names
//! - `state` (optional): "present" (default) or "absent"
//! - `ipoib` (optional): Enable IPoIB for this partition (boolean)

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

pub struct IbPartitionModule;

impl Module for IbPartitionModule {
    fn name(&self) -> &'static str {
        "ib_partition"
    }

    fn description(&self) -> &'static str {
        "Manage InfiniBand partition keys via partitions.conf"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::HostExclusive
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

        let pkey = params.get_string_required("pkey")?;
        let members = params.get_vec_string("members")?.unwrap_or_default();
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let ipoib = params.get_bool_or("ipoib", false);

        let partitions_conf = "/etc/opensm/partitions.conf";

        // Ensure partitions.conf exists
        let (conf_exists, _, _) =
            run_cmd(connection, &format!("test -f {}", partitions_conf), context)?;

        if !conf_exists && !context.check_mode {
            run_cmd_ok(connection, "mkdir -p /etc/opensm", context)?;
            run_cmd_ok(connection, &format!("touch {}", partitions_conf), context)?;
        }

        let (_, current_conf, _) = run_cmd(
            connection,
            &format!("cat {} 2>/dev/null || echo ''", partitions_conf),
            context,
        )?;

        let pkey_line_prefix = format!("{}:", pkey);
        let pkey_exists = current_conf
            .lines()
            .any(|line| line.trim().starts_with(&pkey_line_prefix));

        if state == "absent" {
            if !pkey_exists {
                return Ok(
                    ModuleOutput::ok(format!("Partition key {} not present", pkey))
                        .with_data("pkey", serde_json::json!(pkey)),
                );
            }

            if context.check_mode {
                return Ok(
                    ModuleOutput::changed(format!("Would remove partition key {}", pkey))
                        .with_data("pkey", serde_json::json!(pkey)),
                );
            }

            run_cmd_ok(
                connection,
                &format!("sed -i '/^{}/d' {}", pkey, partitions_conf),
                context,
            )?;

            return Ok(
                ModuleOutput::changed(format!("Removed partition key {}", pkey))
                    .with_data("pkey", serde_json::json!(pkey)),
            );
        }

        if pkey_exists {
            return Ok(
                ModuleOutput::ok(format!("Partition key {} already configured", pkey))
                    .with_data("pkey", serde_json::json!(pkey)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would add partition key {}", pkey))
                    .with_data("pkey", serde_json::json!(pkey)),
            );
        }

        let members_str = if members.is_empty() {
            "ALL".to_string()
        } else {
            members.join(",")
        };

        let ipoib_flag = if ipoib { ",ipoib" } else { "" };
        let pkey_line = format!("{}{}={}\n", pkey, ipoib_flag, members_str);

        let escaped = pkey_line.replace('\'', "'\\''");
        run_cmd_ok(
            connection,
            &format!("echo '{}' >> {}", escaped, partitions_conf),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Added partition key {}", pkey))
                .with_data("pkey", serde_json::json!(pkey))
                .with_data("members", serde_json::json!(members)),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &["pkey"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("members", serde_json::json!([]));
        m.insert("state", serde_json::json!("present"));
        m.insert("ipoib", serde_json::json!(false));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = IbPartitionModule;
        assert_eq!(module.name(), "ib_partition");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = IbPartitionModule;
        let required = module.required_params();
        assert!(required.contains(&"pkey"));
    }

    #[test]
    fn test_optional_params() {
        let module = IbPartitionModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("members"));
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("ipoib"));
    }

    #[test]
    fn test_partition_line_format() {
        let pkey = "0x7fff";
        let members = vec!["node1".to_string(), "node2".to_string()];
        let members_str = members.join(",");
        let line = format!("{}={}\n", pkey, members_str);
        assert!(line.contains("0x7fff"));
        assert!(line.contains("node1,node2"));
    }
}
