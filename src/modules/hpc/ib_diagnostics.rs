//! InfiniBand diagnostics module
//!
//! Run IB diagnostic tools (ibdiagnet, iblinkinfo, ibstat) and parse results.
//!
//! # Parameters
//!
//! - `check` (required): Diagnostic type - "link_health", "topology", "counters", "full"
//! - `port_filter` (optional): Filter results by port/HCA
//! - `output_dir` (optional): Directory to store diagnostic reports

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

        let summary = if issues.is_empty() && errors.is_empty() {
            "IB diagnostics completed successfully. No issues detected.".to_string()
        } else if !issues.is_empty() {
            format!(
                "IB diagnostics completed. {} issue(s) detected: {}",
                issues.len(),
                issues.join(", ")
            )
        } else {
            format!("IB diagnostics completed with {} error(s)", errors.len())
        };

        Ok(ModuleOutput::ok(summary)
            .with_data("check", serde_json::json!(check))
            .with_data("output_dir", serde_json::json!(output_dir))
            .with_data("issues", serde_json::json!(issues))
            .with_data("errors", serde_json::json!(errors))
            .with_data("results", serde_json::json!(results)))
    }

    fn required_params(&self) -> &[&'static str] {
        &["check"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("port_filter", serde_json::json!(null));
        m.insert("output_dir", serde_json::json!("/tmp/ib_diagnostics"));
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
}
