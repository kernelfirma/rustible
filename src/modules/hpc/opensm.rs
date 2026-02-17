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
//! - `validate_config` (optional, default true): Run config validation before deploy

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use regex::Regex;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of preflight configuration validation.
#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single field that drifted from desired to actual.
#[derive(Debug, serde::Serialize)]
struct DriftItem {
    field: String,
    desired: String,
    actual: String,
}

/// Post-change verification result.
#[derive(Debug, serde::Serialize)]
struct VerifyResult {
    verified: bool,
    details: Vec<String>,
    warnings: Vec<String>,
}

/// Known routing engines supported by OpenSM.
const VALID_ROUTING_ENGINES: &[&str] = &[
    "minhop",
    "updn",
    "ftree",
    "lash",
    "dor",
    "torus-2QoS",
    "dfsssp",
    "sssp",
];

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

/// Validate OpenSM configuration parameters before applying.
///
/// Checks:
/// - `subnet_prefix`: must match hex format `0x` followed by exactly 16 hex digits
/// - `routing_engine`: must be one of the known engines
/// - `log_level`: must be a number in the range 0-255
fn validate_opensm_config(
    subnet_prefix: &Option<String>,
    routing_engine: &Option<String>,
    log_level: &Option<String>,
) -> PreflightResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if let Some(ref prefix) = subnet_prefix {
        let re = Regex::new(r"^0x[0-9a-fA-F]{16}$").unwrap();
        if !re.is_match(prefix) {
            errors.push(format!(
                "Invalid subnet_prefix '{}': must match 0x followed by 16 hex digits \
                 (e.g., 0xfe80000000000000)",
                prefix
            ));
        }
    }

    if let Some(ref engine) = routing_engine {
        if !VALID_ROUTING_ENGINES.contains(&engine.as_str()) {
            errors.push(format!(
                "Invalid routing_engine '{}': must be one of {}",
                engine,
                VALID_ROUTING_ENGINES.join(", ")
            ));
        }
    }

    if let Some(ref level) = log_level {
        match level.parse::<u32>() {
            Ok(v) if v <= 255 => {}
            Ok(v) => {
                errors.push(format!("Invalid log_level '{}': must be in range 0-255", v));
            }
            Err(_) => {
                errors.push(format!(
                    "Invalid log_level '{}': must be a numeric value in range 0-255",
                    level
                ));
            }
        }
    }

    if errors.is_empty()
        && subnet_prefix.is_none()
        && routing_engine.is_none()
        && log_level.is_none()
    {
        warnings.push("No configuration parameters specified".to_string());
    }

    PreflightResult {
        passed: errors.is_empty(),
        warnings,
        errors,
    }
}

/// Write config to a staging file, validate it, then move to target path.
fn staged_config_deploy(
    connection: &Arc<dyn Connection + Send + Sync>,
    config_content: &str,
    conf_path: &str,
    context: &ModuleContext,
) -> ModuleResult<VerifyResult> {
    let staging_path = "/tmp/opensm.conf.staging";
    let mut details = Vec::new();
    let warnings = Vec::new();

    // Write config to staging file
    let escaped = config_content.replace('\'', "'\\''");
    run_cmd_ok(
        connection,
        &format!("echo '{}' > {}", escaped, staging_path),
        context,
    )?;
    details.push(format!("Wrote staging config to {}", staging_path));

    // Validate with opensm --validate (best-effort, command may not exist)
    let (valid, _, stderr) = run_cmd(
        connection,
        &format!("opensm -c {} -f /dev/null --validate", staging_path),
        context,
    )?;

    if !valid {
        // Clean up staging file on failure
        let _ = run_cmd(connection, &format!("rm -f {}", staging_path), context);
        return Ok(VerifyResult {
            verified: false,
            details: vec![format!("Staging validation failed: {}", stderr.trim())],
            warnings,
        });
    }
    details.push("Staging config passed opensm validation".to_string());

    // Move staging file to target
    run_cmd_ok(
        connection,
        &format!("mv {} {}", staging_path, conf_path),
        context,
    )?;
    details.push(format!("Deployed config to {}", conf_path));

    Ok(VerifyResult {
        verified: true,
        details,
        warnings,
    })
}

/// Check SM failover status before restarting OpenSM.
///
/// Parses `sminfo` output to determine if this SM is master or standby.
/// If master and no standby is detected, adds a warning about potential
/// fabric disruption.
fn failover_safe_restart(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<PreflightResult> {
    let mut warnings = Vec::new();
    let errors = Vec::new();

    let (success, sminfo_out, _) = run_cmd(connection, "sminfo", context)?;
    if !success {
        warnings.push("Could not query SM info via sminfo; proceeding with restart".to_string());
        return Ok(PreflightResult {
            passed: true,
            warnings,
            errors,
        });
    }

    let role = parse_sm_role(&sminfo_out);

    if role == "master" {
        // Check for standby SMs by looking at saquery output.
        // If there is only one SM and it is master, there is no standby to take over.
        let (_, saquery_out, _) = run_cmd(
            connection,
            "saquery -s --sminfo 2>/dev/null || true",
            context,
        )?;
        let sm_count = saquery_out.lines().filter(|l| l.contains("SMInfo")).count();
        if sm_count <= 1 {
            warnings.push(
                "This SM is master and no standby SM detected; \
                 restarting may disrupt fabric"
                    .to_string(),
            );
        }
    }

    Ok(PreflightResult {
        passed: true,
        warnings,
        errors,
    })
}

/// Parse the role of the local SM from `sminfo` output.
///
/// Typical output: `sminfo: sm lid 1 ... state 3 ...`
/// State values: 1=not-active, 2=discovering, 3=standby, 4=master
/// Also handles textual output like `... SMState:Master ...`
fn parse_sm_role(sminfo_output: &str) -> &'static str {
    let lower = sminfo_output.to_lowercase();
    // Check for textual role
    if lower.contains("master") {
        return "master";
    }
    if lower.contains("standby") {
        return "standby";
    }
    // Check for numeric state
    if let Some(caps) = Regex::new(r"state\s+(\d+)").unwrap().captures(&lower) {
        if let Some(m) = caps.get(1) {
            match m.as_str() {
                "4" => return "master",
                "3" => return "standby",
                _ => {}
            }
        }
    }
    "unknown"
}

/// Collect topology diagnostics from `sminfo` and `ibstat`.
fn collect_topology_diagnostics(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<HashMap<String, String>> {
    let mut result = HashMap::new();

    let (_, sm_info, _) = run_cmd(
        connection,
        "sminfo 2>/dev/null || echo 'sminfo unavailable'",
        context,
    )?;
    result.insert("sm_info".to_string(), sm_info.trim().to_string());

    let (_, ib_status, _) = run_cmd(
        connection,
        "ibstat 2>/dev/null || echo 'ibstat unavailable'",
        context,
    )?;
    result.insert("ib_status".to_string(), ib_status.trim().to_string());

    Ok(result)
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

        let validate_config = params.get_bool_or("validate_config", true);

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

        // Config validation preflight
        if validate_config {
            let preflight = validate_opensm_config(&subnet_prefix, &routing_engine, &log_level);
            if !preflight.passed {
                return Err(ModuleError::ValidationFailed(format!(
                    "Config validation failed: {}",
                    preflight.errors.join("; ")
                )));
            }
        }

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();
        let mut output_data: HashMap<String, serde_json::Value> = HashMap::new();

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
                // Use staged deploy for config changes
                let config_content = config_lines.join("\n") + "\n";
                let deploy_result =
                    staged_config_deploy(connection, &config_content, conf_path, context)?;
                if !deploy_result.verified {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Staged config deploy failed: {}",
                        deploy_result.details.join("; ")
                    )));
                }
                changed = true;
                changes.push(format!("Updated {} via staged deploy", conf_path));
                output_data.insert(
                    "deploy_result".to_string(),
                    serde_json::json!(deploy_result),
                );
            }
        }

        // Failover check before restart and enable/start OpenSM service
        if !context.check_mode {
            let (active, _, _) = run_cmd(connection, "systemctl is-active opensm", context)?;
            if !active || changed {
                // Run failover safety check before restarting
                let failover = failover_safe_restart(connection, context)?;
                if !failover.warnings.is_empty() {
                    output_data.insert(
                        "failover_warnings".to_string(),
                        serde_json::json!(failover.warnings),
                    );
                }

                if !active {
                    run_cmd_ok(connection, "systemctl enable --now opensm", context)?;
                    changed = true;
                    changes.push("Started OpenSM service".to_string());
                } else if changed {
                    run_cmd_ok(connection, "systemctl restart opensm", context)?;
                    changes.push("Restarted OpenSM service after config change".to_string());
                }
            }
        }

        // Collect topology diagnostics
        if !context.check_mode {
            let diag = collect_topology_diagnostics(connection, context)?;
            output_data.insert("topology_diagnostics".to_string(), serde_json::json!(diag));
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} OpenSM changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!("Applied {} OpenSM changes", changes.len()))
                .with_data("changes", serde_json::json!(changes))
        } else {
            ModuleOutput::ok("OpenSM is configured")
        };

        for (key, value) in output_data {
            output = output.with_data(&key, value);
        }

        Ok(output)
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
        m.insert("validate_config", serde_json::json!(true));
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
        assert!(optional.contains_key("validate_config"));
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(detect_os_family("ID=rhel\nVERSION=8"), Some("rhel"));
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID_LIKE=\"rhel fedora\""), Some("rhel"));
        assert_eq!(detect_os_family("ID=unknown"), None);
    }

    #[test]
    fn test_subnet_prefix_validation() {
        // Valid prefixes
        let result = validate_opensm_config(&Some("0xfe80000000000000".to_string()), &None, &None);
        assert!(result.passed);
        assert!(result.errors.is_empty());

        let result = validate_opensm_config(&Some("0x0000000000000000".to_string()), &None, &None);
        assert!(result.passed);

        let result = validate_opensm_config(&Some("0xFFFFFFFFFFFFFFFF".to_string()), &None, &None);
        assert!(result.passed);

        // Invalid: too short
        let result = validate_opensm_config(&Some("0xfe80".to_string()), &None, &None);
        assert!(!result.passed);
        assert!(result.errors[0].contains("Invalid subnet_prefix"));

        // Invalid: missing 0x prefix
        let result = validate_opensm_config(&Some("fe80000000000000".to_string()), &None, &None);
        assert!(!result.passed);

        // Invalid: too long
        let result =
            validate_opensm_config(&Some("0xfe8000000000000000".to_string()), &None, &None);
        assert!(!result.passed);

        // Invalid: non-hex characters
        let result = validate_opensm_config(&Some("0xfe800000000000zz".to_string()), &None, &None);
        assert!(!result.passed);

        // Invalid: empty
        let result = validate_opensm_config(&Some("".to_string()), &None, &None);
        assert!(!result.passed);
    }

    #[test]
    fn test_routing_engine_validation() {
        // Valid engines
        for engine in &[
            "minhop",
            "updn",
            "ftree",
            "lash",
            "dor",
            "torus-2QoS",
            "dfsssp",
            "sssp",
        ] {
            let result = validate_opensm_config(&None, &Some(engine.to_string()), &None);
            assert!(
                result.passed,
                "Engine '{}' should be valid but got errors: {:?}",
                engine, result.errors
            );
        }

        // Invalid engine
        let result = validate_opensm_config(&None, &Some("invalid_engine".to_string()), &None);
        assert!(!result.passed);
        assert!(result.errors[0].contains("Invalid routing_engine"));

        // Invalid: empty string
        let result = validate_opensm_config(&None, &Some("".to_string()), &None);
        assert!(!result.passed);

        // Invalid: close but wrong case
        let result = validate_opensm_config(&None, &Some("Ftree".to_string()), &None);
        assert!(!result.passed);
    }

    #[test]
    fn test_log_level_validation() {
        // Valid levels
        let result = validate_opensm_config(&None, &None, &Some("0".to_string()));
        assert!(result.passed);

        let result = validate_opensm_config(&None, &None, &Some("128".to_string()));
        assert!(result.passed);

        let result = validate_opensm_config(&None, &None, &Some("255".to_string()));
        assert!(result.passed);

        // Invalid: out of range
        let result = validate_opensm_config(&None, &None, &Some("256".to_string()));
        assert!(!result.passed);
        assert!(result.errors[0].contains("Invalid log_level"));

        let result = validate_opensm_config(&None, &None, &Some("999".to_string()));
        assert!(!result.passed);

        // Invalid: non-numeric
        let result = validate_opensm_config(&None, &None, &Some("abc".to_string()));
        assert!(!result.passed);
        assert!(result.errors[0].contains("numeric value"));

        // Invalid: negative (parsed as non-numeric since u32)
        let result = validate_opensm_config(&None, &None, &Some("-1".to_string()));
        assert!(!result.passed);
    }

    #[test]
    fn test_sm_role_parsing() {
        // Master via text
        assert_eq!(
            parse_sm_role("sminfo: sm lid 1 sm guid 0x1234 state Master"),
            "master"
        );

        // Standby via text
        assert_eq!(
            parse_sm_role("sminfo: sm lid 2 sm guid 0x5678 state Standby"),
            "standby"
        );

        // Master via numeric state (4)
        assert_eq!(
            parse_sm_role("sminfo: sm lid 1 sm guid 0x1234 state 4 priority 5"),
            "master"
        );

        // Standby via numeric state (3)
        assert_eq!(
            parse_sm_role("sminfo: sm lid 2 sm guid 0x5678 state 3 priority 5"),
            "standby"
        );

        // Unknown state
        assert_eq!(
            parse_sm_role("sminfo: sm lid 1 sm guid 0x1234 state 1"),
            "unknown"
        );

        // Empty output
        assert_eq!(parse_sm_role(""), "unknown");

        // Garbage output
        assert_eq!(parse_sm_role("error: no IB device found"), "unknown");

        // Case insensitivity for textual role
        assert_eq!(parse_sm_role("SMState:MASTER"), "master");
        assert_eq!(parse_sm_role("SMState:STANDBY"), "standby");
    }
}
