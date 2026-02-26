//! NVIDIA NCCL (Collective Communications Library) module
//!
//! Installs and configures NCCL for multi-GPU and multi-node collective
//! operations, with optional performance validation.
//!
//! # Parameters
//!
//! - `state` (optional): "present" (default) or "absent"
//! - `version` (optional): Specific version to install (e.g. "2.29.3-1+cuda12.9")
//! - `config` (optional): JSON object of NCCL environment variables for /etc/nccl.conf
//! - `validate` (optional): Run nccl-test all_reduce_perf (default: false)

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

fn detect_os_family(os_release: &str) -> Option<&'static str> {
    for line in os_release.lines() {
        if line.starts_with("ID_LIKE=") || line.starts_with("ID=") {
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
                return Some("rhel");
            } else if val.contains("debian") || val.contains("ubuntu") {
                return Some("debian");
            }
        }
    }
    None
}

// ---- Helper structs ----

#[derive(Debug, serde::Serialize)]
struct NcclValidationResult {
    passed: bool,
    bandwidth_gb_s: Option<f64>,
    details: Vec<String>,
}

// ---- Helper functions ----

/// Generate /etc/nccl.conf content from a JSON config object.
///
/// The config is expected to be a `serde_json::Value::Object` where keys
/// are NCCL environment variable names and values are their settings.
///
/// Example input: `{"NCCL_DEBUG": "INFO", "NCCL_SOCKET_IFNAME": "eth0"}`
/// Example output:
/// ```text
/// # NCCL configuration - managed by rustible
/// NCCL_DEBUG=INFO
/// NCCL_SOCKET_IFNAME=eth0
/// ```
fn generate_nccl_conf(config: &serde_json::Value) -> String {
    let mut lines = vec!["# NCCL configuration - managed by rustible".to_string()];

    if let Some(obj) = config.as_object() {
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        for key in keys {
            let val = match &obj[key] {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            lines.push(format!("{}={}", key, val));
        }
    }

    lines.push(String::new()); // trailing newline
    lines.join("\n")
}

/// Parse `all_reduce_perf` output to extract the peak bandwidth.
///
/// Example output:
/// ```text
/// #       size         count      type   redop    root     time   algbw   busbw #wrong     time   algbw   busbw #wrong
///      1048576        262144     float     sum      -1    0.123   8.52   14.91      0    0.122   8.59   15.03      0
///      2097152        524288     float     sum      -1    0.134  15.65   27.39      0    0.133  15.77   27.59      0
/// ```
fn parse_nccl_test_output(output: &str) -> NcclValidationResult {
    let mut max_busbw: f64 = 0.0;
    let mut details = Vec::new();
    let mut has_data = false;

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        // We expect at least 13 columns; busbw (out-of-place) is column index 7
        if parts.len() >= 13 {
            if let Ok(busbw) = parts[7].parse::<f64>() {
                has_data = true;
                if busbw > max_busbw {
                    max_busbw = busbw;
                }
            }
        }
        details.push(line.to_string());
    }

    NcclValidationResult {
        passed: has_data && max_busbw > 0.0,
        bandwidth_gb_s: if has_data { Some(max_busbw) } else { None },
        details,
    }
}

/// Parse NCCL version from dpkg/rpm output or NCCL_VERSION file.
///
/// Accepts strings like "2.29.3-1+cuda12.9" or "2.29.3".
fn parse_nccl_version(output: &str) -> Option<String> {
    let version = output.trim();
    if version.is_empty() {
        return None;
    }
    // Take the first line only
    Some(version.lines().next().unwrap_or("").trim().to_string())
}

// ---- NCCL Module ----

pub struct NcclModule;

impl Module for NcclModule {
    fn name(&self) -> &'static str {
        "nccl"
    }

    fn description(&self) -> &'static str {
        "Install and configure NVIDIA NCCL for multi-GPU collective operations"
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

        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported("Unsupported OS for NCCL module".to_string())
        })?;

        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let version = params.get_string("version")?;
        let config = params.get("config").cloned();
        let validate = params.get_bool_or("validate", false);

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // -- state=absent --
        if state == "absent" {
            if context.check_mode {
                return Ok(ModuleOutput::changed("Would remove NCCL packages"));
            }

            let remove_cmd = match os_family {
                "rhel" => "dnf remove -y libnccl libnccl-devel",
                _ => {
                    "DEBIAN_FRONTEND=noninteractive apt-get remove --purge -y libnccl2 libnccl-dev"
                }
            };
            let _ = run_cmd(connection, remove_cmd, context);
            let _ = run_cmd(connection, "rm -f /etc/nccl.conf", context);

            return Ok(
                ModuleOutput::changed("Removed NCCL packages and configuration").with_data(
                    "changes",
                    serde_json::json!(["Removed NCCL packages", "Removed /etc/nccl.conf"]),
                ),
            );
        }

        // -- state=present --

        // Step 1: Install NCCL packages
        let (pkg_name, dev_pkg) = match os_family {
            "rhel" => {
                if let Some(ref v) = version {
                    (format!("libnccl-{}", v), format!("libnccl-devel-{}", v))
                } else {
                    ("libnccl".to_string(), "libnccl-devel".to_string())
                }
            }
            _ => {
                if let Some(ref v) = version {
                    (format!("libnccl2={}", v), format!("libnccl-dev={}", v))
                } else {
                    ("libnccl2".to_string(), "libnccl-dev".to_string())
                }
            }
        };

        let check_cmd = match os_family {
            "rhel" => "rpm -q libnccl >/dev/null 2>&1",
            _ => "dpkg -s libnccl2 >/dev/null 2>&1",
        };
        let (installed, _, _) = run_cmd(connection, check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push(format!("Would install {} {}", pkg_name, dev_pkg));
            } else {
                let install_cmd = match os_family {
                    "rhel" => format!("dnf install -y {} {}", pkg_name, dev_pkg),
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y {} {}",
                        pkg_name, dev_pkg
                    ),
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push(format!("Installed {} {}", pkg_name, dev_pkg));
            }
        }

        // Step 2: Write /etc/nccl.conf if config provided
        if let Some(ref config_val) = config {
            if config_val.is_object() {
                let conf_content = generate_nccl_conf(config_val);
                if context.check_mode {
                    changes.push("Would write /etc/nccl.conf".to_string());
                } else {
                    // Check if content differs
                    let (exists, current, _) =
                        run_cmd(connection, "cat /etc/nccl.conf 2>/dev/null", context)?;

                    if !exists || current != conf_content {
                        let escaped = conf_content.replace('\'', "'\\''");
                        run_cmd_ok(
                            connection,
                            &format!("printf '%s' '{}' > /etc/nccl.conf", escaped),
                            context,
                        )?;
                        changed = true;
                        changes.push("Updated /etc/nccl.conf".to_string());
                    }
                }
            }
        }

        // Step 3: Run validation
        let validation = if validate && !context.check_mode {
            let (ok, stdout, stderr) = run_cmd(
                connection,
                "all_reduce_perf -b 8 -e 128M -f 2 -g 1 2>&1",
                context,
            )?;
            let combined = if ok {
                stdout
            } else {
                format!("{}\n{}", stdout, stderr)
            };
            let result = parse_nccl_test_output(&combined);
            if result.passed {
                if let Some(bw) = result.bandwidth_gb_s {
                    changes.push(format!(
                        "NCCL all_reduce_perf peak bus bandwidth: {:.2} GB/s",
                        bw
                    ));
                }
            } else {
                changes.push("NCCL all_reduce_perf test did not produce valid results".to_string());
            }
            Some(result)
        } else if validate && context.check_mode {
            changes.push("Would run all_reduce_perf".to_string());
            None
        } else {
            None
        };

        // Step 4: Query installed version
        let installed_version = if !context.check_mode {
            let version_cmd = match os_family {
                "rhel" => "rpm -q --qf '%{VERSION}-%{RELEASE}' libnccl 2>/dev/null",
                _ => "dpkg-query -W -f='${Version}' libnccl2 2>/dev/null",
            };
            let (ok, stdout, _) = run_cmd(connection, version_cmd, context)?;
            if ok {
                parse_nccl_version(&stdout)
            } else {
                None
            }
        } else {
            None
        };

        // Build output
        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} NCCL changes",
                changes.len()
            ))
            .with_data("changes", serde_json::json!(changes)));
        }

        let mut output = if changed {
            ModuleOutput::changed(format!("Applied {} NCCL changes", changes.len()))
        } else {
            ModuleOutput::ok("NCCL is installed and configured")
        };

        output = output.with_data("changes", serde_json::json!(changes));

        if let Some(ref v) = installed_version {
            output = output.with_data("version", serde_json::json!(v));
        }
        if let Some(ref val) = validation {
            output = output.with_data("validation", serde_json::json!(val));
        }

        Ok(output)
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("state", serde_json::json!("present"));
        m.insert("version", serde_json::json!(null));
        m.insert("config", serde_json::json!(null));
        m.insert("validate", serde_json::json!(false));
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_metadata() {
        let module = NcclModule;
        assert_eq!(module.name(), "nccl");
        assert!(!module.description().is_empty());
        assert_eq!(module.required_params().len(), 0);
    }

    #[test]
    fn test_generate_nccl_conf() {
        let config = serde_json::json!({
            "NCCL_DEBUG": "INFO",
            "NCCL_SOCKET_IFNAME": "eth0",
            "NCCL_IB_HCA": "mlx5"
        });
        let content = generate_nccl_conf(&config);
        assert!(content.starts_with("# NCCL configuration"));
        assert!(content.contains("NCCL_DEBUG=INFO"));
        assert!(content.contains("NCCL_SOCKET_IFNAME=eth0"));
        assert!(content.contains("NCCL_IB_HCA=mlx5"));
    }

    #[test]
    fn test_generate_nccl_conf_sorted() {
        let config = serde_json::json!({
            "NCCL_SOCKET_IFNAME": "eth0",
            "NCCL_DEBUG": "WARN"
        });
        let content = generate_nccl_conf(&config);
        let lines: Vec<&str> = content.lines().collect();
        // Keys should be sorted alphabetically
        assert!(lines[1].starts_with("NCCL_DEBUG"));
        assert!(lines[2].starts_with("NCCL_SOCKET_IFNAME"));
    }

    #[test]
    fn test_generate_nccl_conf_empty() {
        let config = serde_json::json!({});
        let content = generate_nccl_conf(&config);
        assert!(content.starts_with("# NCCL configuration"));
        // Should only have the header and trailing newline
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_parse_nccl_test_output() {
        let output = r#"# nThread 1 nGpus 1 minBytes 8 maxBytes 134217728 step: 2(factor) warmup iters: 5 iters: 20 agg iters: 1 validation: 1 graph: 0
#
# Using devices
#   Rank  0 Group  0 Pid 12345 on localhost device  0 [0x3b] NVIDIA A100-SXM4-80GB
#
#       size         count      type   redop    root     time   algbw   busbw #wrong     time   algbw   busbw #wrong
           8             2     float     sum      -1    0.001   0.01    0.01      0    0.001   0.01    0.01      0
      1048576        262144     float     sum      -1    0.123   8.52   14.91      0    0.122   8.59   15.03      0
    134217728      33554432     float     sum      -1    1.234  108.77  190.35      0    1.233  108.86  190.50      0
"#;
        let result = parse_nccl_test_output(output);
        assert!(result.passed);
        assert!(result.bandwidth_gb_s.is_some());
        // Peak bus bandwidth should be 190.35
        let bw = result.bandwidth_gb_s.unwrap();
        assert!(bw > 190.0 && bw < 191.0);
    }

    #[test]
    fn test_parse_nccl_test_output_empty() {
        let result = parse_nccl_test_output("");
        assert!(!result.passed);
        assert_eq!(result.bandwidth_gb_s, None);
    }

    #[test]
    fn test_parse_nccl_version() {
        assert_eq!(
            parse_nccl_version("2.29.3-1+cuda12.9"),
            Some("2.29.3-1+cuda12.9".to_string())
        );
        assert_eq!(parse_nccl_version("2.29.3\n"), Some("2.29.3".to_string()));
        assert_eq!(parse_nccl_version(""), None);
        assert_eq!(
            parse_nccl_version("  2.29.3  \n"),
            Some("2.29.3".to_string())
        );
    }

    #[test]
    fn test_detect_os_family() {
        assert_eq!(
            detect_os_family("ID_LIKE=\"rhel centos fedora\""),
            Some("rhel")
        );
        assert_eq!(detect_os_family("ID=ubuntu\nVERSION=22.04"), Some("debian"));
        assert_eq!(detect_os_family("ID=freebsd"), None);
    }

    #[test]
    fn test_nccl_validation_result_serialization() {
        let result = NcclValidationResult {
            passed: true,
            bandwidth_gb_s: Some(190.35),
            details: vec!["test line".to_string()],
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["passed"], true);
        assert_eq!(json["bandwidth_gb_s"], 190.35);
        assert_eq!(json["details"].as_array().unwrap().len(), 1);
    }
}
