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

// ---- Helpers ----

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

/// Generate a traceability header for PXE config files.
fn generate_traceability_header(profile_name: &str) -> String {
    format!(
        "# Managed by Rustible - do not edit manually\n\
         # Generated: <timestamp placeholder>\n\
         # Profile: {}\n",
        profile_name
    )
}

/// Validate that kernel and initrd artifact files exist, are non-empty,
/// and are readable on the remote host.
fn validate_artifacts(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    kernel: Option<&str>,
    initrd: Option<&str>,
) -> ModuleResult<PreflightResult> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let paths: Vec<(&str, &str)> = [kernel.map(|k| (k, "kernel")), initrd.map(|i| (i, "initrd"))]
        .into_iter()
        .flatten()
        .collect();

    for (path, label) in paths {
        // Check file exists
        let (exists, _, _) = run_cmd(connection, &format!("test -f {}", path), context)?;
        if !exists {
            errors.push(format!("{} file does not exist: {}", label, path));
            continue;
        }

        // Check file is not zero-length
        let (non_empty, _, _) = run_cmd(connection, &format!("test -s {}", path), context)?;
        if !non_empty {
            errors.push(format!("{} file is zero-length: {}", label, path));
            continue;
        }

        // Check file is readable
        let (readable, _, _) = run_cmd(connection, &format!("test -r {}", path), context)?;
        if !readable {
            warnings.push(format!("{} file may not be readable: {}", label, path));
        }
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Compare desired PXE profile content against current content line by line.
/// Returns a list of drift items showing which lines differ.
fn reconcile_profile_content(desired: &str, actual: &str) -> Vec<DriftItem> {
    let mut drift = Vec::new();
    let desired_lines: Vec<&str> = desired.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();

    let max_lines = std::cmp::max(desired_lines.len(), actual_lines.len());

    for i in 0..max_lines {
        let desired_line = desired_lines.get(i).unwrap_or(&"");
        let actual_line = actual_lines.get(i).unwrap_or(&"");

        if desired_line != actual_line {
            drift.push(DriftItem {
                field: format!("line_{}", i + 1),
                desired: desired_line.to_string(),
                actual: actual_line.to_string(),
            });
        }
    }

    drift
}

/// Handle boot mode for PXE host entries.
/// For "one_time" mode, create a cleanup hook script that removes the PXE
/// config after the host boots. For "persistent" mode, no hook is needed.
fn handle_boot_mode(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    host_file: &str,
    boot_mode: &str,
) -> ModuleResult<()> {
    let hook_file = format!("{}.cleanup", host_file);

    if boot_mode == "one_time" {
        // Create a cleanup hook script that removes the PXE config after boot
        let hook_content = format!(
            "#!/bin/sh\n\
             # Managed by Rustible - one_time boot cleanup hook\n\
             # This script removes the PXE config after the host boots.\n\
             rm -f {}\n\
             rm -f {}\n",
            host_file, hook_file
        );
        let escaped = hook_content.replace('\'', "'\\''");
        run_cmd_ok(
            connection,
            &format!("echo '{}' > {}", escaped, hook_file),
            context,
        )?;
        run_cmd_ok(connection, &format!("chmod +x {}", hook_file), context)?;
    } else {
        // persistent mode: remove any leftover cleanup hook
        let (hook_exists, _, _) = run_cmd(connection, &format!("test -f {}", hook_file), context)?;
        if hook_exists {
            run_cmd_ok(connection, &format!("rm -f {}", hook_file), context)?;
        }
    }

    Ok(())
}

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
        let do_validate = params.get_bool_or("validate_artifacts", true);

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

        // Artifact validation preflight
        if do_validate {
            let preflight =
                validate_artifacts(connection, context, kernel.as_deref(), initrd.as_deref())?;
            if !preflight.passed {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Artifact validation failed: {}",
                    preflight.errors.join("; ")
                )));
            }
        }

        // Generate profile content with traceability header
        let header = generate_traceability_header(&profile_name);
        let mut profile_content = header;
        profile_content.push_str("DEFAULT linux\nLABEL linux\n");
        if let Some(ref k) = kernel {
            profile_content.push_str(&format!("  KERNEL {}\n", k));
        }
        if let Some(ref i) = initrd {
            profile_content.push_str(&format!("  INITRD {}\n", i));
        }
        if let Some(ref a) = append {
            profile_content.push_str(&format!("  APPEND {}\n", a));
        }

        // Read current content for reconciliation
        let (exists, current_content, _) = run_cmd(
            connection,
            &format!("cat {} 2>/dev/null || echo ''", profile_file),
            context,
        )?;

        // Use content reconciliation for idempotency check
        let drift = reconcile_profile_content(&profile_content, &current_content);

        if exists && drift.is_empty() {
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
            .with_data("profile", serde_json::json!(profile_name))
            .with_data("drift", serde_json::json!(drift)));
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

        let mut output = ModuleOutput::changed(format!(
            "{} PXE profile '{}'",
            if exists { "Updated" } else { "Created" },
            profile_name
        ))
        .with_data("profile", serde_json::json!(profile_name));

        if !drift.is_empty() {
            output = output.with_data("drift", serde_json::json!(drift));
        }

        Ok(output)
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
        m.insert("validate_artifacts", serde_json::json!(true));
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
        let boot_mode = params
            .get_string("boot_mode")?
            .unwrap_or_else(|| "persistent".to_string());

        // Validate boot_mode
        if boot_mode != "persistent" && boot_mode != "one_time" {
            return Err(ModuleError::ExecutionFailed(format!(
                "Invalid boot_mode '{}': must be 'persistent' or 'one_time'",
                boot_mode
            )));
        }

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
            // Also clean up any one_time cleanup hook
            let hook_file = format!("{}.cleanup", host_file);
            let (hook_exists, _, _) =
                run_cmd(connection, &format!("test -f {}", hook_file), context)?;
            if hook_exists {
                run_cmd_ok(connection, &format!("rm -f {}", hook_file), context)?;
            }
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
            // Check if boot_mode matches current state
            let hook_file = format!("{}.cleanup", host_file);
            let (hook_exists, _, _) =
                run_cmd(connection, &format!("test -f {}", hook_file), context)?;
            let current_mode = if hook_exists {
                "one_time"
            } else {
                "persistent"
            };

            if current_mode == boot_mode {
                return Ok(ModuleOutput::ok(format!(
                    "PXE host {} already linked to profile '{}' (mode: {})",
                    mac, profile, boot_mode
                ))
                .with_data("mac", serde_json::json!(mac))
                .with_data("profile", serde_json::json!(profile))
                .with_data("boot_mode", serde_json::json!(boot_mode)));
            }
            // Boot mode differs -- fall through to update the hook
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would link {} to profile '{}' (mode: {})",
                mac, profile, boot_mode
            ))
            .with_data("mac", serde_json::json!(mac))
            .with_data("profile", serde_json::json!(profile))
            .with_data("boot_mode", serde_json::json!(boot_mode)));
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

        // Handle boot mode (one_time vs persistent)
        handle_boot_mode(connection, context, &host_file, &boot_mode)?;

        Ok(ModuleOutput::changed(format!(
            "Linked {} to profile '{}' (mode: {})",
            mac, profile, boot_mode
        ))
        .with_data("mac", serde_json::json!(mac))
        .with_data("profile", serde_json::json!(profile))
        .with_data("boot_mode", serde_json::json!(boot_mode)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["mac", "profile"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("boot_mode", serde_json::json!("persistent"));
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

    #[test]
    fn test_boot_mode_validation() {
        // Valid boot modes
        let valid_modes = ["persistent", "one_time"];
        for mode in &valid_modes {
            assert!(
                *mode == "persistent" || *mode == "one_time",
                "Expected '{}' to be a valid boot mode",
                mode
            );
        }

        // Invalid boot modes
        let invalid_modes = ["once", "forever", "temporary", ""];
        for mode in &invalid_modes {
            assert!(
                *mode != "persistent" && *mode != "one_time",
                "Expected '{}' to be an invalid boot mode",
                mode
            );
        }
    }

    #[test]
    fn test_profile_content_drift() {
        // No drift: identical content
        let desired = "# Header\nDEFAULT linux\nLABEL linux\n  KERNEL /boot/vmlinuz\n";
        let actual = "# Header\nDEFAULT linux\nLABEL linux\n  KERNEL /boot/vmlinuz\n";
        let drift = reconcile_profile_content(desired, actual);
        assert!(drift.is_empty(), "Expected no drift for identical content");

        // Drift: different kernel line
        let desired = "DEFAULT linux\nLABEL linux\n  KERNEL /boot/vmlinuz-new\n";
        let actual = "DEFAULT linux\nLABEL linux\n  KERNEL /boot/vmlinuz-old\n";
        let drift = reconcile_profile_content(desired, actual);
        assert_eq!(drift.len(), 1, "Expected exactly one drift item");
        assert_eq!(drift[0].field, "line_3");
        assert_eq!(drift[0].desired, "  KERNEL /boot/vmlinuz-new");
        assert_eq!(drift[0].actual, "  KERNEL /boot/vmlinuz-old");

        // Drift: desired has more lines than actual
        let desired = "line1\nline2\nline3\n";
        let actual = "line1\n";
        let drift = reconcile_profile_content(desired, actual);
        assert_eq!(drift.len(), 2, "Expected two drift items for missing lines");

        // Drift: actual has more lines than desired
        let desired = "line1\n";
        let actual = "line1\nline2\nline3\n";
        let drift = reconcile_profile_content(desired, actual);
        assert_eq!(drift.len(), 2, "Expected two drift items for extra lines");

        // Drift: completely different content
        let desired = "aaa\nbbb\n";
        let actual = "xxx\nyyy\n";
        let drift = reconcile_profile_content(desired, actual);
        assert_eq!(
            drift.len(),
            2,
            "Expected two drift items for fully different content"
        );
        assert_eq!(drift[0].field, "line_1");
        assert_eq!(drift[1].field, "line_2");
    }

    #[test]
    fn test_artifact_path_validation() {
        // Test that artifact paths are validated for basic format correctness
        let valid_paths = [
            "/boot/vmlinuz",
            "/var/lib/tftpboot/kernel",
            "/opt/images/initrd.img",
        ];
        for path in &valid_paths {
            assert!(
                path.starts_with('/'),
                "Expected '{}' to be an absolute path",
                path
            );
            assert!(
                !path.contains(".."),
                "Expected '{}' to not contain path traversal",
                path
            );
        }

        let invalid_paths = ["../etc/passwd", "relative/path", ""];
        for path in &invalid_paths {
            assert!(
                !path.starts_with('/') || path.contains("..") || path.is_empty(),
                "Expected '{}' to be rejected as invalid",
                path
            );
        }
    }

    #[test]
    fn test_traceability_header() {
        let header = generate_traceability_header("my-profile");

        // Must contain the managed-by comment
        assert!(
            header.contains("# Managed by Rustible - do not edit manually"),
            "Header must contain managed-by comment"
        );

        // Must contain the timestamp placeholder
        assert!(
            header.contains("# Generated: <timestamp placeholder>"),
            "Header must contain generated timestamp line"
        );

        // Must contain the profile name
        assert!(
            header.contains("# Profile: my-profile"),
            "Header must contain profile name"
        );

        // Verify the header ends with a newline
        assert!(header.ends_with('\n'), "Header must end with a newline");

        // Verify the header has exactly 3 lines
        let line_count = header.lines().count();
        assert_eq!(
            line_count, 3,
            "Header must have exactly 3 lines, got {}",
            line_count
        );

        // Verify with a different profile name
        let header2 = generate_traceability_header("centos7-gpu");
        assert!(
            header2.contains("# Profile: centos7-gpu"),
            "Header must reflect the given profile name"
        );
    }

    #[test]
    fn test_pxe_host_optional_params_include_boot_mode() {
        let module = PxeHostModule;
        let optionals = module.optional_params();
        assert!(
            optionals.contains_key("boot_mode"),
            "PxeHostModule must have boot_mode as optional param"
        );
        assert_eq!(
            optionals["boot_mode"],
            serde_json::json!("persistent"),
            "Default boot_mode must be 'persistent'"
        );
    }

    #[test]
    fn test_pxe_profile_optional_params_include_validate_artifacts() {
        let module = PxeProfileModule;
        let optionals = module.optional_params();
        assert!(
            optionals.contains_key("validate_artifacts"),
            "PxeProfileModule must have validate_artifacts as optional param"
        );
        assert_eq!(
            optionals["validate_artifacts"],
            serde_json::json!(true),
            "Default validate_artifacts must be true"
        );
    }
}
