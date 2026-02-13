//! InfiniBand validation module
//!
//! Parses `ibstat` output to validate InfiniBand port state and speed,
//! and optionally generates network topology via `ibnetdiscover`.
//!
//! Feature-gated behind `#[cfg(feature = "ofed")]`.
//!
//! # Parameters
//!
//! - `expected_state` (optional): expected port state (default: "Active")
//! - `expected_speed` (optional): expected link speed (e.g. "HDR" or "100 Gb/sec")
//! - `generate_topology` (optional): bool (default: false) - run ibnetdiscover for topology

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

/// Parsed representation of a single IB port from ibstat output.
#[derive(Debug, Clone)]
struct IbPort {
    device: String,
    port: String,
    state: String,
    physical_state: String,
    rate: String,
    base_lid: String,
    sm_lid: String,
    port_guid: String,
}

/// Parse ibstat output into a list of port structures.
///
/// ibstat output has a format like:
/// ```text
/// CA 'mlx5_0'
///     CA type: MT4123
///     ...
///     Port 1:
///         State: Active
///         Physical state: LinkUp
///         Rate: 100
///         Base lid: 1
///         LMC: 0
///         SM lid: 1
///         Capability mask: ...
///         Port GUID: 0x...
/// ```
fn parse_ibstat(output: &str) -> Vec<IbPort> {
    let mut ports = Vec::new();
    let mut current_device = String::new();
    let mut current_port = String::new();
    let mut state = String::new();
    let mut physical_state = String::new();
    let mut rate = String::new();
    let mut base_lid = String::new();
    let mut sm_lid = String::new();
    let mut port_guid = String::new();
    let mut in_port = false;

    for line in output.lines() {
        let trimmed = line.trim();

        // Match device header: CA 'mlx5_0'
        if trimmed.starts_with("CA '") && trimmed.ends_with('\'') {
            // Save previous port if we were in one
            if in_port && !current_device.is_empty() {
                ports.push(IbPort {
                    device: current_device.clone(),
                    port: current_port.clone(),
                    state: state.clone(),
                    physical_state: physical_state.clone(),
                    rate: rate.clone(),
                    base_lid: base_lid.clone(),
                    sm_lid: sm_lid.clone(),
                    port_guid: port_guid.clone(),
                });
            }
            current_device = trimmed
                .strip_prefix("CA '")
                .unwrap_or("")
                .strip_suffix('\'')
                .unwrap_or("")
                .to_string();
            in_port = false;
            continue;
        }

        // Match port header: Port 1:
        if trimmed.starts_with("Port ") && trimmed.ends_with(':') {
            // Save previous port if we were in one
            if in_port && !current_device.is_empty() {
                ports.push(IbPort {
                    device: current_device.clone(),
                    port: current_port.clone(),
                    state: state.clone(),
                    physical_state: physical_state.clone(),
                    rate: rate.clone(),
                    base_lid: base_lid.clone(),
                    sm_lid: sm_lid.clone(),
                    port_guid: port_guid.clone(),
                });
            }
            current_port = trimmed
                .strip_prefix("Port ")
                .unwrap_or("")
                .strip_suffix(':')
                .unwrap_or("")
                .to_string();
            state.clear();
            physical_state.clear();
            rate.clear();
            base_lid.clear();
            sm_lid.clear();
            port_guid.clear();
            in_port = true;
            continue;
        }

        if !in_port {
            continue;
        }

        // Parse port fields
        if let Some(val) = trimmed.strip_prefix("State:") {
            state = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("Physical state:") {
            physical_state = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("Rate:") {
            rate = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("Base lid:") {
            base_lid = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("SM lid:") {
            sm_lid = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("Port GUID:") {
            port_guid = val.trim().to_string();
        }
    }

    // Save last port
    if in_port && !current_device.is_empty() {
        ports.push(IbPort {
            device: current_device,
            port: current_port,
            state,
            physical_state,
            rate,
            base_lid,
            sm_lid,
            port_guid,
        });
    }

    ports
}

pub struct IbValidateModule;

impl Module for IbValidateModule {
    fn name(&self) -> &'static str {
        "ib_validate"
    }

    fn description(&self) -> &'static str {
        "Validate InfiniBand port state, speed, and optionally generate network topology"
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

        let expected_state = params
            .get_string("expected_state")?
            .unwrap_or_else(|| "Active".to_string());
        let expected_speed = params.get_string("expected_speed")?;
        let generate_topology = params.get_bool_or("generate_topology", false);

        // --- Check mode: report what would be validated ---
        if context.check_mode {
            let mut checks: Vec<String> = Vec::new();
            checks.push(format!(
                "Would validate IB port state = '{}'",
                expected_state
            ));
            if let Some(ref speed) = expected_speed {
                checks.push(format!("Would validate IB link speed contains '{}'", speed));
            }
            if generate_topology {
                checks.push("Would run ibnetdiscover for topology".to_string());
            }
            return Ok(ModuleOutput::ok("Would validate InfiniBand configuration")
                .with_data("checks", serde_json::json!(checks)));
        }

        // --- Verify ibstat is available ---
        let (ibstat_exists, _, _) =
            run_cmd(connection, "command -v ibstat >/dev/null 2>&1", context)?;
        if !ibstat_exists {
            return Err(ModuleError::ExecutionFailed(
                "ibstat not found. Install rdma-core / infiniband-diags first.".to_string(),
            ));
        }

        // --- Run ibstat and parse output ---
        let ibstat_output = run_cmd_ok(connection, "ibstat", context)?;
        let ports = parse_ibstat(&ibstat_output);

        if ports.is_empty() {
            return Ok(ModuleOutput::failed("No InfiniBand devices/ports found")
                .with_data("ibstat_raw", serde_json::json!(ibstat_output)));
        }

        // --- Validate each port ---
        let mut all_pass = true;
        let mut validation_results: Vec<serde_json::Value> = Vec::new();

        for port in &ports {
            let state_ok = port.state == expected_state;
            let speed_ok = if let Some(ref speed) = expected_speed {
                port.rate.contains(speed.as_str())
            } else {
                true
            };

            let pass = state_ok && speed_ok;
            if !pass {
                all_pass = false;
            }

            let mut result = serde_json::json!({
                "device": port.device,
                "port": port.port,
                "state": port.state,
                "physical_state": port.physical_state,
                "rate": port.rate,
                "base_lid": port.base_lid,
                "sm_lid": port.sm_lid,
                "port_guid": port.port_guid,
                "state_ok": state_ok,
                "speed_ok": speed_ok,
                "pass": pass,
            });

            if !state_ok {
                result["state_expected"] = serde_json::json!(expected_state);
            }
            if !speed_ok {
                if let Some(ref speed) = expected_speed {
                    result["speed_expected"] = serde_json::json!(speed);
                }
            }

            validation_results.push(result);
        }

        // --- Topology generation ---
        let topology = if generate_topology {
            let (topo_ok, topo_stdout, topo_stderr) =
                run_cmd(connection, "ibnetdiscover 2>&1", context)?;
            if topo_ok {
                Some(topo_stdout)
            } else {
                // Non-fatal: topology generation is best-effort
                Some(format!("ibnetdiscover failed: {}", topo_stderr.trim()))
            }
        } else {
            None
        };

        // --- Build final output ---
        let msg = if all_pass {
            format!("All {} InfiniBand port(s) passed validation", ports.len())
        } else {
            let fail_count = validation_results
                .iter()
                .filter(|r| r["pass"] == serde_json::json!(false))
                .count();
            format!(
                "{} of {} InfiniBand port(s) failed validation",
                fail_count,
                ports.len()
            )
        };

        let mut output = if all_pass {
            ModuleOutput::ok(&msg)
        } else {
            ModuleOutput::failed(&msg)
        };

        output = output
            .with_data("port_count", serde_json::json!(ports.len()))
            .with_data("all_pass", serde_json::json!(all_pass))
            .with_data("ports", serde_json::json!(validation_results));

        if let Some(topo) = topology {
            output = output.with_data("topology", serde_json::json!(topo));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("expected_state", serde_json::json!("Active"));
        m.insert("expected_speed", serde_json::json!(null));
        m.insert("generate_topology", serde_json::json!(false));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ibstat_single_port() {
        let output = r#"CA 'mlx5_0'
    CA type: MT4123
    Number of ports: 1
    Firmware version: 20.31.1014
    Hardware version: 0
    Node GUID: 0x0c42a103007e9346
    System image GUID: 0x0c42a103007e9348
    Port 1:
        State: Active
        Physical state: LinkUp
        Rate: 100
        Base lid: 12
        LMC: 0
        SM lid: 1
        Capability mask: 0x2651e84a
        Port GUID: 0x0c42a103007e9347
        Link layer: InfiniBand
"#;
        let ports = parse_ibstat(output);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].device, "mlx5_0");
        assert_eq!(ports[0].port, "1");
        assert_eq!(ports[0].state, "Active");
        assert_eq!(ports[0].physical_state, "LinkUp");
        assert_eq!(ports[0].rate, "100");
        assert_eq!(ports[0].base_lid, "12");
        assert_eq!(ports[0].sm_lid, "1");
        assert_eq!(ports[0].port_guid, "0x0c42a103007e9347");
    }

    #[test]
    fn test_parse_ibstat_multi_port() {
        let output = r#"CA 'mlx5_0'
    CA type: MT4123
    Number of ports: 2
    Port 1:
        State: Active
        Physical state: LinkUp
        Rate: 100
        Base lid: 1
        SM lid: 1
        Port GUID: 0xaaaa
    Port 2:
        State: Down
        Physical state: Disabled
        Rate: 10
        Base lid: 0
        SM lid: 0
        Port GUID: 0xbbbb
CA 'mlx5_1'
    CA type: MT4124
    Number of ports: 1
    Port 1:
        State: Active
        Physical state: LinkUp
        Rate: 200
        Base lid: 5
        SM lid: 1
        Port GUID: 0xcccc
"#;
        let ports = parse_ibstat(output);
        assert_eq!(ports.len(), 3);

        assert_eq!(ports[0].device, "mlx5_0");
        assert_eq!(ports[0].port, "1");
        assert_eq!(ports[0].state, "Active");
        assert_eq!(ports[0].port_guid, "0xaaaa");

        assert_eq!(ports[1].device, "mlx5_0");
        assert_eq!(ports[1].port, "2");
        assert_eq!(ports[1].state, "Down");
        assert_eq!(ports[1].rate, "10");

        assert_eq!(ports[2].device, "mlx5_1");
        assert_eq!(ports[2].port, "1");
        assert_eq!(ports[2].state, "Active");
        assert_eq!(ports[2].rate, "200");
    }

    #[test]
    fn test_parse_ibstat_empty() {
        let ports = parse_ibstat("");
        assert!(ports.is_empty());
    }
}
