//! OpenSM (InfiniBand Subnet Manager) configuration module
//!
//! Manage OpenSM configuration including opensm.conf, subnet prefix,
//! routing engine, log level, and service state.
//!
//! # Parameters
//!
//! - `subnet_prefix` (optional): IB subnet prefix (e.g., "0xfe80000000000000")
//! - `routing_engine` (optional): Routing algorithm (e.g., "minhop", "ftree")
//! - `log_level` (optional): Log verbosity (0-255)
//! - `state` (optional): "present" (default) or "absent"

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

fn detect_os_family(os_release: &str) -> Option<&'static str> {
    let id_line = os_release
        .lines()
        .find(|l| l.starts_with("ID_LIKE=") || l.starts_with("ID="));
    match id_line {
        Some(line) => {
            let val = line
                .split('=')
                .nth(1)
                .unwrap_or("")
                .trim_matches('"')
                .to_lowercase();
            if val.contains("rhel")
                || val.contains("fedora")
                || val.contains("centos")
                || val == "rocky"
                || val == "almalinux"
            {
                Some("rhel")
            } else if val.contains("debian") || val.contains("ubuntu") {
                Some("debian")
            } else {
                None
            }
        }
        None => None,
    }
}

pub struct OpensmConfigModule;

impl Module for OpensmConfigModule {
    fn name(&self) -> &'static str {
        "opensm_config"
    }

    fn description(&self) -> &'static str {
        "Manage OpenSM InfiniBand subnet manager configuration"
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

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported("Unsupported OS for OpenSM module".to_string())
        })?;

        if state == "absent" {
            return self.handle_absent(connection, os_family, context);
        }

        let subnet_prefix = params.get_string("subnet_prefix")?;
        let routing_engine = params.get_string("routing_engine")?;
        let log_level = params.get_string("log_level")?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Install OpenSM
        let check_cmd = match os_family {
            "rhel" => "rpm -q opensm >/dev/null 2>&1",
            _ => "dpkg -s opensm >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install OpenSM".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y opensm",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y opensm",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed OpenSM".to_string());
            }
        }

        // Configure opensm.conf
        let conf_path = "/etc/opensm/opensm.conf";
        let (conf_exists, _, _) = run_cmd(connection, &format!("test -f {}", conf_path), context)?;

        if !conf_exists && !context.check_mode {
            run_cmd_ok(connection, "mkdir -p /etc/opensm", context)?;
        }

        let mut config_lines = Vec::new();
        if let Some(ref prefix) = subnet_prefix {
            config_lines.push(format!("subnet_prefix {}", prefix));
        }
        if let Some(ref engine) = routing_engine {
            config_lines.push(format!("routing_engine {}", engine));
        }
        if let Some(ref level) = log_level {
            config_lines.push(format!("log_flags {}", level));
        }

        if !config_lines.is_empty() {
            if context.check_mode {
                changes.push(format!("Would update {}", conf_path));
            } else {
                let config_content = config_lines.join("\n") + "\n";
                let escaped = config_content.replace('\'', "'\\''");
                run_cmd_ok(
                    connection,
                    &format!("echo '{}' > {}", escaped, conf_path),
                    context,
                )?;
                changed = true;
                changes.push(format!("Updated {}", conf_path));
            }
        }

        // Enable and start OpenSM service
        if !context.check_mode {
            let (active, _, _) = run_cmd(connection, "systemctl is-active opensm", context)?;
            if !active {
                run_cmd_ok(connection, "systemctl enable --now opensm", context)?;
                changed = true;
                changes.push("Started OpenSM service".to_string());
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} OpenSM changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} OpenSM changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok("OpenSM is configured"))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("subnet_prefix", serde_json::json!(null));
        m.insert("routing_engine", serde_json::json!(null));
        m.insert("log_level", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

impl OpensmConfigModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        os_family: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let check_cmd = match os_family {
            "rhel" => "rpm -q opensm >/dev/null 2>&1",
            _ => "dpkg -s opensm >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            return Ok(ModuleOutput::ok("OpenSM is not installed"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would remove OpenSM"));
        }

        let _ = run_cmd(connection, "systemctl stop opensm", context);
        let _ = run_cmd(connection, "systemctl disable opensm", context);

        let remove_cmd = match os_family {
            "rhel" => "dnf remove -y opensm",
            _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y opensm",
        };
        run_cmd_ok(connection, remove_cmd, context)?;

        Ok(ModuleOutput::changed("Removed OpenSM"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = OpensmConfigModule;
        assert_eq!(module.name(), "opensm_config");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = OpensmConfigModule;
        let required = module.required_params();
        assert_eq!(required.len(), 0);
    }

    #[test]
    fn test_optional_params() {
        let module = OpensmConfigModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("subnet_prefix"));
        assert!(optional.contains_key("routing_engine"));
        assert!(optional.contains_key("log_level"));
        assert!(optional.contains_key("state"));
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID=rhel\nVERSION=8"), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID_LIKE=\"rhel fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=unknown"), None);
    }
}
