//! PXE (Preboot Execution Environment) management modules
//!
//! Manage PXE boot profiles and host assignments.
//!
//! # Modules
//!
//! - `pxe_profile`: Manage PXE boot profiles (kernel, initrd, append parameters)
//! - `pxe_host`: Associate hosts (by MAC address) with PXE profiles

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

const PXELINUX_CFG_DIR: &str = "/var/lib/tftpboot/pxelinux.cfg";

// ---- PXE Profile Module ----

pub struct PxeProfileModule;

impl Module for PxeProfileModule {
    fn name(&self) -> &'static str {
        "pxe_profile"
    }

    fn description(&self) -> &'static str {
        "Manage PXE boot profiles (kernel, initrd, boot parameters)"
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

        let profile_name = params.get_string_required("name")?;
        let kernel = params.get_string("kernel")?;
        let initrd = params.get_string("initrd")?;
        let append = params.get_string("append")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        let profile_file = format!("{}/{}", PXELINUX_CFG_DIR, profile_name);

        if state == "absent" {
            let (exists, _, _) =
                run_cmd(connection, &format!("test -f {}", profile_file), context)?;

            if !exists {
                return Ok(
                    ModuleOutput::ok(format!("PXE profile '{}' not present", profile_name))
                        .with_data("profile", serde_json::json!(profile_name)),
                );
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would remove PXE profile '{}'",
                    profile_name
                ))
                .with_data("profile", serde_json::json!(profile_name)));
            }

            run_cmd_ok(connection, &format!("rm -f {}", profile_file), context)?;
            return Ok(
                ModuleOutput::changed(format!("Removed PXE profile '{}'", profile_name))
                    .with_data("profile", serde_json::json!(profile_name)),
            );
        }

        // Generate profile content
        let mut profile_content = String::from("DEFAULT linux\nLABEL linux\n");
        if let Some(ref k) = kernel {
            profile_content.push_str(&format!("  KERNEL {}\n", k));
        }
        if let Some(ref i) = initrd {
            profile_content.push_str(&format!("  INITRD {}\n", i));
        }
        if let Some(ref a) = append {
            profile_content.push_str(&format!("  APPEND {}\n", a));
        }

        let (exists, current_content, _) = run_cmd(
            connection,
            &format!("cat {} 2>/dev/null || echo ''", profile_file),
            context,
        )?;

        if exists && current_content == profile_content {
            return Ok(
                ModuleOutput::ok(format!("PXE profile '{}' is up to date", profile_name))
                    .with_data("profile", serde_json::json!(profile_name)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would {} PXE profile '{}'",
                if exists { "update" } else { "create" },
                profile_name
            ))
            .with_data("profile", serde_json::json!(profile_name)));
        }

        run_cmd_ok(
            connection,
            &format!("mkdir -p {}", PXELINUX_CFG_DIR),
            context,
        )?;

        let escaped = profile_content.replace('\'', "'\\''");
        run_cmd_ok(
            connection,
            &format!("echo '{}' > {}", escaped, profile_file),
            context,
        )?;

        Ok(ModuleOutput::changed(format!(
            "{} PXE profile '{}'",
            if exists { "Updated" } else { "Created" },
            profile_name
        ))
        .with_data("profile", serde_json::json!(profile_name)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("kernel", serde_json::json!(null));
        m.insert("initrd", serde_json::json!(null));
        m.insert("append", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

// ---- PXE Host Module ----

pub struct PxeHostModule;

impl Module for PxeHostModule {
    fn name(&self) -> &'static str {
        "pxe_host"
    }

    fn description(&self) -> &'static str {
        "Associate hosts (by MAC address) with PXE boot profiles"
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

        let mac = params.get_string_required("mac")?;
        let profile = params.get_string_required("profile")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        // Convert MAC to pxelinux format (01-aa-bb-cc-dd-ee-ff)
        let pxe_mac = format!("01-{}", mac.replace(':', "-").to_lowercase());
        let host_file = format!("{}/{}", PXELINUX_CFG_DIR, pxe_mac);

        if state == "absent" {
            let (exists, _, _) = run_cmd(connection, &format!("test -f {}", host_file), context)?;

            if !exists {
                return Ok(
                    ModuleOutput::ok(format!("PXE host entry for {} not present", mac))
                        .with_data("mac", serde_json::json!(mac)),
                );
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would remove PXE host entry for {}",
                    mac
                ))
                .with_data("mac", serde_json::json!(mac)));
            }

            run_cmd_ok(connection, &format!("rm -f {}", host_file), context)?;
            return Ok(
                ModuleOutput::changed(format!("Removed PXE host entry for {}", mac))
                    .with_data("mac", serde_json::json!(mac)),
            );
        }

        // Create symlink to profile
        let (exists, current_link, _) = run_cmd(
            connection,
            &format!("readlink {} 2>/dev/null || echo ''", host_file),
            context,
        )?;

        if exists && current_link.trim() == profile {
            return Ok(ModuleOutput::ok(format!(
                "PXE host {} already linked to profile '{}'",
                mac, profile
            ))
            .with_data("mac", serde_json::json!(mac))
            .with_data("profile", serde_json::json!(profile)));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would link {} to profile '{}'",
                mac, profile
            ))
            .with_data("mac", serde_json::json!(mac))
            .with_data("profile", serde_json::json!(profile)));
        }

        run_cmd_ok(
            connection,
            &format!("mkdir -p {}", PXELINUX_CFG_DIR),
            context,
        )?;

        if exists {
            run_cmd_ok(connection, &format!("rm -f {}", host_file), context)?;
        }

        run_cmd_ok(
            connection,
            &format!("ln -s {} {}", profile, host_file),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Linked {} to profile '{}'", mac, profile))
                .with_data("mac", serde_json::json!(mac))
                .with_data("profile", serde_json::json!(profile)),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &["mac", "profile"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pxe_profile_module_metadata() {
        let module = PxeProfileModule;
        assert_eq!(module.name(), "pxe_profile");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_pxe_profile_required_params() {
        let module = PxeProfileModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_pxe_host_module_metadata() {
        let module = PxeHostModule;
        assert_eq!(module.name(), "pxe_host");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_pxe_host_required_params() {
        let module = PxeHostModule;
        let required = module.required_params();
        assert!(required.contains(&"mac"));
        assert!(required.contains(&"profile"));
    }

    #[test]
    fn test_mac_format_conversion() {
        let mac = "aa:bb:cc:dd:ee:ff";
        let pxe_mac = format!("01-{}", mac.replace(':', "-").to_lowercase());
        assert_eq!(pxe_mac, "01-aa-bb-cc-dd-ee-ff");
    }
}
