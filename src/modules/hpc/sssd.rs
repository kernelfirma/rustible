//! SSSD (System Security Services Daemon) configuration modules
//!
//! Manage SSSD configuration for centralized authentication and identity.
//!
//! # Modules
//!
//! - `sssd_config`: Manage main sssd.conf with services and domains
//! - `sssd_domain`: Manage per-domain configuration

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

// ---- SSSD Config Module ----

pub struct SssdConfigModule;

impl Module for SssdConfigModule {
    fn name(&self) -> &'static str {
        "sssd_config"
    }

    fn description(&self) -> &'static str {
        "Manage SSSD main configuration (sssd.conf services and domains)"
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
            ModuleError::Unsupported("Unsupported OS for SSSD module".to_string())
        })?;

        if state == "absent" {
            return self.handle_absent(connection, os_family, context);
        }

        let services = params
            .get_vec_string("services")?
            .ok_or_else(|| ModuleError::InvalidParameter("services is required".to_string()))?;
        let domains = params
            .get_vec_string("domains")?
            .ok_or_else(|| ModuleError::InvalidParameter("domains is required".to_string()))?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Install SSSD
        let check_cmd = match os_family {
            "rhel" => "rpm -q sssd >/dev/null 2>&1",
            _ => "dpkg -s sssd >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push("Would install SSSD".to_string());
            } else {
                let install_cmd = match os_family {
                    "rhel" => "dnf install -y sssd sssd-tools",
                    _ => "DEBIAN_FRONTEND=noninteractive apt-get install -y sssd sssd-tools",
                };
                run_cmd_ok(connection, install_cmd, context)?;
                changed = true;
                changes.push("Installed SSSD".to_string());
            }
        }

        // Generate sssd.conf
        let sssd_conf = format!(
            "[sssd]\nservices = {}\ndomains = {}\n\n",
            services.join(", "),
            domains.join(", ")
        );

        let (conf_exists, current_conf, _) =
            run_cmd(connection, "cat /etc/sssd/sssd.conf 2>/dev/null", context)?;
        let needs_update = !conf_exists || !current_conf.contains(&sssd_conf);

        if needs_update {
            if context.check_mode {
                changes.push("Would update /etc/sssd/sssd.conf".to_string());
            } else {
                run_cmd_ok(connection, "mkdir -p /etc/sssd", context)?;
                let escaped = sssd_conf.replace('\'', "'\\''");
                run_cmd_ok(
                    connection,
                    &format!(
                        "echo '{}' > /etc/sssd/sssd.conf && chmod 600 /etc/sssd/sssd.conf",
                        escaped
                    ),
                    context,
                )?;
                changed = true;
                changes.push("Updated /etc/sssd/sssd.conf".to_string());
            }
        }

        // Enable and start SSSD
        if !context.check_mode {
            let (active, _, _) = run_cmd(connection, "systemctl is-active sssd", context)?;
            if !active {
                run_cmd_ok(connection, "systemctl enable --now sssd", context)?;
                changed = true;
                changes.push("Started SSSD service".to_string());
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} SSSD changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} SSSD changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok("SSSD is configured"))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["services", "domains"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m
    }
}

impl SssdConfigModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        os_family: &str,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let check_cmd = match os_family {
            "rhel" => "rpm -q sssd >/dev/null 2>&1",
            _ => "dpkg -s sssd >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            return Ok(ModuleOutput::ok("SSSD is not installed"));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed("Would remove SSSD"));
        }

        let _ = run_cmd(connection, "systemctl stop sssd", context);
        let _ = run_cmd(connection, "systemctl disable sssd", context);

        let remove_cmd = match os_family {
            "rhel" => "dnf remove -y sssd sssd-tools",
            _ => "DEBIAN_FRONTEND=noninteractive apt-get remove -y sssd sssd-tools",
        };
        run_cmd_ok(connection, remove_cmd, context)?;

        Ok(ModuleOutput::changed("Removed SSSD"))
    }
}

// ---- SSSD Domain Module ----

pub struct SssdDomainModule;

impl Module for SssdDomainModule {
    fn name(&self) -> &'static str {
        "sssd_domain"
    }

    fn description(&self) -> &'static str {
        "Manage SSSD domain configuration in sssd.conf"
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

        let domain_name = params.get_string_required("name")?;
        let provider = params.get_string_required("provider")?;
        let ldap_uri = params.get_string("ldap_uri")?;
        let krb5_realm = params.get_string("krb5_realm")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        if state == "absent" {
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would remove domain '{}'",
                    domain_name
                )));
            }
            // Remove domain section from sssd.conf
            run_cmd_ok(
                connection,
                &format!(
                    "sed -i '/^\\[domain\\/{}\\]/,/^\\[/{{/^\\[domain\\/{}/d; /^\\[/!d}}' /etc/sssd/sssd.conf",
                    domain_name, domain_name
                ),
                context,
            )?;
            return Ok(ModuleOutput::changed(format!(
                "Removed domain '{}'",
                domain_name
            )));
        }

        let mut domain_conf = format!("[domain/{}]\nid_provider = {}\n", domain_name, provider);
        if let Some(ref uri) = ldap_uri {
            domain_conf.push_str(&format!("ldap_uri = {}\n", uri));
        }
        if let Some(ref realm) = krb5_realm {
            domain_conf.push_str(&format!("krb5_realm = {}\n", realm));
        }

        let (conf_exists, current_conf, _) =
            run_cmd(connection, "cat /etc/sssd/sssd.conf 2>/dev/null", context)?;
        if !conf_exists {
            return Err(ModuleError::ExecutionFailed(
                "sssd.conf does not exist. Run sssd_config first.".to_string(),
            ));
        }

        let domain_section_present = current_conf.contains(&format!("[domain/{}]", domain_name));

        if domain_section_present {
            return Ok(
                ModuleOutput::ok(format!("Domain '{}' already configured", domain_name))
                    .with_data("domain", serde_json::json!(domain_name)),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would add domain '{}'", domain_name))
                    .with_data("domain", serde_json::json!(domain_name)),
            );
        }

        let escaped = domain_conf.replace('\'', "'\\''");
        run_cmd_ok(
            connection,
            &format!("echo '{}' >> /etc/sssd/sssd.conf", escaped),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Added domain '{}'", domain_name))
                .with_data("domain", serde_json::json!(domain_name)),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &["name", "provider"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("ldap_uri", serde_json::json!(null));
        m.insert("krb5_realm", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sssd_config_module_metadata() {
        let module = SssdConfigModule;
        assert_eq!(module.name(), "sssd_config");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_sssd_config_required_params() {
        let module = SssdConfigModule;
        let required = module.required_params();
        assert!(required.contains(&"services"));
        assert!(required.contains(&"domains"));
    }

    #[test]
    fn test_sssd_domain_module_metadata() {
        let module = SssdDomainModule;
        assert_eq!(module.name(), "sssd_domain");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_sssd_domain_required_params() {
        let module = SssdDomainModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
        assert!(required.contains(&"provider"));
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID=rhel\nVERSION=8"), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
    }
}
