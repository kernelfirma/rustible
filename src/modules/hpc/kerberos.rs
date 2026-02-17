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
//! - `secondary_kdc` (optional): Secondary KDC server(s) for HA
//! - `verify_keytab` (optional): Verify keytab after deployment (default: true)
//! - `state` (optional): "present" (default) or "absent"

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

// ---- Helper structs ----

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

// ---- Enhancement helpers ----

/// Parse a numeric time offset (in seconds) from chronyc or timedatectl output.
fn parse_time_offset(output: &str) -> f64 {
    // Try chronyc format first: "System time     :  0.000123456 seconds slow/fast"
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("System time") && trimmed.contains("seconds") {
            if let Some(after_colon) = trimmed.split(':').nth(1) {
                let parts: Vec<&str> = after_colon.split_whitespace().collect();
                if let Some(val_str) = parts.first() {
                    if let Ok(val) = val_str.parse::<f64>() {
                        return val;
                    }
                }
            }
        }
    }

    // Try timedatectl format: "System clock synchronized: yes/no"
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.contains("clock synchronized") || trimmed.contains("NTP synchronized") {
            if trimmed.contains("no") {
                return 999.0;
            } else if trimmed.contains("yes") {
                return 0.0;
            }
        }
    }

    // Unable to determine offset
    0.0
}

/// Check time synchronization status on the target host.
fn check_time_sync(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<PreflightResult> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    let (ok, stdout, _) = run_cmd(
        connection,
        "chronyc tracking 2>/dev/null || timedatectl status",
        context,
    )?;

    if !ok {
        errors.push("Unable to check time synchronization".to_string());
        return Ok(PreflightResult {
            passed: false,
            warnings,
            errors,
        });
    }

    let offset = parse_time_offset(&stdout);

    if offset > 120.0 {
        warnings.push(format!(
            "Time skew is {:.3} seconds; Kerberos requires tight time sync (< 120s)",
            offset
        ));
    }

    // Check for timedatectl "no" sync
    for line in stdout.lines() {
        let trimmed = line.trim();
        if (trimmed.contains("clock synchronized") || trimmed.contains("NTP synchronized"))
            && trimmed.contains("no")
        {
            warnings.push("System clock is not synchronized; Kerberos may fail".to_string());
        }
    }

    Ok(PreflightResult {
        passed: errors.is_empty(),
        warnings,
        errors,
    })
}

/// Generate a krb5.conf [realms] section with support for multiple KDCs.
fn configure_multi_kdc(realm: &str, kdcs: &[String], admin_server: &str) -> String {
    let mut kdc_lines = String::new();
    for kdc in kdcs {
        kdc_lines.push_str(&format!("        kdc = {}\n", kdc));
    }
    format!(
        "    {} = {{\n{}        admin_server = {}\n    }}",
        realm, kdc_lines, admin_server
    )
}

/// Verify the contents of a keytab file by parsing `klist -ket` output.
fn verify_keytab(
    connection: &Arc<dyn Connection + Send + Sync>,
    keytab_path: &str,
    context: &ModuleContext,
) -> ModuleResult<VerifyResult> {
    let mut details = Vec::new();
    let mut warnings = Vec::new();

    let (ok, stdout, stderr) = run_cmd(
        connection,
        &format!("klist -ket '{}'", keytab_path),
        context,
    )?;

    if !ok {
        return Ok(VerifyResult {
            verified: false,
            details: vec![format!("klist failed: {}", stderr.trim())],
            warnings,
        });
    }

    let (principals, enc_types) = parse_keytab_output(&stdout);

    if principals.is_empty() {
        warnings.push("No principals found in keytab".to_string());
    } else {
        details.push(format!("Principals: {}", principals.join(", ")));
    }

    if enc_types.is_empty() {
        warnings.push("No encryption types found in keytab".to_string());
    } else {
        details.push(format!("Encryption types: {}", enc_types.join(", ")));
    }

    Ok(VerifyResult {
        verified: !principals.is_empty(),
        details,
        warnings,
    })
}

/// Parse klist -ket output to extract principal names and encryption types.
fn parse_keytab_output(output: &str) -> (Vec<String>, Vec<String>) {
    let mut principals: Vec<String> = Vec::new();
    let mut enc_types: Vec<String> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        // Skip header lines and empty lines
        if trimmed.is_empty()
            || trimmed.starts_with("Keytab")
            || trimmed.starts_with("KVNO")
            || trimmed.starts_with("----")
        {
            continue;
        }

        // Typical klist -ket line:
        //   1 01/01/2024 00:00:00 aes256-cts-hmac-sha1-96 host/node.example.com@EXAMPLE.COM
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 4 {
            for part in &parts {
                if part.contains('@') && !principals.contains(&part.to_string()) {
                    principals.push(part.to_string());
                }
            }
            for part in &parts {
                if part.contains('-')
                    && (part.contains("aes")
                        || part.contains("des")
                        || part.contains("rc4")
                        || part.contains("camellia")
                        || part.contains("arcfour"))
                    && !enc_types.contains(&part.to_string())
                {
                    enc_types.push(part.to_string());
                }
            }
        }
    }

    (principals, enc_types)
}

/// Run a Kerberos health check to verify authentication is working.
fn krb5_health_check(
    connection: &Arc<dyn Connection + Send + Sync>,
    keytab_path: Option<&str>,
    context: &ModuleContext,
) -> ModuleResult<PreflightResult> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Check if there is a valid ticket cache
    let (klist_ok, _, _) = run_cmd(connection, "klist -s 2>/dev/null", context)?;

    if klist_ok {
        return Ok(PreflightResult {
            passed: true,
            warnings,
            errors,
        });
    }

    // If keytab is available, try kinit with it
    if let Some(kt) = keytab_path {
        let (_, principal_out, _) = run_cmd(
            connection,
            &format!(
                "klist -kt '{}' 2>/dev/null | tail -1 | awk '{{print $NF}}'",
                kt
            ),
            context,
        )?;
        let principal = principal_out.trim();
        if !principal.is_empty() {
            let (kinit_ok, _, kinit_err) = run_cmd(
                connection,
                &format!("kinit -k -t '{}' '{}'", kt, principal),
                context,
            )?;
            if kinit_ok {
                return Ok(PreflightResult {
                    passed: true,
                    warnings,
                    errors,
                });
            }
            errors.push(format!("kinit failed: {}", kinit_err.trim()));
        } else {
            warnings.push("Could not determine principal from keytab".to_string());
        }
    }

    if errors.is_empty() {
        warnings.push("No valid Kerberos ticket and no keytab available for kinit".to_string());
    }

    Ok(PreflightResult {
        passed: false,
        warnings,
        errors,
    })
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
        let secondary_kdc = params.get_string("secondary_kdc")?;
        let do_verify_keytab = params.get_bool_or("verify_keytab", true);

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // --- Preflight: time sync check ---
        let time_check = check_time_sync(connection, context)?;
        if !time_check.warnings.is_empty() {
            for w in &time_check.warnings {
                changes.push(format!("WARNING: {}", w));
            }
        }
        if !time_check.passed {
            for e in &time_check.errors {
                changes.push(format!("ERROR: {}", e));
            }
        }

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

        // Build list of all KDCs (primary + secondary)
        let mut all_kdcs = vec![kdc.clone()];
        if let Some(ref sec) = secondary_kdc {
            all_kdcs.push(sec.clone());
        }
        // Also support kdc as a list via get_vec_string
        if let Some(kdc_list) = params.get_vec_string("kdc")? {
            if kdc_list.len() > 1 {
                for extra in &kdc_list[1..] {
                    if !all_kdcs.contains(extra) {
                        all_kdcs.push(extra.clone());
                    }
                }
            }
        }

        // Generate krb5.conf with multi-KDC support
        let realms_section = configure_multi_kdc(&realm, &all_kdcs, &admin_server);
        let krb5_conf = format!(
            r#"[libdefaults]
    default_realm = {}
    dns_lookup_realm = false
    dns_lookup_kdc = false

[realms]
{}

[domain_realm]
    .{} = {}
    {} = {}
"#,
            realm,
            realms_section,
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

            // Verify keytab contents after deployment
            if do_verify_keytab && !context.check_mode {
                let verify = verify_keytab(connection, "/etc/krb5.keytab", context)?;
                if !verify.verified {
                    for w in &verify.warnings {
                        changes.push(format!("Keytab warning: {}", w));
                    }
                } else {
                    for d in &verify.details {
                        changes.push(format!("Keytab: {}", d));
                    }
                }
            }
        }

        // Health check: test Kerberos authentication
        if !context.check_mode {
            let keytab_path = if keytab_src.is_some() {
                Some("/etc/krb5.keytab")
            } else {
                None
            };
            let health = krb5_health_check(connection, keytab_path, context)?;
            if !health.passed {
                for e in &health.errors {
                    changes.push(format!("Health check: {}", e));
                }
                for w in &health.warnings {
                    changes.push(format!("Health check: {}", w));
                }
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
                    .with_data("realm", serde_json::json!(realm))
                    .with_data("kdcs", serde_json::json!(all_kdcs))
                    .with_data("time_sync", serde_json::json!(time_check)),
            )
        } else {
            Ok(ModuleOutput::ok("Kerberos client is configured")
                .with_data("realm", serde_json::json!(realm))
                .with_data("kdcs", serde_json::json!(all_kdcs)))
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["realm", "kdc"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("admin_server", serde_json::json!(null));
        m.insert("keytab_src", serde_json::json!(null));
        m.insert("secondary_kdc", serde_json::json!(null));
        m.insert("verify_keytab", serde_json::json!(true));
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
        assert!(optional.contains_key("secondary_kdc"));
        assert!(optional.contains_key("verify_keytab"));
        assert!(optional.contains_key("state"));
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID=rhel\nVERSION=8"), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID_LIKE=\"rhel fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=unknown"), None);
    }

    #[test]
    fn test_time_sync_parsing() {
        // Test chronyc format
        let chronyc_output = "\
Reference ID    : A9FE0101 (169.254.1.1)
Stratum         : 3
Ref time (UTC)  : Thu Jan 01 00:00:00 2024
System time     : 0.000456789 seconds slow of NTP time
Last offset     : -0.000012345 seconds
RMS offset      : 0.000123456 seconds
";
        let offset = parse_time_offset(chronyc_output);
        assert!(
            (offset - 0.000456789).abs() < 1e-9,
            "chronyc offset mismatch: {}",
            offset
        );

        // Test chronyc with large skew
        let chronyc_large_skew = "\
System time     : 150.123456789 seconds fast of NTP time
";
        let offset_large = parse_time_offset(chronyc_large_skew);
        assert!(
            offset_large > 120.0,
            "Expected large offset, got: {}",
            offset_large
        );

        // Test timedatectl format (synchronized)
        let timedatectl_synced = "\
               Local time: Thu 2024-01-01 00:00:00 UTC
           Universal time: Thu 2024-01-01 00:00:00 UTC
                 RTC time: Thu 2024-01-01 00:00:00
                Time zone: UTC (UTC, +0000)
System clock synchronized: yes
              NTP service: active
";
        let offset_synced = parse_time_offset(timedatectl_synced);
        assert!(
            (offset_synced - 0.0).abs() < 1e-9,
            "timedatectl synced offset should be 0.0"
        );

        // Test timedatectl format (not synchronized)
        let timedatectl_unsynced = "\
               Local time: Thu 2024-01-01 00:00:00 UTC
System clock synchronized: no
              NTP service: inactive
";
        let offset_unsynced = parse_time_offset(timedatectl_unsynced);
        assert!(
            offset_unsynced > 120.0,
            "Unsynced should report large offset, got: {}",
            offset_unsynced
        );

        // Test empty input
        let offset_empty = parse_time_offset("");
        assert!(
            (offset_empty - 0.0).abs() < 1e-9,
            "Empty input should return 0.0"
        );
    }

    #[test]
    fn test_multi_kdc_config_generation() {
        // Single KDC
        let single = configure_multi_kdc(
            "EXAMPLE.COM",
            &["kdc1.example.com".to_string()],
            "admin.example.com",
        );
        assert!(
            single.contains("EXAMPLE.COM = {"),
            "Should contain realm block"
        );
        assert!(
            single.contains("kdc = kdc1.example.com"),
            "Should contain primary KDC"
        );
        assert!(
            single.contains("admin_server = admin.example.com"),
            "Should contain admin server"
        );

        // Multiple KDCs
        let multi = configure_multi_kdc(
            "EXAMPLE.COM",
            &[
                "kdc1.example.com".to_string(),
                "kdc2.example.com".to_string(),
                "kdc3.example.com".to_string(),
            ],
            "admin.example.com",
        );
        assert!(
            multi.contains("kdc = kdc1.example.com"),
            "Should contain first KDC"
        );
        assert!(
            multi.contains("kdc = kdc2.example.com"),
            "Should contain second KDC"
        );
        assert!(
            multi.contains("kdc = kdc3.example.com"),
            "Should contain third KDC"
        );
        assert!(
            multi.contains("admin_server = admin.example.com"),
            "Should contain admin server"
        );

        // Verify line count for KDC entries
        let kdc_count = multi.lines().filter(|l| l.contains("kdc = ")).count();
        assert_eq!(kdc_count, 3, "Should have exactly 3 KDC lines");

        // Verify it is valid INI-style config
        assert!(
            multi.starts_with("    EXAMPLE.COM = {"),
            "Should start with indented realm"
        );
        assert!(multi.ends_with('}'), "Should end with closing brace");
    }

    #[test]
    fn test_keytab_output_parsing() {
        // Standard klist -ket output
        let klist_output = "\
Keytab name: FILE:/etc/krb5.keytab
KVNO Timestamp           Principal
---- ------------------- -------------------------------------------------------
   1 01/01/2024 00:00:00 aes256-cts-hmac-sha1-96 host/node1.example.com@EXAMPLE.COM
   1 01/01/2024 00:00:00 aes128-cts-hmac-sha1-96 host/node1.example.com@EXAMPLE.COM
   2 01/01/2024 00:00:00 aes256-cts-hmac-sha1-96 nfs/node1.example.com@EXAMPLE.COM
   2 01/01/2024 00:00:00 arcfour-hmac nfs/node1.example.com@EXAMPLE.COM
";
        let (principals, enc_types) = parse_keytab_output(klist_output);

        // Should find unique principals
        assert!(principals.contains(&"host/node1.example.com@EXAMPLE.COM".to_string()));
        assert!(principals.contains(&"nfs/node1.example.com@EXAMPLE.COM".to_string()));
        assert_eq!(principals.len(), 2, "Should have 2 unique principals");

        // Should find unique encryption types
        assert!(enc_types.contains(&"aes256-cts-hmac-sha1-96".to_string()));
        assert!(enc_types.contains(&"aes128-cts-hmac-sha1-96".to_string()));
        assert!(enc_types.contains(&"arcfour-hmac".to_string()));
        assert_eq!(enc_types.len(), 3, "Should have 3 unique encryption types");

        // Empty output
        let (empty_principals, empty_enc) = parse_keytab_output("");
        assert!(empty_principals.is_empty());
        assert!(empty_enc.is_empty());

        // Header-only output (no entries)
        let header_only = "\
Keytab name: FILE:/etc/krb5.keytab
KVNO Timestamp           Principal
---- ------------------- -------------------------------------------------------
";
        let (header_principals, header_enc) = parse_keytab_output(header_only);
        assert!(
            header_principals.is_empty(),
            "Should find no principals in header-only output"
        );
        assert!(
            header_enc.is_empty(),
            "Should find no enc types in header-only output"
        );
    }
}
