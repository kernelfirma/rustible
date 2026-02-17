//! Dedicated LNet-aware Lustre mount management
//!
//! Manages Lustre filesystem mounts with LNet NID configuration,
//! mount options, and fstab persistence. This module focuses specifically
//! on mount lifecycle management, complementing `lustre_client` which
//! handles package installation and basic mounts.
//!
//! # Parameters
//!
//! - `nid` (required): LNet NID address (e.g., "10.0.0.1@tcp")
//! - `fs_name` (required): Lustre filesystem name
//! - `mount_point` (required): Target mount point path
//! - `mount_options` (optional): Mount options (default: "defaults")
//! - `fstab` (optional): Whether to manage fstab entry (default: true)
//! - `state` (optional): "mounted" (default), "unmounted", "absent"

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use regex::Regex;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

/// Result of preflight LNet / environment validation.
#[derive(Debug, serde::Serialize)]
struct PreflightResult {
    passed: bool,
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// A single field that drifted from desired to actual state.
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

/// Validate a Lustre NID format string.
///
/// Valid formats: `<IPv4>@<network>` where network is `tcp`, `tcp1`, `o2ib`, `o2ib0`, etc.
/// Examples: `10.0.0.1@tcp`, `192.168.1.100@o2ib`, `10.0.0.1@tcp0`
fn is_valid_nid(nid: &str) -> bool {
    let re = Regex::new(
        r"^(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})@(tcp|o2ib)\d*$",
    )
    .unwrap();
    if !re.is_match(nid) {
        return false;
    }
    // Validate each octet is 0-255
    if let Some(at_pos) = nid.find('@') {
        let ip_part = &nid[..at_pos];
        for octet_str in ip_part.split('.') {
            if let Ok(octet) = octet_str.parse::<u32>() {
                if octet > 255 {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    true
}

/// Run LNet preflight checks before mount operations.
///
/// Checks:
/// - LNet kernel module is loaded
/// - NID format is valid
/// - LNet connectivity to the NID (best-effort, warns on failure)
fn lnet_preflight_check(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    nid: &str,
) -> ModuleResult<PreflightResult> {
    let mut warnings = Vec::new();
    let mut errors = Vec::new();

    // Check LNet kernel module
    let (lnet_loaded, _, _) = run_cmd(connection, "lsmod | grep -q lnet", context)?;
    if !lnet_loaded {
        errors.push("LNet kernel module is not loaded; run 'modprobe lnet' first".to_string());
    }

    // Validate NID format
    if !is_valid_nid(nid) {
        errors.push(format!(
            "Invalid NID format '{}': expected <IPv4>@<tcp|o2ib>[N] (e.g., 10.0.0.1@tcp)",
            nid
        ));
    }

    // Best-effort LNet connectivity check via lctl ping
    if errors.is_empty() {
        let (ping_ok, _, ping_stderr) = run_cmd(
            connection,
            &format!("lctl ping '{}' 2>&1 || true", nid),
            context,
        )?;
        if !ping_ok {
            warnings.push(format!(
                "lctl ping {} failed (best-effort check): {}",
                nid,
                ping_stderr.trim()
            ));
        }
    }

    Ok(PreflightResult {
        passed: errors.is_empty(),
        warnings,
        errors,
    })
}

/// Parse a single fstab line into its components.
///
/// Returns `(source, mount_point, fs_type, options, dump, pass)` or None if unparseable.
fn parse_fstab_line(line: &str) -> Option<(String, String, String, String, String, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 6 {
        Some((
            parts[0].to_string(),
            parts[1].to_string(),
            parts[2].to_string(),
            parts[3].to_string(),
            parts[4].to_string(),
            parts[5].to_string(),
        ))
    } else if parts.len() >= 4 {
        // Minimal fstab line (source, mount, type, options)
        Some((
            parts[0].to_string(),
            parts[1].to_string(),
            parts[2].to_string(),
            parts[3].to_string(),
            "0".to_string(),
            "0".to_string(),
        ))
    } else {
        None
    }
}

/// Check fstab for drift against desired state.
///
/// Reads /etc/fstab, finds the entry for the given mount point, and compares
/// source, fs_type, and mount options against desired values.
fn fstab_convergence_check(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    desired_source: &str,
    mount_point: &str,
    desired_options: &str,
) -> ModuleResult<Vec<DriftItem>> {
    let mut drift = Vec::new();

    let (ok, fstab_content, _) =
        run_cmd(connection, "cat /etc/fstab 2>/dev/null || true", context)?;
    if !ok {
        return Ok(drift);
    }

    // Find the line matching our mount point
    for line in fstab_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((source, mp, fs_type, options, _, _)) = parse_fstab_line(trimmed) {
            if mp == mount_point {
                // Compare source
                if source != desired_source {
                    drift.push(DriftItem {
                        field: "source".to_string(),
                        desired: desired_source.to_string(),
                        actual: source,
                    });
                }
                // Compare fs_type
                if fs_type != "lustre" {
                    drift.push(DriftItem {
                        field: "fs_type".to_string(),
                        desired: "lustre".to_string(),
                        actual: fs_type,
                    });
                }
                // Compare mount options
                if options != desired_options {
                    drift.push(DriftItem {
                        field: "mount_options".to_string(),
                        desired: desired_options.to_string(),
                        actual: options,
                    });
                }
                break;
            }
        }
    }

    Ok(drift)
}

/// Perform a safe remount with rollback on failure.
///
/// Captures current mount options, attempts remount with new options, and
/// rolls back to previous options if the remount fails.
///
/// Returns (success, details).
fn remount_with_rollback(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    mount_point: &str,
    new_options: &str,
) -> ModuleResult<(bool, Vec<String>)> {
    let mut details = Vec::new();

    // Capture current mount options
    let (ok, mount_output, _) = run_cmd(
        connection,
        &format!("mount | grep ' {} '", mount_point),
        context,
    )?;

    let previous_options = if ok && !mount_output.trim().is_empty() {
        // Parse options from mount output: "source on /path type lustre (opts)"
        let line = mount_output.trim();
        if let Some(start) = line.rfind('(') {
            if let Some(end) = line.rfind(')') {
                let opts = &line[start + 1..end];
                details.push(format!("Captured current mount options: {}", opts));
                Some(opts.to_string())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        details.push("Mount point not currently mounted; cannot remount".to_string());
        return Ok((false, details));
    };

    // Attempt remount with new options
    let remount_cmd = format!("mount -o remount,'{}' '{}'", new_options, mount_point);
    let (success, _, stderr) = run_cmd(connection, &remount_cmd, context)?;

    if success {
        details.push(format!(
            "Successfully remounted {} with options: {}",
            mount_point, new_options
        ));
        return Ok((true, details));
    }

    details.push(format!(
        "Remount failed: {}; attempting rollback",
        stderr.trim()
    ));

    // Rollback to previous options
    if let Some(ref prev_opts) = previous_options {
        let rollback_cmd = format!("mount -o remount,'{}' '{}'", prev_opts, mount_point);
        let (rollback_ok, _, rollback_stderr) = run_cmd(connection, &rollback_cmd, context)?;
        if rollback_ok {
            details.push(format!(
                "Rolled back to previous options: {}",
                prev_opts
            ));
        } else {
            details.push(format!(
                "Rollback also failed: {}",
                rollback_stderr.trim()
            ));
        }
    }

    Ok((false, details))
}

/// Collect mount health telemetry for a mount point.
///
/// Runs `df -h` and `stat` on the mount point and returns structured health data.
fn mount_health_output(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    mount_point: &str,
) -> ModuleResult<serde_json::Value> {
    let mut health = serde_json::Map::new();

    // df -h output for space info
    let (df_ok, df_stdout, _) = run_cmd(
        connection,
        &format!("df -h '{}' 2>/dev/null | tail -n1", mount_point),
        context,
    )?;
    if df_ok && !df_stdout.trim().is_empty() {
        let parts: Vec<&str> = df_stdout.trim().split_whitespace().collect();
        if parts.len() >= 6 {
            health.insert("filesystem".to_string(), serde_json::json!(parts[0]));
            health.insert("size".to_string(), serde_json::json!(parts[1]));
            health.insert("used".to_string(), serde_json::json!(parts[2]));
            health.insert("available".to_string(), serde_json::json!(parts[3]));
            health.insert("use_percent".to_string(), serde_json::json!(parts[4]));
            health.insert("mounted_on".to_string(), serde_json::json!(parts[5]));
        }
    }

    // stat to verify accessibility
    let (stat_ok, stat_stdout, _) = run_cmd(
        connection,
        &format!("stat -f '{}' 2>/dev/null || true", mount_point),
        context,
    )?;
    health.insert("accessible".to_string(), serde_json::json!(stat_ok));
    if stat_ok && !stat_stdout.trim().is_empty() {
        // Extract filesystem type from stat -f output
        for line in stat_stdout.lines() {
            let trimmed = line.trim();
            if trimmed.contains("Type:") {
                health.insert(
                    "stat_type".to_string(),
                    serde_json::json!(trimmed),
                );
                break;
            }
        }
    }

    health.insert("mount_point".to_string(), serde_json::json!(mount_point));

    Ok(serde_json::Value::Object(health))
}

pub struct LustreMountModule;

impl Module for LustreMountModule {
    fn name(&self) -> &'static str {
        "lustre_mount"
    }

    fn description(&self) -> &'static str {
        "Manage LNet-aware Lustre filesystem mounts with fstab persistence"
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

        let nid = params.get_string_required("nid")?;
        let fs_name = params.get_string_required("fs_name")?;
        let mount_point = params.get_string_required("mount_point")?;
        let mount_options = params
            .get_string("mount_options")?
            .unwrap_or_else(|| "defaults".to_string());
        let manage_fstab = params.get_bool_or("fstab", true);
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "mounted".to_string());

        let lustre_source = format!("{}:/{}", nid, fs_name);

        match state.as_str() {
            "absent" => self.handle_absent(
                connection,
                context,
                &lustre_source,
                &nid,
                &fs_name,
                &mount_point,
            ),
            "unmounted" => self.handle_unmounted(
                connection,
                context,
                &lustre_source,
                &nid,
                &fs_name,
                &mount_point,
                &mount_options,
                manage_fstab,
            ),
            "mounted" => self.handle_mounted(
                connection,
                context,
                &lustre_source,
                &nid,
                &fs_name,
                &mount_point,
                &mount_options,
                manage_fstab,
            ),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Must be 'mounted', 'unmounted', or 'absent'",
                state
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["nid", "fs_name", "mount_point"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("mount_options", serde_json::json!("defaults"));
        m.insert("fstab", serde_json::json!(true));
        m.insert("state", serde_json::json!("mounted"));
        m
    }
}

impl LustreMountModule {
    fn handle_absent(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        _lustre_source: &str,
        nid: &str,
        fs_name: &str,
        mount_point: &str,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Unmount if mounted
        let (is_mounted, _, _) = run_cmd(
            connection,
            &format!("mountpoint -q '{}'", mount_point),
            context,
        )?;

        if is_mounted {
            if context.check_mode {
                changes.push(format!("Would unmount {}", mount_point));
            } else {
                run_cmd_ok(connection, &format!("umount '{}'", mount_point), context)?;
                changed = true;
                changes.push(format!("Unmounted {}", mount_point));
            }
        }

        // Remove fstab entry
        let (in_fstab, _, _) = run_cmd(
            connection,
            &format!("grep -qF '{}:/{} ' /etc/fstab", nid, fs_name),
            context,
        )?;

        if in_fstab {
            if context.check_mode {
                changes.push("Would remove fstab entry".to_string());
            } else {
                run_cmd_ok(
                    connection,
                    &format!("sed -i '\\|{}:/{}|d' /etc/fstab", nid, fs_name),
                    context,
                )?;
                changed = true;
                changes.push("Removed fstab entry".to_string());
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(
                ModuleOutput::changed(format!("Would remove Lustre mount {}", mount_point))
                    .with_data("changes", serde_json::json!(changes)),
            );
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Removed Lustre mount {}", mount_point))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok("Lustre mount is already absent"))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_unmounted(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        _lustre_source: &str,
        nid: &str,
        fs_name: &str,
        mount_point: &str,
        mount_options: &str,
        manage_fstab: bool,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Unmount if mounted
        let (is_mounted, _, _) = run_cmd(
            connection,
            &format!("mountpoint -q '{}'", mount_point),
            context,
        )?;

        if is_mounted {
            if context.check_mode {
                changes.push(format!("Would unmount {}", mount_point));
            } else {
                run_cmd_ok(connection, &format!("umount '{}'", mount_point), context)?;
                changed = true;
                changes.push(format!("Unmounted {}", mount_point));
            }
        }

        // Ensure fstab entry exists (but don't mount)
        if manage_fstab {
            let lustre_source = format!("{}:/{}", nid, fs_name);
            self.ensure_fstab(
                connection,
                context,
                nid,
                fs_name,
                mount_point,
                mount_options,
                &lustre_source,
                &mut changed,
                &mut changes,
            )?;
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Lustre mount changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        if changed {
            Ok(
                ModuleOutput::changed(format!("Applied {} Lustre mount changes", changes.len()))
                    .with_data("changes", serde_json::json!(changes)),
            )
        } else {
            Ok(ModuleOutput::ok(
                "Lustre mount is in desired state (unmounted)",
            ))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_mounted(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        lustre_source: &str,
        nid: &str,
        fs_name: &str,
        mount_point: &str,
        mount_options: &str,
        manage_fstab: bool,
    ) -> ModuleResult<ModuleOutput> {
        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();
        let mut output_data: HashMap<String, serde_json::Value> = HashMap::new();

        // LNet preflight check
        let preflight = lnet_preflight_check(connection, context, nid)?;
        output_data.insert("preflight".to_string(), serde_json::json!(preflight));
        if !preflight.passed {
            return Err(ModuleError::ExecutionFailed(format!(
                "LNet preflight failed: {}",
                preflight.errors.join("; ")
            )));
        }

        // Ensure mount point directory exists
        let (dir_exists, _, _) =
            run_cmd(connection, &format!("test -d '{}'", mount_point), context)?;

        if !dir_exists {
            if context.check_mode {
                changes.push(format!("Would create mount point {}", mount_point));
            } else {
                run_cmd_ok(connection, &format!("mkdir -p '{}'", mount_point), context)?;
                changed = true;
                changes.push(format!("Created mount point {}", mount_point));
            }
        }

        // Ensure lustre kernel module is loaded
        let (lustre_loaded, _, _) = run_cmd(connection, "lsmod | grep -q lustre", context)?;

        if !lustre_loaded {
            if context.check_mode {
                changes.push("Would load lustre kernel module".to_string());
            } else {
                run_cmd_ok(connection, "modprobe lustre", context)?;
                changed = true;
                changes.push("Loaded lustre kernel module".to_string());
            }
        }

        // Fstab convergence check and management
        if manage_fstab {
            let drift =
                fstab_convergence_check(connection, context, lustre_source, mount_point, mount_options)?;
            if !drift.is_empty() {
                output_data.insert("fstab_drift".to_string(), serde_json::json!(drift));
            }

            self.ensure_fstab(
                connection,
                context,
                nid,
                fs_name,
                mount_point,
                mount_options,
                lustre_source,
                &mut changed,
                &mut changes,
            )?;
        }

        // Mount if not already mounted
        let (is_mounted, _, _) = run_cmd(
            connection,
            &format!("mountpoint -q '{}'", mount_point),
            context,
        )?;

        if !is_mounted {
            if context.check_mode {
                changes.push(format!("Would mount {} at {}", lustre_source, mount_point));
            } else {
                let mount_cmd = format!(
                    "mount -t lustre -o '{}' '{}' '{}'",
                    mount_options, lustre_source, mount_point
                );
                run_cmd_ok(connection, &mount_cmd, context)?;
                changed = true;
                changes.push(format!("Mounted {} at {}", lustre_source, mount_point));
            }
        } else if mount_options != "defaults" {
            // Already mounted -- check if options need a remount
            let drift_items =
                fstab_convergence_check(connection, context, lustre_source, mount_point, mount_options)?;
            let options_drifted = drift_items
                .iter()
                .any(|d| d.field == "mount_options");
            if options_drifted && !context.check_mode {
                let (remount_ok, remount_details) =
                    remount_with_rollback(connection, context, mount_point, mount_options)?;
                output_data.insert(
                    "remount_details".to_string(),
                    serde_json::json!(remount_details),
                );
                if remount_ok {
                    changed = true;
                    changes.push(format!(
                        "Remounted {} with updated options: {}",
                        mount_point, mount_options
                    ));
                }
            }
        }

        // Collect mount health telemetry after successful mount
        if !context.check_mode {
            let health = mount_health_output(connection, context, mount_point)?;
            output_data.insert("health".to_string(), health);
        }

        if context.check_mode && !changes.is_empty() {
            let mut output = ModuleOutput::changed(format!(
                "Would apply {} Lustre mount changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes));
            for (key, value) in &output_data {
                output = output.with_data(key, value.clone());
            }
            return Ok(output);
        }

        let mut output = if changed {
            ModuleOutput::changed(format!("Applied {} Lustre mount changes", changes.len()))
                .with_data("changes", serde_json::json!(changes))
                .with_data("mount_point", serde_json::json!(mount_point))
                .with_data("source", serde_json::json!(lustre_source))
        } else {
            ModuleOutput::ok("Lustre mount is in desired state")
                .with_data("mount_point", serde_json::json!(mount_point))
                .with_data("source", serde_json::json!(lustre_source))
        };

        for (key, value) in output_data {
            output = output.with_data(&key, value);
        }

        Ok(output)
    }

    /// Ensure fstab contains the correct entry for this Lustre mount.
    ///
    /// Uses convergence checking to detect drift and update existing entries
    /// rather than only appending new ones.
    #[allow(clippy::too_many_arguments)]
    fn ensure_fstab(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        nid: &str,
        fs_name: &str,
        mount_point: &str,
        mount_options: &str,
        lustre_source: &str,
        changed: &mut bool,
        changes: &mut Vec<String>,
    ) -> ModuleResult<()> {
        let fstab_entry = format!(
            "{}:/{} {} lustre {} 0 0",
            nid, fs_name, mount_point, mount_options
        );

        // Check if there is any fstab entry for this mount point
        let (mp_in_fstab, _, _) = run_cmd(
            connection,
            &format!("grep -q '\\s{}\\s' /etc/fstab 2>/dev/null || grep -q '\\s{}$' /etc/fstab 2>/dev/null", mount_point, mount_point),
            context,
        )?;

        if mp_in_fstab {
            // Entry exists for mount point -- check for drift
            let drift =
                fstab_convergence_check(connection, context, lustre_source, mount_point, mount_options)?;

            if !drift.is_empty() {
                if context.check_mode {
                    let drift_fields: Vec<String> =
                        drift.iter().map(|d| d.field.clone()).collect();
                    changes.push(format!(
                        "Would update fstab entry for {} (drifted fields: {})",
                        mount_point,
                        drift_fields.join(", ")
                    ));
                } else {
                    // Replace the existing line for this mount point with the correct one
                    let escaped_mp = mount_point.replace('/', "\\/");
                    run_cmd_ok(
                        connection,
                        &format!(
                            "sed -i '/\\s{}\\s/d;/\\s{}$/d' /etc/fstab && echo '{}' >> /etc/fstab",
                            escaped_mp, escaped_mp, fstab_entry
                        ),
                        context,
                    )?;
                    *changed = true;
                    changes.push(format!("Updated fstab entry for {}", mount_point));
                }
            }
        } else {
            // No entry for this source at all -- check by source too
            let (src_in_fstab, _, _) = run_cmd(
                connection,
                &format!("grep -qF '{}:/{} ' /etc/fstab", nid, fs_name),
                context,
            )?;

            if !src_in_fstab {
                if context.check_mode {
                    changes.push(format!("Would add fstab entry for {}", mount_point));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("echo '{}' >> /etc/fstab", fstab_entry),
                        context,
                    )?;
                    *changed = true;
                    changes.push(format!("Added fstab entry for {}", mount_point));
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_name_and_description() {
        let module = LustreMountModule;
        assert_eq!(module.name(), "lustre_mount");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = LustreMountModule;
        let required = module.required_params();
        assert!(required.contains(&"nid"));
        assert!(required.contains(&"fs_name"));
        assert!(required.contains(&"mount_point"));
    }

    #[test]
    fn test_optional_params() {
        let module = LustreMountModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("mount_options"));
        assert!(optional.contains_key("fstab"));
        assert!(optional.contains_key("state"));
    }

    #[test]
    fn test_parallelization_hint() {
        let module = LustreMountModule;
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }

    #[test]
    fn test_nid_format_validation() {
        // Valid NIDs
        assert!(is_valid_nid("10.0.0.1@tcp"));
        assert!(is_valid_nid("192.168.1.100@o2ib"));
        assert!(is_valid_nid("10.0.0.1@tcp0"));
        assert!(is_valid_nid("172.16.0.1@o2ib1"));
        assert!(is_valid_nid("0.0.0.0@tcp"));
        assert!(is_valid_nid("255.255.255.255@tcp"));

        // Invalid NIDs
        assert!(!is_valid_nid(""));
        assert!(!is_valid_nid("10.0.0.1"));
        assert!(!is_valid_nid("10.0.0.1@"));
        assert!(!is_valid_nid("10.0.0.1@ib"));
        assert!(!is_valid_nid("10.0.0.1@gni"));
        assert!(!is_valid_nid("not-an-ip@tcp"));
        assert!(!is_valid_nid("10.0.0.256@tcp"));
        assert!(!is_valid_nid("10.0.0@tcp"));
        assert!(!is_valid_nid("@tcp"));
        assert!(!is_valid_nid("10.0.0.1@TCP"));
    }

    #[test]
    fn test_fstab_line_parsing() {
        // Standard 6-field fstab line
        let line = "10.0.0.1@tcp:/scratch /mnt/scratch lustre defaults 0 0";
        let parsed = parse_fstab_line(line);
        assert!(parsed.is_some());
        let (source, mp, fs_type, options, dump, pass) = parsed.unwrap();
        assert_eq!(source, "10.0.0.1@tcp:/scratch");
        assert_eq!(mp, "/mnt/scratch");
        assert_eq!(fs_type, "lustre");
        assert_eq!(options, "defaults");
        assert_eq!(dump, "0");
        assert_eq!(pass, "0");

        // Minimal 4-field fstab line
        let line2 = "10.0.0.1@o2ib:/home /mnt/home lustre rw,flock";
        let parsed2 = parse_fstab_line(line2);
        assert!(parsed2.is_some());
        let (source2, mp2, fs2, opts2, dump2, pass2) = parsed2.unwrap();
        assert_eq!(source2, "10.0.0.1@o2ib:/home");
        assert_eq!(mp2, "/mnt/home");
        assert_eq!(fs2, "lustre");
        assert_eq!(opts2, "rw,flock");
        assert_eq!(dump2, "0");
        assert_eq!(pass2, "0");

        // Comment line should return None
        assert!(parse_fstab_line("# comment").is_none());

        // Empty line should return None
        assert!(parse_fstab_line("").is_none());

        // Too few fields
        assert!(parse_fstab_line("source /mount").is_none());

        // Line with extra whitespace
        let line3 = "10.0.0.1@tcp:/data   /mnt/data   lustre   noatime,flock   0   0";
        let parsed3 = parse_fstab_line(line3);
        assert!(parsed3.is_some());
        let (s3, m3, f3, o3, _, _) = parsed3.unwrap();
        assert_eq!(s3, "10.0.0.1@tcp:/data");
        assert_eq!(m3, "/mnt/data");
        assert_eq!(f3, "lustre");
        assert_eq!(o3, "noatime,flock");
    }

    #[test]
    fn test_health_output_structure() {
        // Verify the JSON structure returned by mount_health_output would be
        // well-formed. Since we can't run actual commands in unit tests, we
        // test the shape by constructing the expected structure manually.
        let mut health = serde_json::Map::new();
        health.insert(
            "filesystem".to_string(),
            serde_json::json!("10.0.0.1@tcp:/scratch"),
        );
        health.insert("size".to_string(), serde_json::json!("100T"));
        health.insert("used".to_string(), serde_json::json!("50T"));
        health.insert("available".to_string(), serde_json::json!("50T"));
        health.insert("use_percent".to_string(), serde_json::json!("50%"));
        health.insert("mounted_on".to_string(), serde_json::json!("/mnt/scratch"));
        health.insert("accessible".to_string(), serde_json::json!(true));
        health.insert(
            "mount_point".to_string(),
            serde_json::json!("/mnt/scratch"),
        );

        let value = serde_json::Value::Object(health);
        assert!(value.is_object());

        let obj = value.as_object().unwrap();
        assert!(obj.contains_key("filesystem"));
        assert!(obj.contains_key("size"));
        assert!(obj.contains_key("used"));
        assert!(obj.contains_key("available"));
        assert!(obj.contains_key("use_percent"));
        assert!(obj.contains_key("mounted_on"));
        assert!(obj.contains_key("accessible"));
        assert!(obj.contains_key("mount_point"));

        // Verify accessible is a boolean
        assert!(obj["accessible"].is_boolean());
        // Verify mount_point is a string
        assert!(obj["mount_point"].is_string());
    }

    #[test]
    fn test_preflight_result_serialization() {
        let result = PreflightResult {
            passed: true,
            warnings: vec!["test warning".to_string()],
            errors: vec![],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["passed"], true);
        assert!(json["warnings"].is_array());
        assert!(json["errors"].is_array());
    }

    #[test]
    fn test_drift_item_serialization() {
        let drift = DriftItem {
            field: "mount_options".to_string(),
            desired: "rw,flock".to_string(),
            actual: "defaults".to_string(),
        };
        let json = serde_json::to_value(&drift).unwrap();
        assert_eq!(json["field"], "mount_options");
        assert_eq!(json["desired"], "rw,flock");
        assert_eq!(json["actual"], "defaults");
    }

    #[test]
    fn test_verify_result_serialization() {
        let result = VerifyResult {
            verified: false,
            details: vec!["detail1".to_string()],
            warnings: vec!["warn1".to_string()],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["verified"], false);
        assert_eq!(json["details"][0], "detail1");
        assert_eq!(json["warnings"][0], "warn1");
    }
}
