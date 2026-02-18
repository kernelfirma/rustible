//! InfiniBand partition management module
//!
//! Manage IB partition keys via partitions.conf for OpenSM.
//!
//! # Parameters
//!
//! - `pkey` (required): Partition key (hex format, e.g., "0x7fff")
//! - `members` (optional): List of node GUIDs/names with access levels (e.g., "guid1=full")
//! - `state` (optional): "present" (default) or "absent"
//! - `ipoib` (optional): Enable IPoIB for this partition (boolean)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

// ---------------------------------------------------------------------------
// Helper structs
// ---------------------------------------------------------------------------

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

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct PartitionEntry {
    pkey: String,
    ipoib: bool,
    members: Vec<String>,
}

// ---------------------------------------------------------------------------
// Config parsing
// ---------------------------------------------------------------------------

/// Parse a partitions.conf file into a list of `PartitionEntry` values.
///
/// The expected format is one entry per non-blank, non-comment line:
///
/// ```text
/// Default=0x7fff, ipoib : ALL=full ;
/// MyPartition=0x8001, ipoib : guid1=full, guid2=limited ;
/// ```
///
/// We also accept the simpler format produced by this module itself:
///
/// ```text
/// 0x7fff,ipoib=ALL=full
/// ```
fn parse_partitions_conf(content: &str) -> Vec<PartitionEntry> {
    let mut entries = Vec::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();

        // Skip blanks and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Try the OpenSM "Name=pkey, flags : members ;" format first.
        if let Some(entry) = parse_opensm_line(line) {
            entries.push(entry);
            continue;
        }

        // Fallback: simple "pkey[,ipoib]=members" format produced by older code.
        if let Some(entry) = parse_simple_line(line) {
            entries.push(entry);
        }
    }

    entries
}

/// Parse a line in the OpenSM partitions.conf format.
///
/// ```text
/// Default=0x7fff, ipoib : ALL=full ;
/// MyPartition=0x8001 : guid1=full, guid2=limited ;
/// 0x7fff, ipoib : ALL=full ;
/// ```
fn parse_opensm_line(line: &str) -> Option<PartitionEntry> {
    // Strip trailing semicolon and whitespace.
    let line = line.trim_end_matches(';').trim();

    // Split on ':' to separate the header from the member list.
    // If there is no ':', this is not OpenSM format -- let the simple parser handle it.
    let (header, members_part) = {
        let mut parts = line.splitn(2, ':');
        let header = parts.next()?.trim();
        let members_part = match parts.next() {
            Some(s) => s.trim(),
            None => return None, // No ':' separator -- not OpenSM format
        };
        (header, members_part)
    };

    if header.is_empty() {
        return None;
    }

    // The header may be "Name=0xNNNN, ipoib" or just "0xNNNN, ipoib".
    // Determine the pkey and flags.
    let (pkey, ipoib) = parse_header(header)?;

    // Parse the member list: "ALL=full" or "guid1=full, guid2=limited"
    let members = parse_member_list(members_part);

    Some(PartitionEntry {
        pkey,
        ipoib,
        members,
    })
}

/// Parse the header portion to extract the pkey and ipoib flag.
fn parse_header(header: &str) -> Option<(String, bool)> {
    // Split on commas for flags.
    let parts: Vec<&str> = header.split(',').map(|s| s.trim()).collect();

    if parts.is_empty() {
        return None;
    }

    // The first part is either "Name=0xNNNN" or just "0xNNNN".
    let first = parts[0];
    let pkey = if first.contains('=') {
        // "Name=0xNNNN" -- take the value after '='.
        // But be careful: if it looks like a pkey itself (starts with 0x), treat it as a
        // simple line, not an OpenSM header.
        let eq_parts: Vec<&str> = first.splitn(2, '=').collect();
        let left = eq_parts[0].trim();
        let right = eq_parts[1].trim();
        if left.starts_with("0x") || left.starts_with("0X") {
            // This is the simple format "0xNNNN=members", bail out.
            return None;
        }
        right.to_string()
    } else {
        first.to_string()
    };

    // Must look like a pkey (starts with 0x).
    if !pkey.starts_with("0x") && !pkey.starts_with("0X") {
        return None;
    }

    let ipoib = parts[1..].iter().any(|p| p.eq_ignore_ascii_case("ipoib"));

    Some((pkey, ipoib))
}

/// Parse a comma-separated member list like "ALL=full, guid2=limited".
fn parse_member_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a simple line like "0x7fff,ipoib=ALL=full,guid2=limited".
fn parse_simple_line(line: &str) -> Option<PartitionEntry> {
    // We expect the pkey at the start, optionally followed by ",ipoib", then "=members".
    let line = line.trim_end_matches(';').trim();
    if !line.starts_with("0x") && !line.starts_with("0X") {
        return None;
    }

    // Split at the first '=' to separate pkey[,flags] from members.
    let (prefix, members_raw) = {
        let mut parts = line.splitn(2, '=');
        let prefix = parts.next()?.trim();
        let members_raw = parts.next().unwrap_or("").trim();
        (prefix, members_raw)
    };

    // prefix is e.g. "0x7fff,ipoib" or "0x7fff"
    let prefix_parts: Vec<&str> = prefix.split(',').map(|s| s.trim()).collect();
    let pkey = prefix_parts[0].to_string();
    let ipoib = prefix_parts[1..]
        .iter()
        .any(|p| p.eq_ignore_ascii_case("ipoib"));

    // members_raw might contain further '=' for access levels, e.g. "ALL=full,guid2=limited"
    // or it could be comma-separated members like "node1,node2".
    let members: Vec<String> = if members_raw.is_empty() {
        Vec::new()
    } else {
        // The members part is everything after the first '=' in the original
        // line. Members are comma-separated, and each member can contain '='.
        members_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    Some(PartitionEntry {
        pkey,
        ipoib,
        members,
    })
}

// ---------------------------------------------------------------------------
// Conflict detection
// ---------------------------------------------------------------------------

/// Detect PKey conflicts in a set of partition entries.
///
/// Checks for:
/// - Duplicate PKeys (same pkey value in multiple entries)
/// - Conflicting member assignments (same GUID with different access levels across partitions)
fn detect_pkey_conflicts(entries: &[PartitionEntry]) -> PreflightResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // 1. Check for duplicate pkeys.
    let mut pkey_counts: HashMap<&str, usize> = HashMap::new();
    for entry in entries {
        *pkey_counts.entry(entry.pkey.as_str()).or_insert(0) += 1;
    }
    for (pkey, count) in &pkey_counts {
        if *count > 1 {
            errors.push(format!(
                "Duplicate pkey {} appears {} times",
                pkey, count
            ));
        }
    }

    // 2. Check for conflicting member access levels across partitions.
    // Build a map: guid -> list of (pkey, access_level).
    let mut member_map: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for entry in entries {
        for member in &entry.members {
            let (guid, access) = split_member_access(member);
            member_map
                .entry(guid.to_string())
                .or_default()
                .push((entry.pkey.clone(), access.to_string()));
        }
    }

    for (guid, assignments) in &member_map {
        if assignments.len() > 1 {
            // Check if the same guid has different access levels.
            let access_levels: Vec<&str> = assignments.iter().map(|(_, a)| a.as_str()).collect();
            let first = access_levels[0];
            let has_conflict = access_levels.iter().any(|a| *a != first);
            if has_conflict {
                let details: Vec<String> = assignments
                    .iter()
                    .map(|(pk, acc)| format!("{}={}", pk, acc))
                    .collect();
                warnings.push(format!(
                    "Member {} has different access levels across partitions: {}",
                    guid,
                    details.join(", ")
                ));
            }
        }
    }

    let passed = errors.is_empty();
    PreflightResult {
        passed,
        warnings,
        errors,
    }
}

/// Split a member string like "guid1=full" into ("guid1", "full").
/// If no access level is specified, returns the whole string and "full" as default.
fn split_member_access(member: &str) -> (&str, &str) {
    match member.split_once('=') {
        Some((guid, access)) => (guid.trim(), access.trim()),
        None => (member.trim(), "full"),
    }
}

// ---------------------------------------------------------------------------
// Member reconciliation
// ---------------------------------------------------------------------------

/// Given current and desired partition entries for the same pkey, compute the minimal
/// set of member changes needed. Returns a list of DriftItem describing additions,
/// removals, and access level changes.
fn reconcile_members(current: &PartitionEntry, desired: &PartitionEntry) -> Vec<DriftItem> {
    let mut drift = Vec::new();

    // Check ipoib flag drift.
    if current.ipoib != desired.ipoib {
        drift.push(DriftItem {
            field: "ipoib".to_string(),
            desired: desired.ipoib.to_string(),
            actual: current.ipoib.to_string(),
        });
    }

    // Build maps of guid -> access for current and desired.
    let current_map: HashMap<&str, &str> = current
        .members
        .iter()
        .map(|m| split_member_access(m))
        .collect();
    let desired_map: HashMap<&str, &str> = desired
        .members
        .iter()
        .map(|m| split_member_access(m))
        .collect();

    // Members to add (in desired but not in current).
    for (guid, access) in &desired_map {
        match current_map.get(guid) {
            None => {
                drift.push(DriftItem {
                    field: format!("member_add:{}", guid),
                    desired: format!("{}={}", guid, access),
                    actual: "absent".to_string(),
                });
            }
            Some(current_access) => {
                if current_access != access {
                    drift.push(DriftItem {
                        field: format!("member_access:{}", guid),
                        desired: format!("{}={}", guid, access),
                        actual: format!("{}={}", guid, current_access),
                    });
                }
            }
        }
    }

    // Members to remove (in current but not in desired).
    for (guid, access) in &current_map {
        if !desired_map.contains_key(guid) {
            drift.push(DriftItem {
                field: format!("member_remove:{}", guid),
                desired: "absent".to_string(),
                actual: format!("{}={}", guid, access),
            });
        }
    }

    drift
}

// ---------------------------------------------------------------------------
// Config serialization
// ---------------------------------------------------------------------------

/// Serialize a PartitionEntry to a partitions.conf line in OpenSM format.
fn format_partition_line_simple(entry: &PartitionEntry) -> String {
    let ipoib_flag = if entry.ipoib { ", ipoib" } else { "" };
    let members_str = if entry.members.is_empty() {
        "ALL=full".to_string()
    } else {
        entry.members.join(", ")
    };
    format!("{}{} : {} ;", entry.pkey, ipoib_flag, members_str)
}

// ---------------------------------------------------------------------------
// Standard helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Module implementation
// ---------------------------------------------------------------------------

pub struct IbPartitionModule;

impl Module for IbPartitionModule {
    fn name(&self) -> &'static str {
        "ib_partition"
    }

    fn description(&self) -> &'static str {
        "Manage InfiniBand partition keys via partitions.conf"
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

        let pkey = params.get_string_required("pkey")?;
        let members = params.get_vec_string("members")?.unwrap_or_default();
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let ipoib = params.get_bool_or("ipoib", false);

        let partitions_conf = "/etc/opensm/partitions.conf";

        // Ensure partitions.conf exists
        let (conf_exists, _, _) =
            run_cmd(connection, &format!("test -f {}", partitions_conf), context)?;

        if !conf_exists && !context.check_mode {
            run_cmd_ok(connection, "mkdir -p /etc/opensm", context)?;
            run_cmd_ok(connection, &format!("touch {}", partitions_conf), context)?;
        }

        let (_, current_conf, _) = run_cmd(
            connection,
            &format!("cat {} 2>/dev/null || echo ''", partitions_conf),
            context,
        )?;

        // Parse the full config for structured comparison.
        let parsed_entries = parse_partitions_conf(&current_conf);

        // Run pkey conflict preflight on current config.
        let preflight = detect_pkey_conflicts(&parsed_entries);
        let preflight_warnings = preflight.warnings.clone();

        // Build the desired entry.
        let desired_members = if members.is_empty() {
            vec!["ALL=full".to_string()]
        } else {
            members.clone()
        };
        let desired_entry = PartitionEntry {
            pkey: pkey.clone(),
            ipoib,
            members: desired_members,
        };

        // Find existing entry for this pkey (parsed comparison instead of grep).
        let existing_entry = parsed_entries.iter().find(|e| e.pkey == pkey);

        if state == "absent" {
            if existing_entry.is_none() {
                return Ok(
                    ModuleOutput::ok(format!("Partition key {} not present", pkey))
                        .with_data("pkey", serde_json::json!(pkey))
                        .with_data("diagnostics", serde_json::json!({
                            "preflight_warnings": preflight_warnings,
                        })),
                );
            }

            if context.check_mode {
                return Ok(
                    ModuleOutput::changed(format!("Would remove partition key {}", pkey))
                        .with_data("pkey", serde_json::json!(pkey))
                        .with_data("diagnostics", serde_json::json!({
                            "preflight_warnings": preflight_warnings,
                        })),
                );
            }

            // Remove the partition line for this pkey.
            let new_entries: Vec<&PartitionEntry> =
                parsed_entries.iter().filter(|e| e.pkey != pkey).collect();
            let new_conf: String = new_entries
                .iter()
                .map(|e| format_partition_line_simple(e))
                .collect::<Vec<_>>()
                .join("\n");
            let new_conf = if new_conf.is_empty() {
                String::new()
            } else {
                format!("{}\n", new_conf)
            };
            let escaped = new_conf.replace('\'', "'\\''");
            run_cmd_ok(
                connection,
                &format!("printf '%s' '{}' > {}", escaped, partitions_conf),
                context,
            )?;

            return Ok(
                ModuleOutput::changed(format!("Removed partition key {}", pkey))
                    .with_data("pkey", serde_json::json!(pkey))
                    .with_data("diagnostics", serde_json::json!({
                        "preflight_warnings": preflight_warnings,
                    })),
            );
        }

        // state == "present"

        // Check for pkey conflicts with the desired entry included.
        let mut all_entries_for_check = parsed_entries.clone();
        if existing_entry.is_none() {
            all_entries_for_check.push(desired_entry.clone());
        } else {
            // Replace the existing entry with the desired one for conflict checking.
            for entry in &mut all_entries_for_check {
                if entry.pkey == pkey {
                    *entry = desired_entry.clone();
                }
            }
        }
        let full_preflight = detect_pkey_conflicts(&all_entries_for_check);
        if !full_preflight.passed {
            return Err(ModuleError::ExecutionFailed(format!(
                "PKey conflict preflight failed: {}",
                full_preflight.errors.join("; ")
            )));
        }

        if let Some(current_entry) = existing_entry {
            // Entry exists -- check for drift using parsed comparison.
            let drift = reconcile_members(current_entry, &desired_entry);

            if drift.is_empty() {
                return Ok(
                    ModuleOutput::ok(format!("Partition key {} already configured", pkey))
                        .with_data("pkey", serde_json::json!(pkey))
                        .with_data("diagnostics", serde_json::json!({
                            "preflight_warnings": full_preflight.warnings,
                            "drift": serde_json::json!([]),
                        })),
                );
            }

            // There is drift -- update the entry.
            if context.check_mode {
                return Ok(
                    ModuleOutput::changed(format!(
                        "Would update partition key {} ({} change(s))",
                        pkey,
                        drift.len()
                    ))
                    .with_data("pkey", serde_json::json!(pkey))
                    .with_data("diagnostics", serde_json::json!({
                        "preflight_warnings": full_preflight.warnings,
                        "drift": drift,
                    })),
                );
            }

            // Rewrite the config with the desired entry replacing the current one.
            let new_entries: Vec<PartitionEntry> = parsed_entries
                .iter()
                .map(|e| {
                    if e.pkey == pkey {
                        desired_entry.clone()
                    } else {
                        e.clone()
                    }
                })
                .collect();
            let new_conf: String = new_entries
                .iter()
                .map(format_partition_line_simple)
                .collect::<Vec<_>>()
                .join("\n");
            let new_conf = format!("{}\n", new_conf);
            let escaped = new_conf.replace('\'', "'\\''");
            run_cmd_ok(
                connection,
                &format!("printf '%s' '{}' > {}", escaped, partitions_conf),
                context,
            )?;

            return Ok(
                ModuleOutput::changed(format!(
                    "Updated partition key {} ({} change(s))",
                    pkey,
                    drift.len()
                ))
                .with_data("pkey", serde_json::json!(pkey))
                .with_data("members", serde_json::json!(members))
                .with_data("diagnostics", serde_json::json!({
                    "preflight_warnings": full_preflight.warnings,
                    "drift": drift,
                })),
            );
        }

        // Entry does not exist -- add it.
        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would add partition key {}", pkey))
                    .with_data("pkey", serde_json::json!(pkey))
                    .with_data("diagnostics", serde_json::json!({
                        "preflight_warnings": full_preflight.warnings,
                    })),
            );
        }

        let new_line = format_partition_line_simple(&desired_entry);
        let escaped = new_line.replace('\'', "'\\''");
        run_cmd_ok(
            connection,
            &format!("echo '{}' >> {}", escaped, partitions_conf),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Added partition key {}", pkey))
                .with_data("pkey", serde_json::json!(pkey))
                .with_data("members", serde_json::json!(members))
                .with_data("diagnostics", serde_json::json!({
                    "preflight_warnings": full_preflight.warnings,
                })),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &["pkey"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("members", serde_json::json!([]));
        m.insert("state", serde_json::json!("present"));
        m.insert("ipoib", serde_json::json!(false));
        m
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = IbPartitionModule;
        assert_eq!(module.name(), "ib_partition");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = IbPartitionModule;
        let required = module.required_params();
        assert!(required.contains(&"pkey"));
    }

    #[test]
    fn test_optional_params() {
        let module = IbPartitionModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("members"));
        assert!(optional.contains_key("state"));
        assert!(optional.contains_key("ipoib"));
    }

    #[test]
    fn test_partition_line_format() {
        let pkey = "0x7fff";
        let members = ["node1".to_string(), "node2".to_string()];
        let members_str = members.join(",");
        let line = format!("{}={}\n", pkey, members_str);
        assert!(line.contains("0x7fff"));
        assert!(line.contains("node1,node2"));
    }

    // -------------------------------------------------------------------
    // Parsing tests
    // -------------------------------------------------------------------

    #[test]
    fn test_partitions_conf_parsing() {
        // Test OpenSM format
        let conf = "\
Default=0x7fff, ipoib : ALL=full ;
MyPartition=0x8001, ipoib : guid1=full, guid2=limited ;
NoIpoib=0x8002 : node1=full ;
";
        let entries = parse_partitions_conf(conf);
        assert_eq!(entries.len(), 3);

        // First entry: Default partition
        assert_eq!(entries[0].pkey, "0x7fff");
        assert!(entries[0].ipoib);
        assert_eq!(entries[0].members, vec!["ALL=full"]);

        // Second entry: MyPartition
        assert_eq!(entries[1].pkey, "0x8001");
        assert!(entries[1].ipoib);
        assert_eq!(entries[1].members, vec!["guid1=full", "guid2=limited"]);

        // Third entry: NoIpoib partition
        assert_eq!(entries[2].pkey, "0x8002");
        assert!(!entries[2].ipoib);
        assert_eq!(entries[2].members, vec!["node1=full"]);
    }

    #[test]
    fn test_partitions_conf_parsing_simple_format() {
        // Test the simple format from older code
        let conf = "\
0x7fff,ipoib=ALL=full
0x8001=guid1=full,guid2=limited
";
        let entries = parse_partitions_conf(conf);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].pkey, "0x7fff");
        assert!(entries[0].ipoib);
        assert_eq!(entries[0].members, vec!["ALL=full"]);

        assert_eq!(entries[1].pkey, "0x8001");
        assert!(!entries[1].ipoib);
        assert_eq!(entries[1].members, vec!["guid1=full", "guid2=limited"]);
    }

    #[test]
    fn test_partitions_conf_parsing_comments_and_blanks() {
        let conf = "\
# This is a comment
Default=0x7fff : ALL=full ;

# Another comment
MyPartition=0x8001, ipoib : guid1=full ;
";
        let entries = parse_partitions_conf(conf);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].pkey, "0x7fff");
        assert_eq!(entries[1].pkey, "0x8001");
    }

    #[test]
    fn test_partitions_conf_parsing_empty() {
        let entries = parse_partitions_conf("");
        assert!(entries.is_empty());

        let entries = parse_partitions_conf("# only comments\n# here\n");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_partitions_conf_parsing_no_members() {
        // A partition header with no members section
        let conf = "0x7fff, ipoib : ;";
        let entries = parse_partitions_conf(conf);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pkey, "0x7fff");
        assert!(entries[0].ipoib);
        assert!(entries[0].members.is_empty());
    }

    // -------------------------------------------------------------------
    // Conflict detection tests
    // -------------------------------------------------------------------

    #[test]
    fn test_pkey_conflict_detection_no_conflicts() {
        let entries = vec![
            PartitionEntry {
                pkey: "0x7fff".to_string(),
                ipoib: true,
                members: vec!["ALL=full".to_string()],
            },
            PartitionEntry {
                pkey: "0x8001".to_string(),
                ipoib: false,
                members: vec!["guid1=full".to_string()],
            },
        ];
        let result = detect_pkey_conflicts(&entries);
        assert!(result.passed);
        assert!(result.errors.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_pkey_conflict_detection_duplicate_pkeys() {
        let entries = vec![
            PartitionEntry {
                pkey: "0x7fff".to_string(),
                ipoib: true,
                members: vec!["ALL=full".to_string()],
            },
            PartitionEntry {
                pkey: "0x7fff".to_string(),
                ipoib: false,
                members: vec!["guid1=full".to_string()],
            },
        ];
        let result = detect_pkey_conflicts(&entries);
        assert!(!result.passed);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("Duplicate pkey 0x7fff"));
    }

    #[test]
    fn test_pkey_conflict_detection_conflicting_members() {
        let entries = vec![
            PartitionEntry {
                pkey: "0x8001".to_string(),
                ipoib: false,
                members: vec!["guid1=full".to_string()],
            },
            PartitionEntry {
                pkey: "0x8002".to_string(),
                ipoib: false,
                members: vec!["guid1=limited".to_string()],
            },
        ];
        let result = detect_pkey_conflicts(&entries);
        // Conflicting member access levels are warnings, not errors
        assert!(result.passed);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("guid1"));
        assert!(result.warnings[0].contains("different access levels"));
    }

    #[test]
    fn test_pkey_conflict_detection_same_member_same_access() {
        // Same member with same access level across partitions is fine
        let entries = vec![
            PartitionEntry {
                pkey: "0x8001".to_string(),
                ipoib: false,
                members: vec!["guid1=full".to_string()],
            },
            PartitionEntry {
                pkey: "0x8002".to_string(),
                ipoib: false,
                members: vec!["guid1=full".to_string()],
            },
        ];
        let result = detect_pkey_conflicts(&entries);
        assert!(result.passed);
        assert!(result.warnings.is_empty());
    }

    // -------------------------------------------------------------------
    // Member reconciliation tests
    // -------------------------------------------------------------------

    #[test]
    fn test_member_reconciliation_no_changes() {
        let current = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: true,
            members: vec!["guid1=full".to_string(), "guid2=limited".to_string()],
        };
        let desired = current.clone();
        let drift = reconcile_members(&current, &desired);
        assert!(drift.is_empty());
    }

    #[test]
    fn test_member_reconciliation_add_member() {
        let current = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: false,
            members: vec!["guid1=full".to_string()],
        };
        let desired = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: false,
            members: vec!["guid1=full".to_string(), "guid2=limited".to_string()],
        };
        let drift = reconcile_members(&current, &desired);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].field, "member_add:guid2");
        assert_eq!(drift[0].desired, "guid2=limited");
        assert_eq!(drift[0].actual, "absent");
    }

    #[test]
    fn test_member_reconciliation_remove_member() {
        let current = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: false,
            members: vec!["guid1=full".to_string(), "guid2=limited".to_string()],
        };
        let desired = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: false,
            members: vec!["guid1=full".to_string()],
        };
        let drift = reconcile_members(&current, &desired);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].field, "member_remove:guid2");
        assert_eq!(drift[0].actual, "guid2=limited");
        assert_eq!(drift[0].desired, "absent");
    }

    #[test]
    fn test_member_reconciliation_change_access() {
        let current = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: false,
            members: vec!["guid1=full".to_string()],
        };
        let desired = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: false,
            members: vec!["guid1=limited".to_string()],
        };
        let drift = reconcile_members(&current, &desired);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].field, "member_access:guid1");
        assert_eq!(drift[0].desired, "guid1=limited");
        assert_eq!(drift[0].actual, "guid1=full");
    }

    #[test]
    fn test_member_reconciliation_ipoib_change() {
        let current = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: false,
            members: vec!["guid1=full".to_string()],
        };
        let desired = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: true,
            members: vec!["guid1=full".to_string()],
        };
        let drift = reconcile_members(&current, &desired);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].field, "ipoib");
        assert_eq!(drift[0].desired, "true");
        assert_eq!(drift[0].actual, "false");
    }

    #[test]
    fn test_member_reconciliation_multiple_changes() {
        let current = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: false,
            members: vec![
                "guid1=full".to_string(),
                "guid2=limited".to_string(),
            ],
        };
        let desired = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: true,
            members: vec![
                "guid1=limited".to_string(),
                "guid3=full".to_string(),
            ],
        };
        let drift = reconcile_members(&current, &desired);
        // ipoib change + guid1 access change + guid3 add + guid2 remove = 4
        assert_eq!(drift.len(), 4);

        let fields: Vec<&str> = drift.iter().map(|d| d.field.as_str()).collect();
        assert!(fields.contains(&"ipoib"));
        assert!(fields.contains(&"member_access:guid1"));
        assert!(fields.contains(&"member_add:guid3"));
        assert!(fields.contains(&"member_remove:guid2"));
    }

    // -------------------------------------------------------------------
    // Helper function tests
    // -------------------------------------------------------------------

    #[test]
    fn test_split_member_access() {
        assert_eq!(split_member_access("guid1=full"), ("guid1", "full"));
        assert_eq!(split_member_access("guid2=limited"), ("guid2", "limited"));
        assert_eq!(split_member_access("ALL=full"), ("ALL", "full"));
        // No access level specified defaults to "full"
        assert_eq!(split_member_access("guid1"), ("guid1", "full"));
    }

    #[test]
    fn test_format_partition_line_simple() {
        let entry = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: true,
            members: vec!["guid1=full".to_string(), "guid2=limited".to_string()],
        };
        let line = format_partition_line_simple(&entry);
        assert_eq!(line, "0x8001, ipoib : guid1=full, guid2=limited ;");
    }

    #[test]
    fn test_format_partition_line_simple_no_ipoib() {
        let entry = PartitionEntry {
            pkey: "0x7fff".to_string(),
            ipoib: false,
            members: vec!["ALL=full".to_string()],
        };
        let line = format_partition_line_simple(&entry);
        assert_eq!(line, "0x7fff : ALL=full ;");
    }

    #[test]
    fn test_format_partition_line_simple_no_members() {
        let entry = PartitionEntry {
            pkey: "0x7fff".to_string(),
            ipoib: false,
            members: vec![],
        };
        let line = format_partition_line_simple(&entry);
        assert_eq!(line, "0x7fff : ALL=full ;");
    }

    #[test]
    fn test_roundtrip_parse_format() {
        // Write a config, parse it, reformat it, and parse again.
        let original = PartitionEntry {
            pkey: "0x8001".to_string(),
            ipoib: true,
            members: vec!["guid1=full".to_string(), "guid2=limited".to_string()],
        };
        let line = format_partition_line_simple(&original);
        let parsed = parse_partitions_conf(&line);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].pkey, original.pkey);
        assert_eq!(parsed[0].ipoib, original.ipoib);
        assert_eq!(parsed[0].members, original.members);
    }
}
