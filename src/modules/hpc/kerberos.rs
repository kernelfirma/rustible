//! Kerberos client configuration module
//!
//! Manage Kerberos authentication client setup including krb5.conf,
//! keytab deployment, and kinit testing.
//!
//! # Parameters
//!
//! - `realm` (required): Kerberos realm (e.g., "EXAMPLE.COM")
//! - `kdc` (required): KDC server (e.g., "kdc.example.com")
//! - `admin_server` (optional): Admin server (defaults to KDC)
//! - `keytab_src` (optional): Path to keytab file on control node
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

pub struct KerberosClientModule;

impl Module for KerberosClientModule {
    fn name(&self) -> &'static str {
        "kerberos_client"
    }

    fn description(&self) -> &'static str {
        "Manage Kerberos client configuration (krb5.conf, keytabs)"
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
            ModuleError::Unsupported(
                "Unsupported OS. Kerberos module supports RHEL-family and Debian-family."
                    .to_string(),
            )
        })?;

        if state == "absent" {
            return self.handle_absent(connection, os_family, context);
        }

        let realm = params.get_string_required("realm")?;
        let kdc = params.get_string_required("kdc")?;
        let admin_server = params
            .get_string("admin_server")?
            .unwrap_or_else(|| kdc.clone());
        let keytab_src = params.get_string("keytab_src")?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Install Kerberos packages
        let check_cmd = match os_family {
            "rhel" => "rpm -q krb5-workstation >/dev/null 2>&1",
            _ => "dpkg -s krb5-user >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install Kerberos packages".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y krb5-workstation krb5-libs",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y krb5-user libkrb5-3",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed Kerberos packages".to_string());
            }
        }

        // Generate krb5.conf
        let krb5_conf = format!(
            r#"[libdefaults]
    default_realm = {}
    dns_lookup_realm = false
    dns_lookup_kdc = false

[realms]
    {} = {{
        kdc = {}
        admin_server = {}
    }}

[domain_realm]
    .{} = {}
    {} = {}
"#,
            realm,
            realm,
            kdc,
            admin_server,
            realm.to_lowercase(),
            realm,
            realm.to_lowercase(),
            realm
        );

        // Check if krb5.conf needs update
        let (krb5_exists, current_conf, _) =
            run_cmd(connection, "cat /etc/krb5.conf 2>/dev/null", context)?;
        let needs_update = !krb5_exists || current_conf != krb5_conf;

        if needs_update {
            if context.check_mode {
                changes.push("Would update /etc/krb5.conf".to_string());
            } else {
                let escaped = krb5_conf.replace('\'', "'\\''");
                run_cmd_ok(
                    connection,
                    &format!("echo '{}' > /etc/krb5.conf", escaped),
                    context,
                )?;
                changed = true;
                changes.push("Updated /etc/krb5.conf".to_string());
            }
        }

        // Deploy keytab if provided
        if let Some(ref keytab) = keytab_src {
            let (keytab_exists, _, _) = run_cmd(connection, "test -f /etc/krb5.keytab", context)?;
            if !keytab_exists {
                if context.check_mode {
                    changes.push(format!("Would deploy keytab from {}", keytab));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!(
                            "cp '{}' /etc/krb5.keytab && chmod 600 /etc/krb5.keytab",
                            keytab
                        ),
                        context,
                    )?;
                    changed = true;
                    changes.push("Deployed keytab".to_string());
                }
            }
        }

        // Test kinit
        if !context.check_mode {
            let (kinit_ok, _, _) = run_cmd(connection, "klist -s 2>/dev/null", context)?;
            if !kinit_ok {
                changes.push("Kerberos configured but no valid ticket".to_string());
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Kerberos changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} Kerberos changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes))
                    .with_data("realm", serde_json::json!(realm)),
            )
        } else {
            Ok(ModuleOutput::ok("Kerberos client is configured")
                .with_data("realm", serde_json::json!(realm)))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["realm", "kdc"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("admin_server", serde_json::json!(null));
        m.insert("keytab_src", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

impl KerberosClientModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        os_family: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let check_cmd = match os_family {
            "rhel" => "rpm -q krb5-workstation >/dev/null 2>&1",
            _ => "dpkg -s krb5-user >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            return Ok(ModuleOutput::ok("Kerberos is not installed"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would remove Kerberos client"));
        }

        let remove_cmd = match os_family {
            "rhel" => "dnf remove -y krb5-workstation krb5-libs",
            _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y krb5-user libkrb5-3",
        };
        run_cmd_ok(connection, remove_cmd, context)?;

        Ok(ModuleOutput::changed("Removed Kerberos client"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = KerberosClientModule;
        assert_eq!(module.name(), "kerberos_client");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = KerberosClientModule;
        let required = module.required_params();
        assert!(required.contains(&"realm"));
        assert!(required.contains(&"kdc"));
    }

    #[test]
    fn test_optional_params() {
        let module = KerberosClientModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("admin_server"));
        assert!(optional.contains_key("keytab_src"));
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
