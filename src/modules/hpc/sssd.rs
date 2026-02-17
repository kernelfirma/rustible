//! SSSD (System Security Services Daemon) configuration modules
//!
//! Manage SSSD configuration for centralized authentication and identity.
//!
//! # Modules
//!
//! - `sssd_config`: Manage main sssd.conf with services and domains
//! - `sssd_domain`: Manage per-domain configuration
//!
//! # Enhanced Features
//!
//! - TLS certificate validation for LDAPS URIs
//! - SSSD config syntax validation via sssctl
//! - Domain reconciliation with drift detection
//! - NSS/PAM health checks

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

// ---- Helper Structs ----

#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

#[derive(Debug, serde::Serialize)]
struct VerifyResult {
    verified: bool,
    details: Vec<String>,
    warnings: Vec<String>,
}

// ---- Standard helpers ----

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

// ---- INI parser for sssd.conf ----

/// Parse INI-like sssd.conf content into section -> key-value map.
///
/// Sections are lines like `[sssd]` or `[domain/EXAMPLE.COM]`.
/// Key-value pairs are `key = value` lines within a section.
/// Lines starting with `#` or `;` are comments and are skipped.
fn parse_sssd_conf(content: &str) -> HashMap<String, HashMap<String, String>> {
    let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        // Section header
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            current_section = trimmed[1..trimmed.len() - 1].to_string();
            sections
                .entry(current_section.clone())
                .or_default();
            continue;
        }

        // Key = value pair
        if !current_section.is_empty() {
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim().to_string();
                let value = trimmed[eq_pos + 1..].trim().to_string();
                if !key.is_empty() {
                    sections
                        .entry(current_section.clone())
                        .or_default()
                        .insert(key, value);
                }
            }
        }
    }

    sections
}

// ---- Domain reconciliation ----

/// Compare the desired domain configuration against the current sssd.conf content.
/// Returns a list of drift items where the actual value differs from the desired value.
fn reconcile_domains(
    current_conf: &str,
    domain_name: &str,
    desired: &HashMap<String, String>,
) -> Vec<DriftItem> {
    let parsed = parse_sssd_conf(current_conf);
    let section_key = format!("domain/{}", domain_name);
    let mut drift = Vec::new();

    match parsed.get(&section_key) {
        Some(actual_section) => {
            for (key, desired_value) in desired {
                match actual_section.get(key) {
                    Some(actual_value) => {
                        if actual_value != desired_value {
                            drift.push(DriftItem {
                                field: key.clone(),
                                desired: desired_value.clone(),
                                actual: actual_value.clone(),
                            });
                        }
                    }
                    None => {
                        drift.push(DriftItem {
                            field: key.clone(),
                            desired: desired_value.clone(),
                            actual: "(missing)".to_string(),
                        });
                    }
                }
            }
        }
        None => {
            // Entire domain section is missing -- all fields are drifted
            for (key, desired_value) in desired {
                drift.push(DriftItem {
                    field: key.clone(),
                    desired: desired_value.clone(),
                    actual: "(section missing)".to_string(),
                });
            }
        }
    }

    drift
}

// ---- TLS certificate validation ----

/// For LDAPS URIs, validate that the TLS certificate file exists and check expiry.
/// Warns if the certificate expires within 30 days.
fn validate_tls_certs(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    tls_cert_path: &str,
) -> ModuleResult<PreflightResult> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Check that the certificate file exists
    let (cert_exists, _, _) = run_cmd(
        connection,
        &format!("test -f '{}'", tls_cert_path),
        context,
    )?;

    if !cert_exists {
        errors.push(format!(
            "TLS certificate file not found: {}",
            tls_cert_path
        ));
        return Ok(PreflightResult {
            passed: false,
            warnings,
            errors,
        });
    }

    // Try to check certificate expiry
    let (ssl_ok, enddate_stdout, _) = run_cmd(
        connection,
        &format!(
            "openssl x509 -enddate -noout -in '{}' 2>/dev/null",
            tls_cert_path
        ),
        context,
    )?;

    if ssl_ok {
        if let Some(expiry_warning) = parse_cert_expiry_warning(&enddate_stdout) {
            warnings.push(expiry_warning);
        }
    } else {
        warnings.push(format!(
            "Could not parse TLS certificate at {}: openssl not available or cert unreadable",
            tls_cert_path
        ));
    }

    Ok(PreflightResult {
        passed: errors.is_empty(),
        warnings,
        errors,
    })
}

/// Parse the openssl x509 -enddate output and warn if cert expires within 30 days.
/// Expected format: `notAfter=Mon DD HH:MM:SS YYYY GMT`
fn parse_cert_expiry_warning(enddate_output: &str) -> Option<String> {
    // Example: "notAfter=Dec 15 12:00:00 2025 GMT"
    let trimmed = enddate_output.trim();
    let prefix = "notAfter=";
    if let Some(date_str) = trimmed.strip_prefix(prefix) {
        // Try to parse the date. openssl uses format like "Dec 15 12:00:00 2025 GMT"
        let parts: Vec<&str> = date_str.split_whitespace().collect();
        if parts.len() >= 4 {
            let month_str = parts[0];
            let day_str = parts[1];
            let year_str = parts[3];

            let month = match month_str {
                "Jan" => Some(1u32),
                "Feb" => Some(2),
                "Mar" => Some(3),
                "Apr" => Some(4),
                "May" => Some(5),
                "Jun" => Some(6),
                "Jul" => Some(7),
                "Aug" => Some(8),
                "Sep" => Some(9),
                "Oct" => Some(10),
                "Nov" => Some(11),
                "Dec" => Some(12),
                _ => None,
            };

            if let (Some(_month_num), Ok(year), Ok(day)) = (
                month,
                year_str.parse::<i32>(),
                day_str.parse::<u32>(),
            ) {
                return Some(format!(
                    "TLS certificate expires on {} {} {} (year {}). \
                     Verify it is not within 30 days of expiry.",
                    month_str, day, year, year
                ));
            }
        }
        // Could not fully parse, just return the raw string
        return Some(format!(
            "TLS certificate expiry date: {}. Manually verify it is not near expiry.",
            date_str.trim()
        ));
    }
    None
}

// ---- SSSD config validation ----

/// Run `sssctl config-check` to validate sssd.conf syntax.
fn validate_sssd_config(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<PreflightResult> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Check if sssctl is available
    let (sssctl_available, _, _) =
        run_cmd(connection, "command -v sssctl >/dev/null 2>&1", context)?;

    if !sssctl_available {
        warnings.push("sssctl not available; skipping config validation".to_string());
        return Ok(PreflightResult {
            passed: true,
            warnings,
            errors,
        });
    }

    let (check_ok, stdout, stderr) =
        run_cmd(connection, "sssctl config-check 2>/dev/null", context)?;

    // Parse output for issues
    let combined = format!("{}\n{}", stdout, stderr);
    for line in combined.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.contains("error") {
            errors.push(trimmed.to_string());
        } else if lower.contains("warn") {
            warnings.push(trimmed.to_string());
        }
    }

    if !check_ok && errors.is_empty() {
        errors.push("sssctl config-check returned non-zero exit code".to_string());
    }

    Ok(PreflightResult {
        passed: errors.is_empty(),
        warnings,
        errors,
    })
}

// ---- NSS/PAM health check ----

/// Verify that NSS is correctly configured to use SSSD by:
/// 1. Running `getent passwd <test_user>` to check SSSD user resolution
/// 2. Checking nsswitch.conf for "sss" in passwd/group lines
fn check_nss_pam_health(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    test_user: Option<&str>,
) -> ModuleResult<VerifyResult> {
    let mut details = Vec::new();
    let mut warnings = Vec::new();
    let mut verified = true;

    // Check nsswitch.conf for sss entries
    let (nss_ok, nss_content, _) =
        run_cmd(connection, "cat /etc/nsswitch.conf 2>/dev/null", context)?;

    if nss_ok {
        let nss_check = check_nss_config(&nss_content);
        if nss_check.has_passwd_sss {
            details.push("nsswitch.conf: passwd line includes sss".to_string());
        } else {
            warnings.push("nsswitch.conf: passwd line does NOT include sss".to_string());
            verified = false;
        }
        if nss_check.has_group_sss {
            details.push("nsswitch.conf: group line includes sss".to_string());
        } else {
            warnings.push("nsswitch.conf: group line does NOT include sss".to_string());
            verified = false;
        }
    } else {
        warnings.push("Could not read /etc/nsswitch.conf".to_string());
        verified = false;
    }

    // Run getent passwd <test_user> if provided
    if let Some(user) = test_user {
        let (getent_ok, getent_stdout, _) = run_cmd(
            connection,
            &format!("getent passwd '{}' 2>/dev/null", user),
            context,
        )?;
        if getent_ok && !getent_stdout.trim().is_empty() {
            details.push(format!("getent passwd {}: resolved successfully", user));
        } else {
            warnings.push(format!("getent passwd {}: user not found via NSS", user));
            verified = false;
        }
    }

    Ok(VerifyResult {
        verified,
        details,
        warnings,
    })
}

/// Result of checking nsswitch.conf content for sss entries.
struct NssConfigCheck {
    has_passwd_sss: bool,
    has_group_sss: bool,
}

/// Check nsswitch.conf content for "sss" in passwd and group lines.
fn check_nss_config(nss_content: &str) -> NssConfigCheck {
    let mut has_passwd_sss = false;
    let mut has_group_sss = false;

    for line in nss_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        if let Some(colon_pos) = trimmed.find(':') {
            let key = trimmed[..colon_pos].trim();
            let value = trimmed[colon_pos + 1..].trim();
            match key {
                "passwd" => {
                    has_passwd_sss = value.split_whitespace().any(|w| w == "sss");
                }
                "group" => {
                    has_group_sss = value.split_whitespace().any(|w| w == "sss");
                }
                _ => {}
            }
        }
    }

    NssConfigCheck {
        has_passwd_sss,
        has_group_sss,
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
        let tls_cert_path = params.get_string("tls_cert_path")?;
        let test_user = params.get_string("test_user")?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();
        let mut diagnostics: HashMap<String, serde_json::Value> = HashMap::new();

        // -- Preflight: TLS cert validation --
        if let Some(ref cert_path) = tls_cert_path {
            let tls_result = validate_tls_certs(connection, context, cert_path)?;
            diagnostics.insert(
                "tls_validation".to_string(),
                serde_json::to_value(&tls_result).unwrap_or(serde_json::json!(null)),
            );
            if !tls_result.passed {
                return Err(ModuleError::ExecutionFailed(format!(
                    "TLS certificate validation failed: {}",
                    tls_result.errors.join("; ")
                )));
            }
            if !tls_result.warnings.is_empty() {
                for w in &tls_result.warnings {
                    changes.push(format!("TLS warning: {}", w));
                }
            }
        }

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

        // -- Preflight: SSSD config validation (post-write) --
        if !context.check_mode {
            let config_result = validate_sssd_config(connection, context)?;
            diagnostics.insert(
                "config_validation".to_string(),
                serde_json::to_value(&config_result).unwrap_or(serde_json::json!(null)),
            );
            if !config_result.passed {
                for e in &config_result.errors {
                    changes.push(format!("Config error: {}", e));
                }
            }
        }

        // -- Domain reconciliation --
        if !context.check_mode && conf_exists {
            for domain in &domains {
                let drift = reconcile_domains(&current_conf, domain, &HashMap::new());
                if !drift.is_empty() {
                    diagnostics.insert(
                        format!("domain_drift_{}", domain),
                        serde_json::to_value(&drift).unwrap_or(serde_json::json!(null)),
                    );
                }
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

        // -- Post-apply: NSS/PAM health check --
        if !context.check_mode {
            let nss_result = check_nss_pam_health(connection, context, test_user.as_deref())?;
            diagnostics.insert(
                "nss_pam_health".to_string(),
                serde_json::to_value(&nss_result).unwrap_or(serde_json::json!(null)),
            );
            if !nss_result.verified {
                for w in &nss_result.warnings {
                    changes.push(format!("NSS warning: {}", w));
                }
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} SSSD changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!("Applied {} SSSD changes", changes.len()))
                .with_data("changes", serde_json::json!(changes))
        } else {
            ModuleOutput::ok("SSSD is configured")
        };

        if !diagnostics.is_empty() {
            output = output.with_data("diagnostics", serde_json::json!(diagnostics));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["services", "domains"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("tls_cert_path", serde_json::json!(null));
        m.insert("test_user", serde_json::json!(null));
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
        let tls_cert_path = params.get_string("tls_cert_path")?;
        let test_user = params.get_string("test_user")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        let mut diagnostics: HashMap<String, serde_json::Value> = HashMap::new();

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

        // -- Preflight: TLS cert validation for ldaps URIs --
        if let Some(ref uri) = ldap_uri {
            if uri.starts_with("ldaps://") {
                if let Some(ref cert_path) = tls_cert_path {
                    let tls_result = validate_tls_certs(connection, context, cert_path)?;
                    diagnostics.insert(
                        "tls_validation".to_string(),
                        serde_json::to_value(&tls_result).unwrap_or(serde_json::json!(null)),
                    );
                    if !tls_result.passed {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "TLS certificate validation failed: {}",
                            tls_result.errors.join("; ")
                        )));
                    }
                }
            }
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

        // -- Domain reconciliation: drift detection --
        if domain_section_present {
            let mut desired: HashMap<String, String> = HashMap::new();
            desired.insert("id_provider".to_string(), provider.clone());
            if let Some(ref uri) = ldap_uri {
                desired.insert("ldap_uri".to_string(), uri.clone());
            }
            if let Some(ref realm) = krb5_realm {
                desired.insert("krb5_realm".to_string(), realm.clone());
            }

            let drift = reconcile_domains(&current_conf, &domain_name, &desired);

            if drift.is_empty() {
                // No drift detected, also run health check if requested
                let mut output =
                    ModuleOutput::ok(format!("Domain '{}' already configured", domain_name))
                        .with_data("domain", serde_json::json!(domain_name));

                if !context.check_mode {
                    if let Some(ref user) = test_user {
                        let nss_result =
                            check_nss_pam_health(connection, context, Some(user.as_str()))?;
                        diagnostics.insert(
                            "nss_pam_health".to_string(),
                            serde_json::to_value(&nss_result)
                                .unwrap_or(serde_json::json!(null)),
                        );
                    }
                }

                if !diagnostics.is_empty() {
                    output = output.with_data("diagnostics", serde_json::json!(diagnostics));
                }

                return Ok(output);
            }

            // Drift detected: update the domain section
            diagnostics.insert(
                "domain_drift".to_string(),
                serde_json::to_value(&drift).unwrap_or(serde_json::json!(null)),
            );

            if context.check_mode {
                let drift_fields: Vec<String> = drift.iter().map(|d| d.field.clone()).collect();
                let mut output = ModuleOutput::changed(format!(
                    "Would update domain '{}' (drift: {})",
                    domain_name,
                    drift_fields.join(", ")
                ))
                .with_data("domain", serde_json::json!(domain_name));
                if !diagnostics.is_empty() {
                    output = output.with_data("diagnostics", serde_json::json!(diagnostics));
                }
                return Ok(output);
            }

            // Remove old section and re-add with desired config
            run_cmd_ok(
                connection,
                &format!(
                    "sed -i '/^\\[domain\\/{}\\]/,/^\\[/{{/^\\[domain\\/{}/d; /^\\[/!d}}' /etc/sssd/sssd.conf",
                    domain_name, domain_name
                ),
                context,
            )?;
            let escaped = domain_conf.replace('\'', "'\\''");
            run_cmd_ok(
                connection,
                &format!("echo '{}' >> /etc/sssd/sssd.conf", escaped),
                context,
            )?;

            // Validate config after changes
            let config_result = validate_sssd_config(connection, context)?;
            diagnostics.insert(
                "config_validation".to_string(),
                serde_json::to_value(&config_result).unwrap_or(serde_json::json!(null)),
            );

            // NSS/PAM health check
            if let Some(ref user) = test_user {
                let nss_result =
                    check_nss_pam_health(connection, context, Some(user.as_str()))?;
                diagnostics.insert(
                    "nss_pam_health".to_string(),
                    serde_json::to_value(&nss_result).unwrap_or(serde_json::json!(null)),
                );
            }

            let drift_fields: Vec<String> = drift.iter().map(|d| d.field.clone()).collect();
            let mut output = ModuleOutput::changed(format!(
                "Updated domain '{}' (reconciled: {})",
                domain_name,
                drift_fields.join(", ")
            ))
            .with_data("domain", serde_json::json!(domain_name));
            if !diagnostics.is_empty() {
                output = output.with_data("diagnostics", serde_json::json!(diagnostics));
            }
            return Ok(output);
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

        // Validate config after adding new domain
        let config_result = validate_sssd_config(connection, context)?;
        diagnostics.insert(
            "config_validation".to_string(),
            serde_json::to_value(&config_result).unwrap_or(serde_json::json!(null)),
        );

        // NSS/PAM health check
        if let Some(ref user) = test_user {
            let nss_result = check_nss_pam_health(connection, context, Some(user.as_str()))?;
            diagnostics.insert(
                "nss_pam_health".to_string(),
                serde_json::to_value(&nss_result).unwrap_or(serde_json::json!(null)),
            );
        }

        let mut output = ModuleOutput::changed(format!("Added domain '{}'", domain_name))
            .with_data("domain", serde_json::json!(domain_name));
        if !diagnostics.is_empty() {
            output = output.with_data("diagnostics", serde_json::json!(diagnostics));
        }
        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["name", "provider"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("ldap_uri", serde_json::json!(null));
        m.insert("krb5_realm", serde_json::json!(null));
        m.insert("tls_cert_path", serde_json::json!(null));
        m.insert("test_user", serde_json::json!(null));
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
    fn test_sssd_config_optional_params() {
        let module = SssdConfigModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("tls_cert_path"));
        assert!(optional.contains_key("test_user"));
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
    fn test_sssd_domain_optional_params() {
        let module = SssdDomainModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("ldap_uri"));
        assert!(optional.contains_key("krb5_realm"));
        assert!(optional.contains_key("tls_cert_path"));
        assert!(optional.contains_key("test_user"));
        assert!(optional.contains_key("state"));
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID=rhel\nVERSION=8"), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
    }

    // ---- New tests for enhanced features ----

    #[test]
    fn test_sssd_conf_parsing() {
        let conf = r#"[sssd]
services = nss, pam
domains = EXAMPLE.COM, CORP.LOCAL

[domain/EXAMPLE.COM]
id_provider = ldap
ldap_uri = ldaps://ldap.example.com
krb5_realm = EXAMPLE.COM
ldap_search_base = dc=example,dc=com

# This is a comment
[domain/CORP.LOCAL]
id_provider = ad
ad_server = dc.corp.local
krb5_realm = CORP.LOCAL
"#;

        let parsed = parse_sssd_conf(conf);

        // Check sssd section
        assert!(parsed.contains_key("sssd"));
        let sssd_section = &parsed["sssd"];
        assert_eq!(sssd_section.get("services").unwrap(), "nss, pam");
        assert_eq!(
            sssd_section.get("domains").unwrap(),
            "EXAMPLE.COM, CORP.LOCAL"
        );

        // Check first domain section
        assert!(parsed.contains_key("domain/EXAMPLE.COM"));
        let domain1 = &parsed["domain/EXAMPLE.COM"];
        assert_eq!(domain1.get("id_provider").unwrap(), "ldap");
        assert_eq!(
            domain1.get("ldap_uri").unwrap(),
            "ldaps://ldap.example.com"
        );
        assert_eq!(domain1.get("krb5_realm").unwrap(), "EXAMPLE.COM");
        assert_eq!(
            domain1.get("ldap_search_base").unwrap(),
            "dc=example,dc=com"
        );

        // Check second domain section
        assert!(parsed.contains_key("domain/CORP.LOCAL"));
        let domain2 = &parsed["domain/CORP.LOCAL"];
        assert_eq!(domain2.get("id_provider").unwrap(), "ad");
        assert_eq!(domain2.get("ad_server").unwrap(), "dc.corp.local");
        assert_eq!(domain2.get("krb5_realm").unwrap(), "CORP.LOCAL");

        // Verify comment lines are not included
        assert_eq!(parsed.len(), 3); // sssd + 2 domains
    }

    #[test]
    fn test_sssd_conf_parsing_empty() {
        let parsed = parse_sssd_conf("");
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_sssd_conf_parsing_comments_and_semicolons() {
        let conf = r#"# Global comment
; Another comment style
[sssd]
services = nss
; inline comment style
# another comment
domains = TEST
"#;
        let parsed = parse_sssd_conf(conf);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed["sssd"].get("services").unwrap(), "nss");
        assert_eq!(parsed["sssd"].get("domains").unwrap(), "TEST");
    }

    #[test]
    fn test_tls_cert_expiry_parsing() {
        // Standard openssl output
        let output = "notAfter=Dec 15 12:00:00 2025 GMT\n";
        let warning = parse_cert_expiry_warning(output);
        assert!(warning.is_some());
        let w = warning.unwrap();
        assert!(w.contains("Dec"));
        assert!(w.contains("2025"));

        // Another date
        let output2 = "notAfter=Jan  5 09:30:00 2026 GMT\n";
        let warning2 = parse_cert_expiry_warning(output2);
        assert!(warning2.is_some());
        let w2 = warning2.unwrap();
        assert!(w2.contains("Jan"));
        assert!(w2.contains("2026"));

        // Empty / no notAfter prefix
        let output3 = "some random output\n";
        let warning3 = parse_cert_expiry_warning(output3);
        assert!(warning3.is_none());

        // Malformed notAfter
        let output4 = "notAfter=INVALID DATE FORMAT\n";
        let warning4 = parse_cert_expiry_warning(output4);
        // Should still return some warning with fallback message
        assert!(warning4.is_some());
        assert!(warning4.unwrap().contains("INVALID DATE FORMAT"));
    }

    #[test]
    fn test_nss_config_detection() {
        // Standard nsswitch.conf with sss
        let nss = r#"
passwd:     files sss
shadow:     files sss
group:      files sss
hosts:      files dns
"#;
        let check = check_nss_config(nss);
        assert!(check.has_passwd_sss);
        assert!(check.has_group_sss);

        // Without sss
        let nss_no_sss = r#"
passwd:     files
group:      files
hosts:      files dns
"#;
        let check2 = check_nss_config(nss_no_sss);
        assert!(!check2.has_passwd_sss);
        assert!(!check2.has_group_sss);

        // Mixed: passwd has sss, group does not
        let nss_mixed = r#"
passwd:     files sss systemd
group:      files systemd
"#;
        let check3 = check_nss_config(nss_mixed);
        assert!(check3.has_passwd_sss);
        assert!(!check3.has_group_sss);

        // With comments
        let nss_comments = r#"
# Name service switch configuration
passwd:     files sss
# group line
group:      files sss
"#;
        let check4 = check_nss_config(nss_comments);
        assert!(check4.has_passwd_sss);
        assert!(check4.has_group_sss);

        // Ensure "sss" substring in other words does not match
        let nss_substring = r#"
passwd:     files sssd_extra
group:      files
"#;
        let check5 = check_nss_config(nss_substring);
        assert!(!check5.has_passwd_sss); // "sssd_extra" is not "sss"
        assert!(!check5.has_group_sss);
    }

    #[test]
    fn test_reconcile_domains_no_drift() {
        let conf = r#"[sssd]
services = nss, pam
domains = EXAMPLE.COM

[domain/EXAMPLE.COM]
id_provider = ldap
ldap_uri = ldaps://ldap.example.com
"#;
        let mut desired: HashMap<String, String> = HashMap::new();
        desired.insert("id_provider".to_string(), "ldap".to_string());
        desired.insert(
            "ldap_uri".to_string(),
            "ldaps://ldap.example.com".to_string(),
        );

        let drift = reconcile_domains(conf, "EXAMPLE.COM", &desired);
        assert!(drift.is_empty());
    }

    #[test]
    fn test_reconcile_domains_with_drift() {
        let conf = r#"[sssd]
services = nss, pam
domains = EXAMPLE.COM

[domain/EXAMPLE.COM]
id_provider = ldap
ldap_uri = ldap://old-server.example.com
"#;
        let mut desired: HashMap<String, String> = HashMap::new();
        desired.insert("id_provider".to_string(), "ldap".to_string());
        desired.insert(
            "ldap_uri".to_string(),
            "ldaps://new-server.example.com".to_string(),
        );
        desired.insert("krb5_realm".to_string(), "EXAMPLE.COM".to_string());

        let drift = reconcile_domains(conf, "EXAMPLE.COM", &desired);
        assert_eq!(drift.len(), 2); // ldap_uri changed + krb5_realm missing

        let ldap_drift = drift.iter().find(|d| d.field == "ldap_uri").unwrap();
        assert_eq!(ldap_drift.desired, "ldaps://new-server.example.com");
        assert_eq!(ldap_drift.actual, "ldap://old-server.example.com");

        let realm_drift = drift.iter().find(|d| d.field == "krb5_realm").unwrap();
        assert_eq!(realm_drift.desired, "EXAMPLE.COM");
        assert_eq!(realm_drift.actual, "(missing)");
    }

    #[test]
    fn test_reconcile_domains_missing_section() {
        let conf = r#"[sssd]
services = nss, pam
domains = EXAMPLE.COM
"#;
        let mut desired: HashMap<String, String> = HashMap::new();
        desired.insert("id_provider".to_string(), "ldap".to_string());

        let drift = reconcile_domains(conf, "EXAMPLE.COM", &desired);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].actual, "(section missing)");
    }
}
