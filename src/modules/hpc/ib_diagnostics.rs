//! InfiniBand diagnostics module
//!
//! Run IB diagnostic tools (ibdiagnet, iblinkinfo, ibstat) and parse results.
//!
//! # Parameters
//!
//! - `check` (required): Diagnostic type - "link_health", "topology", "counters", "full"
//! - `port_filter` (optional): Filter results by port/HCA
//! - `output_dir` (optional): Directory to store diagnostic reports
//! - `auto_drain` (optional, default false): Automatically drain node on failure
//! - `threshold_symbol_errors` (optional, default 10): Threshold for SymbolErrorCounter
//! - `threshold_link_downed` (optional, default 5): Threshold for LinkDownedCounter
//! - `drain_reason` (optional): Custom reason string for drain command

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult,
    ParallelizationHint, ParamExt,
};

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

#[derive(Debug, Clone, serde::Serialize)]
struct CounterVerdict {
    counter_name: String,
    value: u64,
    threshold: u64,
    status: String,
}

/// Default thresholds for IB error counters.
fn default_thresholds() -> HashMap<String, u64> {
    let mut m = HashMap::new();
    m.insert("SymbolErrorCounter".to_string(), 10);
    m.insert("LinkDownedCounter".to_string(), 5);
    m.insert("RcvErrors".to_string(), 100);
    m.insert("PortRcvConstraintErrors".to_string(), 50);
    m
}

/// Compare each counter value against its threshold and return a verdict per counter.
///
/// Status is "fail" if value >= threshold, "warn" if value >= 80% of threshold,
/// otherwise "pass".
fn apply_thresholds(
    counters: &HashMap<String, u64>,
    thresholds: &HashMap<String, u64>,
) -> Vec<CounterVerdict> {
    let mut verdicts = Vec::new();
    for (name, &threshold) in thresholds {
        let value = counters.get(name).copied().unwrap_or(0);
        let status = if value >= threshold {
            "fail".to_string()
        } else if value as f64 >= threshold as f64 * 0.8 {
            "warn".to_string()
        } else {
            "pass".to_string()
        };
        verdicts.push(CounterVerdict {
            counter_name: name.clone(),
            value,
            threshold,
            status,
        });
    }
    // Sort for deterministic output
    verdicts.sort_by(|a, b| a.counter_name.cmp(&b.counter_name));
    verdicts
}

/// Aggregate per-counter verdicts into an overall verdict.
///
/// If any counter is "fail" the overall verdict is "fail".
/// If any counter is "warn" the overall verdict is "warn".
/// Otherwise the overall verdict is "pass".
fn generate_verdict(
    counters: &HashMap<String, u64>,
    thresholds: &HashMap<String, u64>,
) -> (String, Vec<CounterVerdict>) {
    let verdicts = apply_thresholds(counters, thresholds);
    let overall = if verdicts.iter().any(|v| v.status == "fail") {
        "fail".to_string()
    } else if verdicts.iter().any(|v| v.status == "warn") {
        "warn".to_string()
    } else {
        "pass".to_string()
    };
    (overall, verdicts)
}

/// Build a Slurm drain command for the given hostname when auto_drain is enabled
/// and the overall verdict is "fail".
///
/// Returns `Some(command_string)` if a drain should be triggered, `None` otherwise.
fn trigger_drain(
    auto_drain: bool,
    overall_verdict: &str,
    hostname: &str,
    drain_reason: &str,
) -> Option<String> {
    if auto_drain && overall_verdict == "fail" {
        Some(format!(
            "scontrol update NodeName={} State=DRAIN Reason=\"IB health check failed: {}\"",
            hostname, drain_reason
        ))
    } else {
        None
    }
}

/// Parse `ibqueryerrors` output to extract counter name/value pairs.
fn parse_counter_values(output: &str) -> HashMap<String, u64> {
    let mut counters = HashMap::new();
    for line in output.lines() {
        let trimmed = line.trim();
        // ibqueryerrors output format: "CounterName:...Value" or similar patterns
        for counter_name in &[
            "SymbolErrorCounter",
            "LinkDownedCounter",
            "RcvErrors",
            "PortRcvConstraintErrors",
        ] {
            if trimmed.contains(counter_name) {
                // Try to extract a numeric value from the line
                if let Some(val) = extract_counter_value(trimmed) {
                    let entry = counters.entry(counter_name.to_string()).or_insert(0);
                    *entry += val;
                }
            }
        }
    }
    counters
}

/// Extract a numeric value from a counter line.
fn extract_counter_value(line: &str) -> Option<u64> {
    // Look for patterns like "....:42" or ".... 42"
    line.split_whitespace()
        .rev()
        .chain(line.rsplit(':').next())
        .find_map(|part| part.trim().parse::<u64>().ok())
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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticCheck {
    LinkHealth,
    Topology,
    Counters,
    Full,
}

impl DiagnosticCheck {
    fn from_str(s: &str) -> Option<DiagnosticCheck> {
        match s.to_lowercase().as_str() {
            "link_health" => Some(DiagnosticCheck::LinkHealth),
            "topology" => Some(DiagnosticCheck::Topology),
            "counters" => Some(DiagnosticCheck::Counters),
            "full" => Some(DiagnosticCheck::Full),
            _ => None,
        }
    }

    fn to_commands(&self) -> Vec<&'static str> {
        match self {
            DiagnosticCheck::LinkHealth => vec!["iblinkinfo", "ibstat"],
            DiagnosticCheck::Topology => vec!["ibnetdiscover", "iblinkinfo"],
            DiagnosticCheck::Counters => vec!["ibqueryerrors"],
            DiagnosticCheck::Full => vec!["ibdiagnet", "iblinkinfo", "ibstat", "ibqueryerrors"],
        }
    }
}

pub struct IbDiagnosticsModule;

impl Module for IbDiagnosticsModule {
    fn name(&self) -> &'static str {
        "ib_diagnostics"
    }

    fn description(&self) -> &'static str {
        "Run InfiniBand diagnostic tools and parse results"
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

        let check_str = params.get_string_required("check")?;
        let port_filter = params.get_string("port_filter")?;
        let output_dir = params
            .get_string("output_dir")?
            .unwrap_or_else(|| "/tmp/ib_diagnostics".to_string());
        let auto_drain = params.get_bool_or("auto_drain", false);
        let threshold_symbol_errors =
            params.get_u32("threshold_symbol_errors")?.unwrap_or(10) as u64;
        let threshold_link_downed = params.get_u32("threshold_link_downed")?.unwrap_or(5) as u64;
        let drain_reason = params
            .get_string("drain_reason")?
            .unwrap_or_else(|| "IB diagnostics threshold exceeded".to_string());

        let check = DiagnosticCheck::from_str(&check_str).ok_or_else(|| {
            ModuleError::InvalidParameter(format!(
                "Invalid check type '{}'. Must be 'link_health', 'topology', 'counters', or 'full'",
                check_str
            ))
        })?;

        if context.check_mode {
            return Ok(
                ModuleOutput::ok(format!("Would run IB diagnostics: {:?}", check))
                    .with_data("check", serde_json::json!(check)),
            );
        }

        // Create output directory
        run_cmd_ok(connection, &format!("mkdir -p {}", output_dir), context)?;

        let mut results: HashMap<String, String> = HashMap::new();
        let mut errors: Vec<String> = Vec::new();

        for cmd in check.to_commands() {
            let mut full_cmd = cmd.to_string();
            if let Some(ref filter) = port_filter {
                full_cmd.push_str(&format!(" -C {}", filter));
            }

            let (ok, stdout, stderr) = run_cmd(connection, &full_cmd, context)?;

            if ok {
                results.insert(cmd.to_string(), stdout.clone());

                // Save to file
                let output_file = format!("{}/{}.log", output_dir, cmd);
                let escaped = stdout.replace('\'', "'\\''");
                let _ = run_cmd(
                    connection,
                    &format!("echo '{}' > {}", escaped, output_file),
                    context,
                );
            } else {
                errors.push(format!("{}: {}", cmd, stderr.trim()));
            }
        }

        // Parse results for issues
        let mut issues: Vec<String> = Vec::new();

        // Check iblinkinfo for link issues
        if let Some(linkinfo) = results.get("iblinkinfo") {
            if linkinfo.contains("Down") || linkinfo.contains("Polling") {
                issues.push("Link state issues detected".to_string());
            }
        }

        // Check ibqueryerrors for error counters
        if let Some(errors_output) = results.get("ibqueryerrors") {
            if !errors_output.trim().is_empty() && !errors_output.contains("No errors") {
                issues.push("Error counters detected".to_string());
            }
        }

        // Build custom thresholds from params (override defaults)
        let mut thresholds = default_thresholds();
        thresholds.insert("SymbolErrorCounter".to_string(), threshold_symbol_errors);
        thresholds.insert("LinkDownedCounter".to_string(), threshold_link_downed);

        // Parse counters and generate verdicts if counter data is available
        let counter_values = results
            .get("ibqueryerrors")
            .map(|output| parse_counter_values(output))
            .unwrap_or_default();

        let (overall_verdict, counter_verdicts) = generate_verdict(&counter_values, &thresholds);

        // Optionally trigger drain
        let drain_cmd = if auto_drain && overall_verdict == "fail" {
            // Get hostname from the remote node
            let hostname = run_cmd_ok(connection, "hostname -s", context)
                .unwrap_or_else(|_| "unknown".to_string())
                .trim()
                .to_string();
            trigger_drain(true, &overall_verdict, &hostname, &drain_reason)
        } else {
            None
        };

        let summary = if issues.is_empty() && errors.is_empty() {
            format!(
                "IB diagnostics completed successfully. Verdict: {}",
                overall_verdict
            )
        } else if !issues.is_empty() {
            format!(
                "IB diagnostics completed. {} issue(s) detected: {}. Verdict: {}",
                issues.len(),
                issues.join(", "),
                overall_verdict
            )
        } else {
            format!(
                "IB diagnostics completed with {} error(s). Verdict: {}",
                errors.len(),
                overall_verdict
            )
        };

        let mut output = ModuleOutput::ok(summary)
            .with_data("check", serde_json::json!(check))
            .with_data("output_dir", serde_json::json!(output_dir))
            .with_data("issues", serde_json::json!(issues))
            .with_data("errors", serde_json::json!(errors))
            .with_data("results", serde_json::json!(results))
            .with_data("verdict", serde_json::json!(overall_verdict))
            .with_data("counter_verdicts", serde_json::json!(counter_verdicts));

        if let Some(ref cmd) = drain_cmd {
            output = output.with_data("drain_command", serde_json::json!(cmd));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &["check"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("port_filter", serde_json::json!(null));
        m.insert("output_dir", serde_json::json!("/tmp/ib_diagnostics"));
        m.insert("auto_drain", serde_json::json!(false));
        m.insert("threshold_symbol_errors", serde_json::json!(10));
        m.insert("threshold_link_downed", serde_json::json!(5));
        m.insert("drain_reason", serde_json::json!(null));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = IbDiagnosticsModule;
        assert_eq!(module.name(), "ib_diagnostics");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_required_params() {
        let module = IbDiagnosticsModule;
        let required = module.required_params();
        assert!(required.contains(&"check"));
    }

    #[test]
    fn test_optional_params() {
        let module = IbDiagnosticsModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("port_filter"));
        assert!(optional.contains_key("output_dir"));
    }

    #[test]
    fn test_diagnostic_check_from_str() {
        assert_eq!(
            DiagnosticCheck::from_str("link_health"),
            Some(DiagnosticCheck::LinkHealth)
        );
        assert_eq!(
            DiagnosticCheck::from_str("topology"),
            Some(DiagnosticCheck::Topology)
        );
        assert_eq!(
            DiagnosticCheck::from_str("COUNTERS"),
            Some(DiagnosticCheck::Counters)
        );
        assert_eq!(
            DiagnosticCheck::from_str("full"),
            Some(DiagnosticCheck::Full)
        );
        assert_eq!(DiagnosticCheck::from_str("invalid"), None);
    }

    #[test]
    fn test_diagnostic_check_to_commands() {
        let link_health = DiagnosticCheck::LinkHealth;
        assert!(link_health.to_commands().contains(&"iblinkinfo"));
        assert!(link_health.to_commands().contains(&"ibstat"));

        let full = DiagnosticCheck::Full;
        assert!(full.to_commands().len() >= 4);
        assert!(full.to_commands().contains(&"ibdiagnet"));
    }

    #[test]
    fn test_threshold_application() {
        let mut counters = HashMap::new();
        counters.insert("SymbolErrorCounter".to_string(), 15);
        counters.insert("LinkDownedCounter".to_string(), 3);
        counters.insert("RcvErrors".to_string(), 85);
        counters.insert("PortRcvConstraintErrors".to_string(), 10);

        let thresholds = default_thresholds();
        let verdicts = apply_thresholds(&counters, &thresholds);

        assert_eq!(verdicts.len(), 4);

        // SymbolErrorCounter: 15 >= 10 -> fail
        let sym = verdicts
            .iter()
            .find(|v| v.counter_name == "SymbolErrorCounter")
            .unwrap();
        assert_eq!(sym.status, "fail");
        assert_eq!(sym.value, 15);
        assert_eq!(sym.threshold, 10);

        // LinkDownedCounter: 3 < 4 (80% of 5) -> pass
        let link = verdicts
            .iter()
            .find(|v| v.counter_name == "LinkDownedCounter")
            .unwrap();
        assert_eq!(link.status, "pass");

        // RcvErrors: 85 >= 80 (80% of 100) -> warn
        let rcv = verdicts
            .iter()
            .find(|v| v.counter_name == "RcvErrors")
            .unwrap();
        assert_eq!(rcv.status, "warn");

        // PortRcvConstraintErrors: 10 < 40 (80% of 50) -> pass
        let port = verdicts
            .iter()
            .find(|v| v.counter_name == "PortRcvConstraintErrors")
            .unwrap();
        assert_eq!(port.status, "pass");
    }

    #[test]
    fn test_threshold_application_with_custom_thresholds() {
        let mut counters = HashMap::new();
        counters.insert("SymbolErrorCounter".to_string(), 8);

        let mut thresholds = HashMap::new();
        thresholds.insert("SymbolErrorCounter".to_string(), 10);

        let verdicts = apply_thresholds(&counters, &thresholds);
        assert_eq!(verdicts.len(), 1);

        // 8 >= 8.0 (80% of 10) -> warn
        let v = &verdicts[0];
        assert_eq!(v.status, "warn");
        assert_eq!(v.value, 8);
    }

    #[test]
    fn test_threshold_application_missing_counter() {
        // Counter not present in map defaults to 0
        let counters: HashMap<String, u64> = HashMap::new();
        let mut thresholds = HashMap::new();
        thresholds.insert("SymbolErrorCounter".to_string(), 10);

        let verdicts = apply_thresholds(&counters, &thresholds);
        assert_eq!(verdicts.len(), 1);
        assert_eq!(verdicts[0].value, 0);
        assert_eq!(verdicts[0].status, "pass");
    }

    #[test]
    fn test_verdict_aggregation() {
        // Case 1: All pass
        let counters_pass: HashMap<String, u64> = HashMap::new();
        let thresholds = default_thresholds();
        let (overall, _) = generate_verdict(&counters_pass, &thresholds);
        assert_eq!(overall, "pass");

        // Case 2: One counter in warn range
        let mut counters_warn = HashMap::new();
        counters_warn.insert("RcvErrors".to_string(), 85); // 85 >= 80 (80% of 100)
        let (overall, verdicts) = generate_verdict(&counters_warn, &thresholds);
        assert_eq!(overall, "warn");
        assert!(verdicts.iter().any(|v| v.status == "warn"));

        // Case 3: One counter in fail range
        let mut counters_fail = HashMap::new();
        counters_fail.insert("SymbolErrorCounter".to_string(), 15); // 15 >= 10
        let (overall, verdicts) = generate_verdict(&counters_fail, &thresholds);
        assert_eq!(overall, "fail");
        assert!(verdicts.iter().any(|v| v.status == "fail"));

        // Case 4: Both warn and fail -> overall fail
        let mut counters_mixed = HashMap::new();
        counters_mixed.insert("SymbolErrorCounter".to_string(), 15); // fail
        counters_mixed.insert("RcvErrors".to_string(), 85); // warn
        let (overall, _) = generate_verdict(&counters_mixed, &thresholds);
        assert_eq!(overall, "fail");
    }

    #[test]
    fn test_drain_command_generation() {
        // auto_drain=true and verdict=fail -> should produce command
        let cmd = trigger_drain(true, "fail", "node01", "IB diagnostics threshold exceeded");
        assert!(cmd.is_some());
        let cmd_str = cmd.unwrap();
        assert!(cmd_str.contains("scontrol update NodeName=node01 State=DRAIN"));
        assert!(cmd_str.contains("IB health check failed: IB diagnostics threshold exceeded"));

        // auto_drain=true but verdict is not fail -> no command
        let cmd = trigger_drain(true, "warn", "node01", "threshold exceeded");
        assert!(cmd.is_none());

        let cmd = trigger_drain(true, "pass", "node01", "threshold exceeded");
        assert!(cmd.is_none());

        // auto_drain=false and verdict=fail -> no command
        let cmd = trigger_drain(false, "fail", "node01", "threshold exceeded");
        assert!(cmd.is_none());
    }

    #[test]
    fn test_drain_command_custom_reason() {
        let cmd = trigger_drain(true, "fail", "gpu-node-05", "custom failure reason");
        assert!(cmd.is_some());
        let cmd_str = cmd.unwrap();
        assert!(cmd_str.contains("NodeName=gpu-node-05"));
        assert!(cmd_str.contains("IB health check failed: custom failure reason"));
    }

    #[test]
    fn test_counter_verdict_structure() {
        let verdict = CounterVerdict {
            counter_name: "SymbolErrorCounter".to_string(),
            value: 15,
            threshold: 10,
            status: "fail".to_string(),
        };

        // Test serialization
        let json = serde_json::to_value(&verdict).unwrap();
        assert_eq!(json["counter_name"], "SymbolErrorCounter");
        assert_eq!(json["value"], 15);
        assert_eq!(json["threshold"], 10);
        assert_eq!(json["status"], "fail");

        // Test clone
        let cloned = verdict.clone();
        assert_eq!(cloned.counter_name, "SymbolErrorCounter");
        assert_eq!(cloned.value, 15);
    }

    #[test]
    fn test_counter_verdict_serialization_all_statuses() {
        for status in &["pass", "warn", "fail"] {
            let verdict = CounterVerdict {
                counter_name: "TestCounter".to_string(),
                value: 42,
                threshold: 100,
                status: status.to_string(),
            };
            let json_str = serde_json::to_string(&verdict).unwrap();
            assert!(json_str.contains(&format!("\"status\":\"{}\"", status)));
        }
    }

    #[test]
    fn test_optional_params_include_new_fields() {
        let module = IbDiagnosticsModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("auto_drain"));
        assert!(optional.contains_key("threshold_symbol_errors"));
        assert!(optional.contains_key("threshold_link_downed"));
        assert!(optional.contains_key("drain_reason"));
    }

    #[test]
    fn test_default_thresholds() {
        let t = default_thresholds();
        assert_eq!(t.get("SymbolErrorCounter"), Some(&10));
        assert_eq!(t.get("LinkDownedCounter"), Some(&5));
        assert_eq!(t.get("RcvErrors"), Some(&100));
        assert_eq!(t.get("PortRcvConstraintErrors"), Some(&50));
    }
}
