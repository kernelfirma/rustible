//! Warewulf cluster management modules
//!
//! Manage Warewulf node and image configurations via wwctl CLI.
//!
//! # Modules
//!
//! - `warewulf_node`: Manage compute node definitions
//! - `warewulf_image`: Manage node images (containers/chroots)

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

// ---- Warewulf Node Module ----

pub struct WarewulfNodeModule;

impl Module for WarewulfNodeModule {
    fn name(&self) -> &'static str {
        "warewulf_node"
    }

    fn description(&self) -> &'static str {
        "Manage Warewulf compute node definitions via wwctl"
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

        let node_name = params.get_string_required("name")?;
        let image = params.get_string("image")?;
        let network = params.get_string("network")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        // Check if wwctl is available
        let (wwctl_ok, _, _) = run_cmd(connection, "which wwctl", context)?;
        if !wwctl_ok {
            return Err(ModuleError::ExecutionFailed(
                "wwctl command not found. Ensure Warewulf is installed.".to_string(),
            ));
        }

        // Check if node exists
        let (node_exists, _, _) = run_cmd(
            connection,
            &format!(
                "wwctl node list {} 2>/dev/null | grep -q '{}'",
                node_name, node_name
            ),
            context,
        )?;

        if state == "absent" {
            if !node_exists {
                return Ok(
                    ModuleOutput::ok(format!("Warewulf node '{}' not present", node_name))
                        .with_data("node", serde_json::json!(node_name)),
                );
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would delete Warewulf node '{}'",
                    node_name
                ))
                .with_data("node", serde_json::json!(node_name)));
            }

            run_cmd_ok(
                connection,
                &format!("wwctl node delete {}", node_name),
                context,
            )?;

            return Ok(
                ModuleOutput::changed(format!("Deleted Warewulf node '{}'", node_name))
                    .with_data("node", serde_json::json!(node_name)),
            );
        }

        if node_exists {
            return Ok(
                ModuleOutput::ok(format!("Warewulf node '{}' already exists", node_name))
                    .with_data("node", serde_json::json!(node_name)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would create Warewulf node '{}'",
                node_name
            ))
            .with_data("node", serde_json::json!(node_name)));
        }

        let mut add_cmd = format!("wwctl node add {}", node_name);
        if let Some(ref img) = image {
            add_cmd.push_str(&format!(" --container {}", img));
        }
        if let Some(ref net) = network {
            add_cmd.push_str(&format!(" --netname {}", net));
        }

        run_cmd_ok(connection, &add_cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Created Warewulf node '{}'", node_name))
                .with_data("node", serde_json::json!(node_name))
                .with_data("image", serde_json::json!(image))
                .with_data("network", serde_json::json!(network)),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("image", serde_json::json!(null));
        m.insert("network", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

// ---- Warewulf Image Module ----

pub struct WarewulfImageModule;

impl Module for WarewulfImageModule {
    fn name(&self) -> &'static str {
        "warewulf_image"
    }

    fn description(&self) -> &'static str {
        "Manage Warewulf node images (containers/chroots) via wwctl"
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

        let image_name = params.get_string_required("name")?;
        let chroot = params.get_string("chroot")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        // Check if wwctl is available
        let (wwctl_ok, _, _) = run_cmd(connection, "which wwctl", context)?;
        if !wwctl_ok {
            return Err(ModuleError::ExecutionFailed(
                "wwctl command not found. Ensure Warewulf is installed.".to_string(),
            ));
        }

        // Check if image exists
        let (image_exists, _, _) = run_cmd(
            connection,
            &format!(
                "wwctl container list 2>/dev/null | grep -q '{}'",
                image_name
            ),
            context,
        )?;

        if state == "absent" {
            if !image_exists {
                return Ok(ModuleOutput::ok(format!(
                    "Warewulf image '{}' not present",
                    image_name
                ))
                .with_data("image", serde_json::json!(image_name)));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would delete Warewulf image '{}'",
                    image_name
                ))
                .with_data("image", serde_json::json!(image_name)));
            }

            run_cmd_ok(
                connection,
                &format!("wwctl container delete {}", image_name),
                context,
            )?;

            return Ok(
                ModuleOutput::changed(format!("Deleted Warewulf image '{}'", image_name))
                    .with_data("image", serde_json::json!(image_name)),
            );
        }

        if image_exists {
            return Ok(
                ModuleOutput::ok(format!("Warewulf image '{}' already exists", image_name))
                    .with_data("image", serde_json::json!(image_name)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would import Warewulf image '{}'",
                image_name
            ))
            .with_data("image", serde_json::json!(image_name)));
        }

        let import_cmd = if let Some(ref ch) = chroot {
            format!("wwctl container import {} {}", ch, image_name)
        } else {
            return Err(ModuleError::InvalidParameter(
                "Parameter 'chroot' is required for creating images".to_string(),
            ));
        };

        run_cmd_ok(connection, &import_cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Imported Warewulf image '{}'", image_name))
                .with_data("image", serde_json::json!(image_name))
                .with_data("chroot", serde_json::json!(chroot)),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("chroot", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warewulf_node_module_metadata() {
        let module = WarewulfNodeModule;
        assert_eq!(module.name(), "warewulf_node");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_warewulf_node_required_params() {
        let module = WarewulfNodeModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_warewulf_image_module_metadata() {
        let module = WarewulfImageModule;
        assert_eq!(module.name(), "warewulf_image");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_warewulf_image_required_params() {
        let module = WarewulfImageModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_warewulf_optional_params() {
        let node_module = WarewulfNodeModule;
        let node_optional = node_module.optional_params();
        assert!(node_optional.contains_key("image"));
        assert!(node_optional.contains_key("network"));
        assert!(node_optional.contains_key("state"));

        let image_module = WarewulfImageModule;
        let image_optional = image_module.optional_params();
        assert!(image_optional.contains_key("chroot"));
        assert!(image_optional.contains_key("state"));
    }
}
