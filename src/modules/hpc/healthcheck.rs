//! HPC health check module
//!
//! Runs a configurable set of health checks against an HPC node:
//! - Munge round-trip authentication test
//! - NFS mount availability
//! - Service status checks
//! - Optional GPU validation
//! - Optional InfiniBand validation
//!
//! Returns structured JSON with pass/fail counts.
//!
//! # Parameters
//!
//! - `checks` (optional): List of checks to run. Default: all applicable.
//!   Values: "munge", "nfs", "services", "gpu", "infiniband"
//! - `nfs_mounts` (optional): List of mount points to verify
//! - `services` (optional): List of systemd services to verify
//! - `fail_on_error` (optional): Whether to fail the module on any check failure (default: false)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
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

/// Run a command and return (success, stdout, stderr) without failing on errors.
fn run_cmd_check(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> (bool, String, String) {
    let options = get_exec_options(context);
    match Handle::current().block_on(async { connection.execute(cmd, Some(options)).await }) {
        Ok(result) => (result.success, result.stdout, result.stderr),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

pub struct HpcHealthcheckModule;

impl Module for HpcHealthcheckModule {
    fn name(&self) -> &'static str {
        "hpc_healthcheck"
    }

    fn description(&self) -> &'static str {
        "Run HPC node health checks (munge, NFS, services, GPU, InfiniBand)"
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

        if context.check_mode {
            return Ok(ModuleOutput::ok("Would run HPC health checks"));
        }

        let enabled_checks = params.get_vec_string("checks")?;
        let nfs_mounts = params.get_vec_string("nfs_mounts")?.unwrap_or_default();
        let services = params.get_vec_string("services")?.unwrap_or_default();
        let fail_on_error = params.get_bool_or("fail_on_error", false);

        let mut results: Vec<serde_json::Value> = Vec::new();
        let mut passed = 0u32;
        let mut failed = 0u32;

        let should_run = |check: &str| -> bool {
            match &enabled_checks {
                Some(checks) => checks.iter().any(|c| c == check),
                None => true,
            }
        };

        // --- Munge check ---
        if should_run("munge") {
            let (ok, _stdout, stderr) =
                run_cmd_check(connection, "munge -n | unmunge >/dev/null 2>&1", context);
            let entry = serde_json::json!({
                "check": "munge",
                "passed": ok,
                "detail": if ok { "Munge round-trip succeeded" } else { "Munge round-trip failed" },
                "stderr": stderr.trim(),
            });
            if ok {
                passed += 1;
            } else {
                failed += 1;
            }
            results.push(entry);
        }

        // --- NFS mount checks ---
        if should_run("nfs") {
            for mount in &nfs_mounts {
                let (ok, _, stderr) = run_cmd_check(
                    connection,
                    &format!("mountpoint -q '{}'", mount),
                    context,
                );
                let entry = serde_json::json!({
                    "check": "nfs",
                    "mount": mount,
                    "passed": ok,
                    "detail": if ok {
                        format!("{} is mounted", mount)
                    } else {
                        format!("{} is NOT mounted", mount)
                    },
                    "stderr": stderr.trim(),
                });
                if ok {
                    passed += 1;
                } else {
                    failed += 1;
                }
                results.push(entry);
            }
        }

        // --- Service checks ---
        if should_run("services") {
            for svc in &services {
                let (ok, _, stderr) = run_cmd_check(
                    connection,
                    &format!("systemctl is-active '{}'", svc),
                    context,
                );
                let entry = serde_json::json!({
                    "check": "service",
                    "service": svc,
                    "passed": ok,
                    "detail": if ok {
                        format!("{} is active", svc)
                    } else {
                        format!("{} is NOT active", svc)
                    },
                    "stderr": stderr.trim(),
                });
                if ok {
                    passed += 1;
                } else {
                    failed += 1;
                }
                results.push(entry);
            }
        }

        // --- GPU check ---
        if should_run("gpu") {
            let (nvidia_present, _, _) =
                run_cmd_check(connection, "which nvidia-smi >/dev/null 2>&1", context);
            if nvidia_present {
                let (ok, stdout, stderr) = run_cmd_check(
                    connection,
                    "nvidia-smi --query-gpu=gpu_name,memory.total,driver_version --format=csv,noheader 2>&1",
                    context,
                );
                let entry = serde_json::json!({
                    "check": "gpu",
                    "passed": ok,
                    "detail": if ok { stdout.trim().to_string() } else { "nvidia-smi failed".to_string() },
                    "stderr": stderr.trim(),
                });
                if ok {
                    passed += 1;
                } else {
                    failed += 1;
                }
                results.push(entry);
            }
        }

        // --- InfiniBand check ---
        if should_run("infiniband") {
            let (ib_present, _, _) =
                run_cmd_check(connection, "which ibstat >/dev/null 2>&1", context);
            if ib_present {
                let (ok, stdout, stderr) =
                    run_cmd_check(connection, "ibstat -s 2>&1", context);
                let entry = serde_json::json!({
                    "check": "infiniband",
                    "passed": ok,
                    "detail": if ok { stdout.trim().to_string() } else { "ibstat failed".to_string() },
                    "stderr": stderr.trim(),
                });
                if ok {
                    passed += 1;
                } else {
                    failed += 1;
                }
                results.push(entry);
            }
        }

        let summary = serde_json::json!({
            "checks": results,
            "passed": passed,
            "failed": failed,
            "total": passed + failed,
        });

        if failed > 0 && fail_on_error {
            return Err(ModuleError::ExecutionFailed(format!(
                "HPC health check: {} of {} checks failed",
                failed,
                passed + failed
            )));
        }

        let msg = format!(
            "HPC health check: {}/{} passed",
            passed,
            passed + failed
        );

        Ok(ModuleOutput::ok(msg).with_data("healthcheck", summary))
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("checks", serde_json::json!(null));
        m.insert("nfs_mounts", serde_json::json!([]));
        m.insert("services", serde_json::json!([]));
        m.insert("fail_on_error", serde_json::json!(false));
        m
    }
}
