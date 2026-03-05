//! Munge authentication module (Slurm prerequisite)
//!
//! Manages MUNGE (MUNGE Uid 'N' Gid Emporium) authentication service
//! installation, key distribution, and service management.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `key_source` (optional): Path to munge.key on the control node
//! - `key_content` (optional): Base64-encoded munge key content
//! - `munge_user` (optional): User to own munge files (default: "munge")
//! - `munge_group` (optional): Group to own munge files (default: "munge")

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

pub struct MungeModule;

impl MungeModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        os_family: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let check_cmd = match os_family {
            "rhel" => "rpm -q munge >/dev/null 2>&1",
            _ => "dpkg -s munge >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            return Ok(ModuleOutput::ok("Munge is not installed"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would remove munge"));
        }

        // Stop and disable service (ignore errors if service doesn't exist)
        let _ = run_cmd(connection, "systemctl stop munge.service", context);
        let _ = run_cmd(connection, "systemctl disable munge.service", context);

        // Remove packages
        let remove_cmd = match os_family {
            "rhel" => "dnf remove -y munge munge-libs munge-devel",
            _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y munge libmunge2 libmunge-dev",
        };
        run_cmd_ok(connection, remove_cmd, context)?;

        Ok(ModuleOutput::changed("Removed munge"))
    }
}

impl Module for MungeModule {
    fn name(&self) -> &'static str {
        "munge"
    }

    fn description(&self) -> &'static str {
        "Manage MUNGE authentication service for HPC clusters"
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
        let munge_user = params
            .get_string("munge_user")?
            .unwrap_or_else(|| "munge".to_string());
        let munge_group = params
            .get_string("munge_group")?
            .unwrap_or_else(|| "munge".to_string());

        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported(
                "Unsupported OS. Munge module supports RHEL-family and Debian-family.".to_string(),
            )
        })?;

        if state == "absent" {
            return self.handle_absent(connection, os_family, context);
        }

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Install munge packages
        let check_cmd = match os_family {
            "rhel" => "rpm -q munge munge-libs >/dev/null 2>&1",
            _ => "dpkg -s munge libmunge2 >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install munge packages".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y munge munge-libs munge-devel",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y munge libmunge2 libmunge-dev",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed munge packages".to_string());
            }
        }

        // Ensure directories exist with correct permissions
        if !context.check_mode {
            run_cmd_ok(
                connection,
                &format!(
                    "mkdir -p /etc/munge /var/log/munge /var/lib/munge /run/munge && \
                     chown {user}:{group} /etc/munge /var/log/munge /var/lib/munge /run/munge && \
                     chmod 0700 /etc/munge /var/log/munge /var/lib/munge && \
                     chmod 0755 /run/munge",
                    user = munge_user,
                    group = munge_group
                ),
                context,
            )?;
        }

        // Distribute munge key
        let key_content = params.get_string("key_content")?;
        let key_source = params.get_string("key_source")?;

        if key_content.is_some() || key_source.is_some() {
            let key_written = if let Some(ref content) = key_content {
                if context.check_mode {
                    changes.push("Would distribute munge key".to_string());
                    false
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("echo '{}' | base64 -d > /etc/munge/munge.key", content),
                        context,
                    )?;
                    true
                }
            } else if let Some(ref source) = key_source {
                if context.check_mode {
                    changes.push(format!("Would copy munge key from {}", source));
                    false
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("cp '{}' /etc/munge/munge.key", source),
                        context,
                    )?;
                    true
                }
            } else {
                false
            };

            if key_written {
                run_cmd_ok(
                    connection,
                    &format!(
                        "chown {}:{} /etc/munge/munge.key && chmod 0400 /etc/munge/munge.key",
                        munge_user, munge_group
                    ),
                    context,
                )?;
                changed = true;
                changes.push("Distributed munge key".to_string());
            }
        } else {
            // Check if key exists, generate if not
            let (key_exists, _, _) = run_cmd(connection, "test -f /etc/munge/munge.key", context)?;
            if !key_exists {
                if context.check_mode {
                    changes.push("Would generate new munge key".to_string());
                } else {
                    run_cmd_ok(connection, "mungekey --create --force", context)?;
                    run_cmd_ok(
                        connection,
                        &format!(
                            "chown {}:{} /etc/munge/munge.key && chmod 0400 /etc/munge/munge.key",
                            munge_user, munge_group
                        ),
                        context,
                    )?;
                    changed = true;
                    changes.push("Generated new munge key".to_string());
                }
            }
        }

        // Enable and start munge service
        let (service_active, _, _) =
            run_cmd(connection, "systemctl is-active munge.service", context)?;
        let (service_enabled, _, _) =
            run_cmd(connection, "systemctl is-enabled munge.service", context)?;

        if !service_enabled || !service_active {
            if context.check_mode {
                changes.push("Would enable and start munge.service".to_string());
            } else {
                run_cmd_ok(connection, "systemctl enable --now munge.service", context)?;
                changed = true;
                changes.push("Enabled and started munge.service".to_string());
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} munge changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} munge changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok("Munge is configured and running"))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("key_source", serde_json::json!(null));
        m.insert("key_content", serde_json::json!(null));
        m.insert("munge_user", serde_json::json!("munge"));
        m.insert("munge_group", serde_json::json!("munge"));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::{detect_os_family, MungeModule};
    use crate::modules::Module;

    #[test]
    fn test_detect_os_family_for_munge() {
        assert_eq!(detect_os_family("ID=ubuntu\nID_LIKE=debian"), Some("debian"));
        assert_eq!(detect_os_family("ID=rocky\nID_LIKE=rhel"), Some("rhel"));
        assert_eq!(detect_os_family("ID=arch"), None);
    }

    #[test]
    fn test_munge_module_metadata() {
        let module = MungeModule;
        assert_eq!(module.name(), "munge");
        assert!(!module.description().is_empty());
        assert!(module.required_params().is_empty());
    }

    #[test]
    fn test_munge_optional_params_defaults() {
        let module = MungeModule;
        let optional = module.optional_params();
        assert_eq!(optional.get("state"), Some(&serde_json::json!("present")));
        assert_eq!(optional.get("munge_user"), Some(&serde_json::json!("munge")));
        assert_eq!(optional.get("munge_group"), Some(&serde_json::json!("munge")));
        assert!(optional.contains_key("key_source"));
        assert!(optional.contains_key("key_content"));
    }
}
