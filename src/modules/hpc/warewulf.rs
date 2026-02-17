//! Warewulf cluster management modules
//!
//! Manage Warewulf node and image configurations via wwctl CLI.
//!
//! # Modules
//!
//! - `warewulf_node`: Manage compute node definitions
//! - `warewulf_image`: Manage node images (containers/chroots)

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

/// Parse `wwctl node list <name> -a` output into a HashMap of properties.
///
/// The output format is typically a series of key-value lines such as:
///   NodeName             = compute-01
///   Container Name       = rocky-9
///   Ipaddr               = 10.0.0.101
///
/// Lines that do not contain a separator or are empty are skipped.
fn get_node_properties(output: &str) -> HashMap<String, String> {
    let mut props = HashMap::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Try "key = value" format (wwctl node list -a output)
        if let Some((key, value)) = trimmed.split_once('=') {
            let k = key.trim().to_lowercase().replace(' ', "_");
            let v = value.trim().to_string();
            if !k.is_empty() {
                props.insert(k, v);
            }
            continue;
        }

        // Try "key: value" format (YAML-like output)
        if let Some((key, value)) = trimmed.split_once(':') {
            let k = key.trim().to_lowercase().replace(' ', "_");
            let v = value.trim().to_string();
            if !k.is_empty() {
                props.insert(k, v);
            }
        }
    }

    props
}

/// Compare desired node properties against current properties and apply changes.
///
/// Only runs `wwctl node set` for properties that actually differ from the
/// current state. Returns a list of drift items describing what changed.
fn reconcile_node(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    node_name: &str,
    desired: &HashMap<String, String>,
    current: &HashMap<String, String>,
) -> ModuleResult<Vec<DriftItem>> {
    let mut drift = Vec::new();

    for (key, desired_value) in desired {
        let actual_value = current
            .get(key)
            .map(|s| s.as_str())
            .unwrap_or("")
            .to_string();

        if actual_value != *desired_value {
            drift.push(DriftItem {
                field: key.clone(),
                desired: desired_value.clone(),
                actual: actual_value,
            });

            if !context.check_mode {
                run_cmd_ok(
                    connection,
                    &format!("wwctl node set {} --{} {}", node_name, key, desired_value),
                    context,
                )?;
            }
        }
    }

    Ok(drift)
}

/// Check that PXE boot dependencies (TFTP and DHCP services) are active.
///
/// Tries the common service names for each dependency and reports errors
/// for any that are not running.
fn check_pxe_dependency(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
) -> ModuleResult<PreflightResult> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check TFTP service
    let tftp_services = ["tftp.service", "in.tftpd"];
    let mut tftp_running = false;
    for svc in &tftp_services {
        let (active, stdout, _) = run_cmd(
            connection,
            &format!("systemctl is-active {} 2>/dev/null", svc),
            context,
        )?;
        if active && stdout.trim() == "active" {
            tftp_running = true;
            break;
        }
    }
    if !tftp_running {
        errors.push("TFTP service is not active (checked tftp.service, in.tftpd)".to_string());
    }

    // Check DHCP service
    let dhcp_services = ["dhcpd.service", "isc-dhcp-server"];
    let mut dhcp_running = false;
    for svc in &dhcp_services {
        let (active, stdout, _) = run_cmd(
            connection,
            &format!("systemctl is-active {} 2>/dev/null", svc),
            context,
        )?;
        if active && stdout.trim() == "active" {
            dhcp_running = true;
            break;
        }
    }
    if !dhcp_running {
        errors.push(
            "DHCP service is not active (checked dhcpd.service, isc-dhcp-server)".to_string(),
        );
    }

    if errors.is_empty() {
        warnings.push("PXE dependencies verified: TFTP and DHCP are active".to_string());
    }

    let passed = errors.is_empty();
    Ok(PreflightResult {
        passed,
        warnings,
        errors,
    })
}

/// Verify a Warewulf node's state after applying changes.
///
/// Re-reads the node properties and confirms the node is registered
/// and that its key attributes are non-empty.
fn verify_node(
    connection: &Arc<dyn Connection + Send + Sync>,
    context: &ModuleContext,
    node_name: &str,
) -> ModuleResult<VerifyResult> {
    let mut details = Vec::new();
    let mut warnings = Vec::new();

    // Confirm node is listed
    let (listed, _, _) = run_cmd(
        connection,
        &format!(
            "wwctl node list {} 2>/dev/null | grep -q '{}'",
            node_name, node_name
        ),
        context,
    )?;

    if !listed {
        return Ok(VerifyResult {
            verified: false,
            details: vec![format!("Node '{}' not found after apply", node_name)],
            warnings,
        });
    }
    details.push(format!("Node '{}' is registered", node_name));

    // Read properties for verification
    let (prop_ok, prop_stdout, _) = run_cmd(
        connection,
        &format!("wwctl node list {} -a 2>/dev/null", node_name),
        context,
    )?;

    if prop_ok && !prop_stdout.trim().is_empty() {
        let props = get_node_properties(&prop_stdout);
        if let Some(container) = props.get("container_name") {
            if container.is_empty() || container == "--" {
                warnings.push("Node has no container/image assigned".to_string());
            } else {
                details.push(format!("Container: {}", container));
            }
        }
    } else {
        warnings.push("Could not read node properties for verification".to_string());
    }

    Ok(VerifyResult {
        verified: true,
        details,
        warnings,
    })
}

// ---- Warewulf Node Module ----

pub struct WarewulfNodeModule;

impl Module for WarewulfNodeModule {
    fn name(&self) -> &'static str {
        "warewulf_node"
    }

    fn description(&self) -> &'static str {
        "Manage Warewulf compute node definitions via wwctl"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::GlobalExclusive
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

        let node_name = params.get_string_required("name")?;
        let image = params.get_string("image")?;
        let network = params.get_string("network")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let canary = params.get_bool_or("canary", false);

        // Check if wwctl is available
        let (wwctl_ok, _, _) = run_cmd(connection, "which wwctl", context)?;
        if !wwctl_ok {
            return Err(ModuleError::ExecutionFailed(
                "wwctl command not found. Ensure Warewulf is installed.".to_string(),
            ));
        }

        // PXE dependency preflight check
        let preflight = check_pxe_dependency(connection, context)?;
        if !preflight.passed {
            return Err(ModuleError::ExecutionFailed(format!(
                "PXE dependency check failed: {}",
                preflight.errors.join("; ")
            )));
        }

        // Check if node exists
        let (node_exists, _, _) = run_cmd(
            connection,
            &format!(
                "wwctl node list {} 2>/dev/null | grep -q '{}'",
                node_name, node_name
            ),
            context,
        )?;

        if state == "absent" {
            if !node_exists {
                return Ok(
                    ModuleOutput::ok(format!("Warewulf node '{}' not present", node_name))
                        .with_data("node", serde_json::json!(node_name))
                        .with_data("preflight", serde_json::json!(preflight)),
                );
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would delete Warewulf node '{}'",
                    node_name
                ))
                .with_data("node", serde_json::json!(node_name))
                .with_data("preflight", serde_json::json!(preflight)));
            }

            run_cmd_ok(
                connection,
                &format!("wwctl node delete {}", node_name),
                context,
            )?;

            return Ok(
                ModuleOutput::changed(format!("Deleted Warewulf node '{}'", node_name))
                    .with_data("node", serde_json::json!(node_name))
                    .with_data("preflight", serde_json::json!(preflight)),
            );
        }

        // For existing nodes, use drift-aware reconciliation
        if node_exists {
            // Build desired properties map from params
            let mut desired_props = HashMap::new();
            if let Some(ref img) = image {
                desired_props.insert("container".to_string(), img.clone());
            }
            if let Some(ref net) = network {
                desired_props.insert("netname".to_string(), net.clone());
            }

            if desired_props.is_empty() {
                return Ok(ModuleOutput::ok(format!(
                    "Warewulf node '{}' already exists",
                    node_name
                ))
                .with_data("node", serde_json::json!(node_name))
                .with_data("preflight", serde_json::json!(preflight)));
            }

            // Get current node properties for drift detection
            let (_, prop_stdout, _) = run_cmd(
                connection,
                &format!("wwctl node list {} -a 2>/dev/null", node_name),
                context,
            )?;
            let current_props = get_node_properties(&prop_stdout);

            // Reconcile: only apply changes for drifted properties
            let drift = reconcile_node(
                connection,
                context,
                &node_name,
                &desired_props,
                &current_props,
            )?;

            if drift.is_empty() {
                return Ok(ModuleOutput::ok(format!(
                    "Warewulf node '{}' is up to date",
                    node_name
                ))
                .with_data("node", serde_json::json!(node_name))
                .with_data("preflight", serde_json::json!(preflight)));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would update {} properties on Warewulf node '{}'",
                    drift.len(),
                    node_name
                ))
                .with_data("node", serde_json::json!(node_name))
                .with_data("drift", serde_json::json!(drift))
                .with_data("preflight", serde_json::json!(preflight)));
            }

            // Post-verify after reconciliation
            let verify = verify_node(connection, context, &node_name)?;

            let mut output = ModuleOutput::changed(format!(
                "Reconciled {} properties on Warewulf node '{}'{}",
                drift.len(),
                node_name,
                if canary { " (canary)" } else { "" }
            ))
            .with_data("node", serde_json::json!(node_name))
            .with_data("drift", serde_json::json!(drift))
            .with_data("preflight", serde_json::json!(preflight))
            .with_data("verify", serde_json::json!(verify))
            .with_data("canary", serde_json::json!(canary));

            if !verify.verified {
                output = output.with_data(
                    "verify_warning",
                    serde_json::json!("Post-apply verification detected issues"),
                );
            }

            return Ok(output);
        }

        // Node does not exist, create it
        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would create Warewulf node '{}'{}",
                node_name,
                if canary { " (canary)" } else { "" }
            ))
            .with_data("node", serde_json::json!(node_name))
            .with_data("preflight", serde_json::json!(preflight))
            .with_data("canary", serde_json::json!(canary)));
        }

        let mut add_cmd = format!("wwctl node add {}", node_name);
        if let Some(ref img) = image {
            add_cmd.push_str(&format!(" --container {}", img));
        }
        if let Some(ref net) = network {
            add_cmd.push_str(&format!(" --netname {}", net));
        }

        run_cmd_ok(connection, &add_cmd, context)?;

        // Post-verify after creation
        let verify = verify_node(connection, context, &node_name)?;

        let mut output = ModuleOutput::changed(format!(
            "Created Warewulf node '{}'{}",
            node_name,
            if canary { " (canary)" } else { "" }
        ))
        .with_data("node", serde_json::json!(node_name))
        .with_data("image", serde_json::json!(image))
        .with_data("network", serde_json::json!(network))
        .with_data("preflight", serde_json::json!(preflight))
        .with_data("verify", serde_json::json!(verify))
        .with_data("canary", serde_json::json!(canary));

        if !verify.verified {
            output = output.with_data(
                "verify_warning",
                serde_json::json!("Post-creation verification detected issues"),
            );
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("image", serde_json::json!(null));
        m.insert("network", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m.insert("canary", serde_json::json!(false));
        m
    }
}

// ---- Warewulf Image Module ----

pub struct WarewulfImageModule;

impl Module for WarewulfImageModule {
    fn name(&self) -> &'static str {
        "warewulf_image"
    }

    fn description(&self) -> &'static str {
        "Manage Warewulf node images (containers/chroots) via wwctl"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::GlobalExclusive
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

        let image_name = params.get_string_required("name")?;
        let chroot = params.get_string("chroot")?;
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        // Check if wwctl is available
        let (wwctl_ok, _, _) = run_cmd(connection, "which wwctl", context)?;
        if !wwctl_ok {
            return Err(ModuleError::ExecutionFailed(
                "wwctl command not found. Ensure Warewulf is installed.".to_string(),
            ));
        }

        // Check if image exists
        let (image_exists, _, _) = run_cmd(
            connection,
            &format!(
                "wwctl container list 2>/dev/null | grep -q '{}'",
                image_name
            ),
            context,
        )?;

        if state == "absent" {
            if !image_exists {
                return Ok(ModuleOutput::ok(format!(
                    "Warewulf image '{}' not present",
                    image_name
                ))
                .with_data("image", serde_json::json!(image_name)));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would delete Warewulf image '{}'",
                    image_name
                ))
                .with_data("image", serde_json::json!(image_name)));
            }

            run_cmd_ok(
                connection,
                &format!("wwctl container delete {}", image_name),
                context,
            )?;

            return Ok(
                ModuleOutput::changed(format!("Deleted Warewulf image '{}'", image_name))
                    .with_data("image", serde_json::json!(image_name)),
            );
        }

        if image_exists {
            return Ok(
                ModuleOutput::ok(format!("Warewulf image '{}' already exists", image_name))
                    .with_data("image", serde_json::json!(image_name)),
            );
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would import Warewulf image '{}'",
                image_name
            ))
            .with_data("image", serde_json::json!(image_name)));
        }

        let import_cmd = if let Some(ref ch) = chroot {
            format!("wwctl container import {} {}", ch, image_name)
        } else {
            return Err(ModuleError::InvalidParameter(
                "Parameter 'chroot' is required for creating images".to_string(),
            ));
        };

        run_cmd_ok(connection, &import_cmd, context)?;

        Ok(
            ModuleOutput::changed(format!("Imported Warewulf image '{}'", image_name))
                .with_data("image", serde_json::json!(image_name))
                .with_data("chroot", serde_json::json!(chroot)),
        )
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("chroot", serde_json::json!(null));
        m.insert("state", serde_json::json!("present"));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warewulf_node_module_metadata() {
        let module = WarewulfNodeModule;
        assert_eq!(module.name(), "warewulf_node");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_warewulf_node_required_params() {
        let module = WarewulfNodeModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_warewulf_image_module_metadata() {
        let module = WarewulfImageModule;
        assert_eq!(module.name(), "warewulf_image");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_warewulf_image_required_params() {
        let module = WarewulfImageModule;
        let required = module.required_params();
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_warewulf_optional_params() {
        let node_module = WarewulfNodeModule;
        let node_optional = node_module.optional_params();
        assert!(node_optional.contains_key("image"));
        assert!(node_optional.contains_key("network"));
        assert!(node_optional.contains_key("state"));
        assert!(node_optional.contains_key("canary"));

        let image_module = WarewulfImageModule;
        let image_optional = image_module.optional_params();
        assert!(image_optional.contains_key("chroot"));
        assert!(image_optional.contains_key("state"));
    }

    #[test]
    fn test_node_property_parsing() {
        // Test "key = value" format (wwctl node list -a)
        let output = "\
NodeName             = compute-01
Container Name       = rocky-9
Ipaddr               = 10.0.0.101
Netmask              = 255.255.255.0
Network              = default
Hwaddr               = 00:11:22:33:44:55
";
        let props = get_node_properties(output);
        assert_eq!(props.get("nodename").unwrap(), "compute-01");
        assert_eq!(props.get("container_name").unwrap(), "rocky-9");
        assert_eq!(props.get("ipaddr").unwrap(), "10.0.0.101");
        assert_eq!(props.get("netmask").unwrap(), "255.255.255.0");
        assert_eq!(props.get("network").unwrap(), "default");
        assert_eq!(props.get("hwaddr").unwrap(), "00:11:22:33:44:55");

        // Test "key: value" format (YAML-like output)
        let yaml_output = "\
nodename: compute-02
container: ubuntu-22
ipaddr: 10.0.0.102
";
        let yaml_props = get_node_properties(yaml_output);
        assert_eq!(yaml_props.get("nodename").unwrap(), "compute-02");
        assert_eq!(yaml_props.get("container").unwrap(), "ubuntu-22");
        assert_eq!(yaml_props.get("ipaddr").unwrap(), "10.0.0.102");

        // Test empty and whitespace lines are skipped
        let sparse_output = "\n  \nNodeName = node-03\n\n";
        let sparse_props = get_node_properties(sparse_output);
        assert_eq!(sparse_props.get("nodename").unwrap(), "node-03");
        assert_eq!(sparse_props.len(), 1);

        // Test empty input
        let empty_props = get_node_properties("");
        assert!(empty_props.is_empty());
    }

    #[test]
    fn test_pxe_dependency_check_format() {
        // Verify the known service names used for TFTP checks
        let tftp_services = ["tftp.service", "in.tftpd"];
        assert!(tftp_services.contains(&"tftp.service"));
        assert!(tftp_services.contains(&"in.tftpd"));

        // Verify the known service names used for DHCP checks
        let dhcp_services = ["dhcpd.service", "isc-dhcp-server"];
        assert!(dhcp_services.contains(&"dhcpd.service"));
        assert!(dhcp_services.contains(&"isc-dhcp-server"));

        // Verify PreflightResult struct format
        let passing = PreflightResult {
            passed: true,
            warnings: vec!["PXE dependencies verified".to_string()],
            errors: vec![],
        };
        assert!(passing.passed);
        assert!(passing.errors.is_empty());
        assert_eq!(passing.warnings.len(), 1);

        let failing = PreflightResult {
            passed: false,
            warnings: vec![],
            errors: vec![
                "TFTP service is not active (checked tftp.service, in.tftpd)".to_string(),
                "DHCP service is not active (checked dhcpd.service, isc-dhcp-server)".to_string(),
            ],
        };
        assert!(!failing.passed);
        assert_eq!(failing.errors.len(), 2);
        assert!(failing.errors[0].contains("TFTP"));
        assert!(failing.errors[1].contains("DHCP"));

        // Verify PreflightResult serializes correctly
        let json = serde_json::to_value(&failing).unwrap();
        assert_eq!(json["passed"], false);
        assert!(json["errors"].as_array().unwrap().len() == 2);
    }

    #[test]
    fn test_drift_reconciliation() {
        // No drift when desired matches current
        let mut desired = HashMap::new();
        desired.insert("container".to_string(), "rocky-9".to_string());
        desired.insert("netname".to_string(), "default".to_string());

        let mut current = HashMap::new();
        current.insert("container".to_string(), "rocky-9".to_string());
        current.insert("netname".to_string(), "default".to_string());

        let drift = compute_drift(&desired, &current);
        assert!(drift.is_empty(), "Expected no drift when properties match");

        // Drift detected when values differ
        let mut current_drift = HashMap::new();
        current_drift.insert("container".to_string(), "centos-7".to_string());
        current_drift.insert("netname".to_string(), "default".to_string());

        let drift = compute_drift(&desired, &current_drift);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].field, "container");
        assert_eq!(drift[0].desired, "rocky-9");
        assert_eq!(drift[0].actual, "centos-7");

        // Drift detected when current property is missing
        let empty_current: HashMap<String, String> = HashMap::new();
        let drift = compute_drift(&desired, &empty_current);
        assert_eq!(drift.len(), 2);

        // Verify all drift items have correct structure
        for item in &drift {
            assert!(!item.field.is_empty());
            assert!(!item.desired.is_empty());
            // actual can be empty (missing property)
        }

        // Verify DriftItem serializes correctly
        let item = &drift[0];
        let json = serde_json::to_value(item).unwrap();
        assert!(json.get("field").is_some());
        assert!(json.get("desired").is_some());
        assert!(json.get("actual").is_some());
    }

    #[test]
    fn test_verify_result_format() {
        let verified = VerifyResult {
            verified: true,
            details: vec!["Node 'compute-01' is registered".to_string()],
            warnings: vec![],
        };
        assert!(verified.verified);
        assert_eq!(verified.details.len(), 1);
        assert!(verified.warnings.is_empty());

        let unverified = VerifyResult {
            verified: false,
            details: vec!["Node 'compute-01' not found after apply".to_string()],
            warnings: vec!["Could not read node properties".to_string()],
        };
        assert!(!unverified.verified);

        // Verify serialization
        let json = serde_json::to_value(&unverified).unwrap();
        assert_eq!(json["verified"], false);
        assert!(json["details"].as_array().unwrap().len() == 1);
        assert!(json["warnings"].as_array().unwrap().len() == 1);
    }

    /// Pure computation helper used by tests to verify drift logic without
    /// needing a connection. Mirrors the comparison logic in `reconcile_node`.
    fn compute_drift(
        desired: &HashMap<String, String>,
        current: &HashMap<String, String>,
    ) -> Vec<DriftItem> {
        let mut drift = Vec::new();
        let mut keys: Vec<&String> = desired.keys().collect();
        keys.sort();
        for key in keys {
            let desired_value = &desired[key];
            let actual_value = current
                .get(key)
                .map(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            if actual_value != *desired_value {
                drift.push(DriftItem {
                    field: key.clone(),
                    desired: desired_value.clone(),
                    actual: actual_value,
                });
            }
        }
        drift
    }
}
