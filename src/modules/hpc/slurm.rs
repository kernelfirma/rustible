//! Slurm workload manager modules
//!
//! Provides configuration and operations modules for Slurm:
//! - `slurm_config`: Manage slurm.conf, cgroup.conf, gres.conf per role
//! - `slurm_ops`: Cluster operations (reconfigure, drain, resume, update_partition)
//!
//! # SlurmConfigModule Parameters
//!
//! - `role` (required): "controller", "compute", or "dbd"
//! - `slurm_conf` (optional): Content for slurm.conf
//! - `cgroup_conf` (optional): Content for cgroup.conf
//! - `gres_conf` (optional): Content for gres.conf
//! - `slurm_user_uid` (optional): UID for the slurm user (default: 64030)
//!
//! # SlurmOpsModule Parameters
//!
//! - `action` (required): "reconfigure", "drain", "resume", "update_partition"
//! - `nodes` (optional): Node list for drain/resume
//! - `reason` (optional): Reason string for drain
//! - `partition_config` (optional): Map of partition settings for update_partition

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

/// Write a config file if its content differs from the desired content.
/// Returns true if the file was changed.
fn ensure_config_file(
    connection: &Arc<dyn Connection + Send + Sync>,
    path: &str,
    content: &str,
    context: &ModuleContext,
) -> ModuleResult<bool> {
    let (_, existing, _) = run_cmd(
        connection,
        &format!("cat '{}' 2>/dev/null || true", path),
        context,
    )?;
    if existing.trim() == content.trim() {
        return Ok(false);
    }
    run_cmd_ok(
        connection,
        &format!(
            "printf '%s\\n' '{}' > '{}'",
            content.replace('\'', "'\\''"),
            path
        ),
        context,
    )?;
    Ok(true)
}

// ---- Slurm Config Module ----

pub struct SlurmConfigModule;

impl Module for SlurmConfigModule {
    fn name(&self) -> &'static str {
        "slurm_config"
    }

    fn description(&self) -> &'static str {
        "Manage Slurm configuration files (slurm.conf, cgroup.conf, gres.conf) per role"
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

        let role = params.get_string_required("role")?;
        if !["controller", "compute", "dbd"].contains(&role.as_str()) {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid role '{}'. Must be 'controller', 'compute', or 'dbd'",
                role
            )));
        }

        let slurm_uid = params
            .get_string("slurm_user_uid")?
            .unwrap_or_else(|| "64030".to_string());

        let os_stdout = run_cmd_ok(connection, "cat /etc/os-release", context)?;
        let os_family = detect_os_family(&os_stdout).ok_or_else(|| {
            ModuleError::Unsupported(
                "Unsupported OS. Slurm module supports RHEL-family and Debian-family.".to_string(),
            )
        })?;

        let mut changed = false;
        let mut changes: Vec<String> = Vec::new();

        // Ensure slurm user exists
        let (user_exists, _, _) = run_cmd(connection, "id slurm 2>/dev/null", context)?;
        if !user_exists {
            if context.check_mode {
                changes.push(format!("Would create slurm user (uid={})", slurm_uid));
            } else {
                run_cmd_ok(
                    connection,
                    &format!(
                        "groupadd -g {} slurm 2>/dev/null; useradd -u {} -g slurm -s /sbin/nologin -d /nonexistent slurm 2>/dev/null || true",
                        slurm_uid, slurm_uid
                    ),
                    context,
                )?;
                changed = true;
                changes.push(format!("Created slurm user (uid={})", slurm_uid));
            }
        }

        // Install packages per role
        let packages = match (os_family, role.as_str()) {
            ("rhel", "controller") => "slurm-slurmctld slurm slurm-perlapi",
            ("rhel", "compute") => "slurm-slurmd slurm slurm-pam_slurm",
            ("rhel", "dbd") => "slurm-slurmdbd slurm",
            (_, "controller") => "slurmctld slurm-client",
            (_, "compute") => "slurmd slurm-client",
            (_, "dbd") => "slurmdbd slurm-client",
            _ => "",
        };

        let first_pkg = packages.split_whitespace().next().unwrap_or("");
        let check_cmd = match os_family {
            "rhel" => format!("rpm -q {} >/dev/null 2>&1", first_pkg),
            _ => format!("dpkg -s {} >/dev/null 2>&1", first_pkg),
        };
        let (installed, _, _) = run_cmd(connection, &check_cmd, context)?;

        if !installed {
            if context.check_mode {
                changes.push(format!("Would install Slurm packages for role '{}'", role));
            } else {
                let install_cmd = match os_family {
                    "rhel" => format!("dnf install -y {}", packages),
                    _ => format!(
                        "DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                        packages
                    ),
                };
                run_cmd_ok(connection, &install_cmd, context)?;
                changed = true;
                changes.push(format!("Installed Slurm packages for role '{}'", role));
            }
        }

        // Create Slurm directories
        if !context.check_mode {
            run_cmd_ok(
                connection,
                "mkdir -p /etc/slurm /var/spool/slurm /var/log/slurm /var/run/slurm && \
                 chown slurm:slurm /var/spool/slurm /var/log/slurm /var/run/slurm",
                context,
            )?;
            if role == "controller" {
                run_cmd_ok(
                    connection,
                    "mkdir -p /var/spool/slurm/slurmctld && chown slurm:slurm /var/spool/slurm/slurmctld",
                    context,
                )?;
            }
            if role == "compute" {
                run_cmd_ok(
                    connection,
                    "mkdir -p /var/spool/slurm/slurmd && chown slurm:slurm /var/spool/slurm/slurmd",
                    context,
                )?;
            }
        }

        // Write configuration files
        if let Some(slurm_conf) = params.get_string("slurm_conf")? {
            if context.check_mode {
                changes.push("Would write slurm.conf".to_string());
            } else if ensure_config_file(connection, "/etc/slurm/slurm.conf", &slurm_conf, context)?
            {
                changed = true;
                changes.push("Updated slurm.conf".to_string());
            }
        }

        if let Some(cgroup_conf) = params.get_string("cgroup_conf")? {
            if context.check_mode {
                changes.push("Would write cgroup.conf".to_string());
            } else if ensure_config_file(
                connection,
                "/etc/slurm/cgroup.conf",
                &cgroup_conf,
                context,
            )? {
                changed = true;
                changes.push("Updated cgroup.conf".to_string());
            }
        }

        if let Some(gres_conf) = params.get_string("gres_conf")? {
            if context.check_mode {
                changes.push("Would write gres.conf".to_string());
            } else if ensure_config_file(connection, "/etc/slurm/gres.conf", &gres_conf, context)? {
                changed = true;
                changes.push("Updated gres.conf".to_string());
            }
        }

        // Enable service for this role
        let service = match role.as_str() {
            "controller" => "slurmctld.service",
            "compute" => "slurmd.service",
            "dbd" => "slurmdbd.service",
            _ => "",
        };

        if !service.is_empty() {
            let (active, _, _) = run_cmd(
                connection,
                &format!("systemctl is-active {}", service),
                context,
            )?;
            let (enabled, _, _) = run_cmd(
                connection,
                &format!("systemctl is-enabled {}", service),
                context,
            )?;

            if !enabled {
                if context.check_mode {
                    changes.push(format!("Would enable {}", service));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("systemctl enable {}", service),
                        context,
                    )?;
                    changed = true;
                    changes.push(format!("Enabled {}", service));
                }
            }

            // Restart if config changed
            if changed && active {
                if context.check_mode {
                    changes.push(format!("Would restart {}", service));
                } else {
                    run_cmd_ok(
                        connection,
                        &format!("systemctl restart {}", service),
                        context,
                    )?;
                    changes.push(format!("Restarted {}", service));
                }
            } else if !active {
                if context.check_mode {
                    changes.push(format!("Would start {}", service));
                } else {
                    run_cmd_ok(connection, &format!("systemctl start {}", service), context)?;
                    changed = true;
                    changes.push(format!("Started {}", service));
                }
            }
        }

        if context.check_mode && !changes.is_empty() {
            return Ok(ModuleOutput::changed(format!(
                "Would apply {} Slurm config changes for role '{}'",
                changes.len(),
                role
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("role", serde_json::json!(role)));
        }

        if changed {
            Ok(ModuleOutput::changed(format!(
                "Applied {} Slurm config changes for role '{}'",
                changes.len(),
                role
            ))
            .with_data("changes", serde_json::json!(changes))
            .with_data("role", serde_json::json!(role)))
        } else {
            Ok(
                ModuleOutput::ok(format!("Slurm is configured for role '{}'", role))
                    .with_data("role", serde_json::json!(role)),
            )
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["role"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("slurm_conf", serde_json::json!(null));
        m.insert("cgroup_conf", serde_json::json!(null));
        m.insert("gres_conf", serde_json::json!(null));
        m.insert("slurm_user_uid", serde_json::json!("64030"));
        m
    }
}

// ---- Slurm Ops Module ----

pub struct SlurmOpsModule;

impl Module for SlurmOpsModule {
    fn name(&self) -> &'static str {
        "slurm_ops"
    }

    fn description(&self) -> &'static str {
        "Slurm cluster operations (reconfigure, drain/resume nodes, update partitions)"
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

        let action = params.get_string_required("action")?;

        match action.as_str() {
            "reconfigure" => self.action_reconfigure(connection, context),
            "drain" => self.action_drain(connection, params, context),
            "resume" => self.action_resume(connection, params, context),
            "update_partition" => self.action_update_partition(connection, params, context),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'reconfigure', 'drain', 'resume', or 'update_partition'",
                action
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("nodes", serde_json::json!(null));
        m.insert("reason", serde_json::json!(null));
        m.insert("partition_config", serde_json::json!(null));
        m
    }
}

impl SlurmOpsModule {
    fn action_reconfigure(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if context.check_mode {
            return Ok(ModuleOutput::changed("Would reconfigure Slurm"));
        }
        run_cmd_ok(connection, "scontrol reconfigure", context)?;
        Ok(ModuleOutput::changed("Slurm reconfigured"))
    }

    fn action_drain(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let nodes = params.get_string_required("nodes")?;
        let reason = params
            .get_string("reason")?
            .unwrap_or_else(|| "Drained by rustible".to_string());

        // Check current state to ensure idempotency
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("scontrol show node {} -o 2>/dev/null", nodes),
            context,
        )?;

        if ok {
            // Parse node states
            let all_drained = stdout
                .lines()
                .all(|line| line.contains("State=DRAINED") || line.contains("State=DRAIN"));
            if all_drained && !stdout.is_empty() {
                return Ok(
                    ModuleOutput::ok(format!("Nodes '{}' are already drained", nodes))
                        .with_data("nodes", serde_json::json!(nodes))
                        .with_data("action", serde_json::json!("drain")),
                );
            }
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would drain nodes '{}' with reason '{}'",
                nodes, reason
            )));
        }

        run_cmd_ok(
            connection,
            &format!(
                "scontrol update NodeName={} State=drain Reason='{}'",
                nodes,
                reason.replace('\'', "'\\''")
            ),
            context,
        )?;

        // Get updated state
        let (_, node_info, _) = run_cmd(
            connection,
            &format!("scontrol show node {} -o 2>/dev/null", nodes),
            context,
        )?;

        Ok(ModuleOutput::changed(format!("Drained nodes '{}'", nodes))
            .with_data("nodes", serde_json::json!(nodes))
            .with_data("reason", serde_json::json!(reason))
            .with_data("action", serde_json::json!("drain"))
            .with_data("node_info", serde_json::json!(node_info.trim())))
    }

    fn action_resume(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let nodes = params.get_string_required("nodes")?;

        // Check current state
        let (ok, stdout, _) = run_cmd(
            connection,
            &format!("scontrol show node {} -o 2>/dev/null", nodes),
            context,
        )?;

        if ok {
            let all_idle = stdout
                .lines()
                .all(|line| line.contains("State=IDLE") || line.contains("State=ALLOCATED"));
            if all_idle && !stdout.is_empty() {
                return Ok(
                    ModuleOutput::ok(format!("Nodes '{}' are already active", nodes))
                        .with_data("nodes", serde_json::json!(nodes))
                        .with_data("action", serde_json::json!("resume")),
                );
            }
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would resume nodes '{}'",
                nodes
            )));
        }

        run_cmd_ok(
            connection,
            &format!("scontrol update NodeName={} State=resume", nodes),
            context,
        )?;

        Ok(ModuleOutput::changed(format!("Resumed nodes '{}'", nodes))
            .with_data("nodes", serde_json::json!(nodes))
            .with_data("action", serde_json::json!("resume")))
    }

    fn action_update_partition(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let partition_config = params.get("partition_config").ok_or_else(|| {
            ModuleError::MissingParameter(
                "partition_config is required for update_partition".to_string(),
            )
        })?;

        let config_map = partition_config.as_object().ok_or_else(|| {
            ModuleError::InvalidParameter(
                "partition_config must be a map of key=value pairs".to_string(),
            )
        })?;

        // Build scontrol update command
        let mut update_parts: Vec<String> = Vec::new();
        for (key, value) in config_map {
            let val_str = value.as_str().unwrap_or(&value.to_string()).to_string();
            update_parts.push(format!("{}={}", key, val_str));
        }
        let update_str = update_parts.join(" ");

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would update partition: {}", update_str))
                    .with_data("action", serde_json::json!("update_partition")),
            );
        }

        run_cmd_ok(
            connection,
            &format!("scontrol update {}", update_str),
            context,
        )?;

        Ok(
            ModuleOutput::changed(format!("Updated partition: {}", update_str))
                .with_data("action", serde_json::json!("update_partition"))
                .with_data("update", serde_json::json!(update_str)),
        )
    }
}
