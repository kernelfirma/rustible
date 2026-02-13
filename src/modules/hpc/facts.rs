//! HPC facts expansion module
//!
//! Gathers HPC-specific system facts and returns them as structured data:
//! - CPU features from `/proc/cpuinfo` flags
//! - NUMA topology from `/sys/devices/system/node/`
//! - Hugepages configuration
//! - GPU inventory via `nvidia-smi` (if present)
//! - InfiniBand devices via `ibstat`/`lspci` (if present)
//!
//! # Parameters
//!
//! - `gather` (optional): List of fact categories to gather.
//!   Values: "cpu", "numa", "hugepages", "gpu", "infiniband"
//!   Default: all available

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
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

/// Run a command and return stdout on success, None on failure.
fn run_cmd_opt(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> Option<String> {
    let options = get_exec_options(context);
    match Handle::current().block_on(async { connection.execute(cmd, Some(options)).await }) {
        Ok(result) if result.success => Some(result.stdout),
        _ => None,
    }
}

pub struct HpcFactsModule;

impl Module for HpcFactsModule {
    fn name(&self) -> &'static str {
        "hpc_facts"
    }

    fn description(&self) -> &'static str {
        "Gather HPC-specific system facts (CPU features, NUMA, hugepages, GPU, InfiniBand)"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
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
            return Ok(ModuleOutput::ok("Would gather HPC facts"));
        }

        let gather = params.get_vec_string("gather")?;
        let should_gather = |cat: &str| -> bool {
            match &gather {
                Some(cats) => cats.iter().any(|c| c == cat),
                None => true,
            }
        };

        let mut facts = serde_json::Map::new();

        // --- CPU features ---
        if should_gather("cpu") {
            if let Some(cpuinfo) = run_cmd_opt(connection, "cat /proc/cpuinfo", context) {
                let flags: Vec<String> = cpuinfo
                    .lines()
                    .find(|l| l.starts_with("flags"))
                    .map(|l| {
                        l.split(':')
                            .nth(1)
                            .unwrap_or("")
                            .split_whitespace()
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();

                let model_name = cpuinfo
                    .lines()
                    .find(|l| l.starts_with("model name"))
                    .and_then(|l| l.split(':').nth(1))
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();

                let cpu_count = cpuinfo
                    .lines()
                    .filter(|l| l.starts_with("processor"))
                    .count();

                facts.insert(
                    "cpu".to_string(),
                    serde_json::json!({
                        "model": model_name,
                        "count": cpu_count,
                        "flags": flags,
                        "has_avx": flags.contains(&"avx".to_string()),
                        "has_avx2": flags.contains(&"avx2".to_string()),
                        "has_avx512f": flags.contains(&"avx512f".to_string()),
                        "has_sse4_2": flags.contains(&"sse4_2".to_string()),
                    }),
                );
            }
        }

        // --- NUMA topology ---
        if should_gather("numa") {
            if let Some(numa_output) = run_cmd_opt(
                connection,
                "ls -d /sys/devices/system/node/node* 2>/dev/null | wc -l",
                context,
            ) {
                let node_count: u32 = numa_output.trim().parse().unwrap_or(0);
                let mut nodes: Vec<serde_json::Value> = Vec::new();

                for i in 0..node_count {
                    let cpulist = run_cmd_opt(
                        connection,
                        &format!("cat /sys/devices/system/node/node{}/cpulist 2>/dev/null", i),
                        context,
                    )
                    .unwrap_or_default();
                    let meminfo = run_cmd_opt(
                        connection,
                        &format!(
                            "grep MemTotal /sys/devices/system/node/node{}/meminfo 2>/dev/null",
                            i
                        ),
                        context,
                    )
                    .unwrap_or_default();
                    let mem_kb: u64 = meminfo
                        .split_whitespace()
                        .nth(3)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);

                    nodes.push(serde_json::json!({
                        "id": i,
                        "cpulist": cpulist.trim(),
                        "memory_kb": mem_kb,
                    }));
                }

                facts.insert(
                    "numa".to_string(),
                    serde_json::json!({
                        "node_count": node_count,
                        "nodes": nodes,
                    }),
                );
            }
        }

        // --- Hugepages ---
        if should_gather("hugepages") {
            let hp_total = run_cmd_opt(
                connection,
                "cat /proc/sys/vm/nr_hugepages 2>/dev/null",
                context,
            )
            .unwrap_or_default();
            let hp_free = run_cmd_opt(
                connection,
                "cat /proc/meminfo | grep HugePages_Free | awk '{print $2}'",
                context,
            )
            .unwrap_or_default();
            let hp_size = run_cmd_opt(
                connection,
                "cat /proc/meminfo | grep Hugepagesize | awk '{print $2}'",
                context,
            )
            .unwrap_or_default();

            facts.insert(
                "hugepages".to_string(),
                serde_json::json!({
                    "total": hp_total.trim().parse::<u64>().unwrap_or(0),
                    "free": hp_free.trim().parse::<u64>().unwrap_or(0),
                    "size_kb": hp_size.trim().parse::<u64>().unwrap_or(0),
                }),
            );
        }

        // --- GPU inventory ---
        if should_gather("gpu") {
            let has_nvidia = run_cmd_opt(
                connection,
                "which nvidia-smi >/dev/null 2>&1 && echo yes",
                context,
            );
            if has_nvidia.is_some() {
                if let Some(gpu_csv) = run_cmd_opt(
                    connection,
                    "nvidia-smi --query-gpu=index,gpu_name,memory.total,driver_version,gpu_bus_id --format=csv,noheader,nounits 2>/dev/null",
                    context,
                ) {
                    let gpus: Vec<serde_json::Value> = gpu_csv
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .map(|line| {
                            let parts: Vec<&str> =
                                line.split(',').map(|s| s.trim()).collect();
                            serde_json::json!({
                                "index": parts.first().unwrap_or(&""),
                                "name": parts.get(1).unwrap_or(&""),
                                "memory_mib": parts.get(2).unwrap_or(&""),
                                "driver_version": parts.get(3).unwrap_or(&""),
                                "bus_id": parts.get(4).unwrap_or(&""),
                            })
                        })
                        .collect();
                    facts.insert(
                        "gpu".to_string(),
                        serde_json::json!({
                            "count": gpus.len(),
                            "devices": gpus,
                            "vendor": "nvidia",
                        }),
                    );
                }
            } else {
                facts.insert(
                    "gpu".to_string(),
                    serde_json::json!({
                        "count": 0,
                        "devices": [],
                        "vendor": null,
                    }),
                );
            }
        }

        // --- InfiniBand ---
        if should_gather("infiniband") {
            let has_ibstat = run_cmd_opt(
                connection,
                "which ibstat >/dev/null 2>&1 && echo yes",
                context,
            );
            if has_ibstat.is_some() {
                let ib_output =
                    run_cmd_opt(connection, "ibstat -s 2>/dev/null", context).unwrap_or_default();
                let ib_devs: Vec<serde_json::Value> = ib_output
                    .lines()
                    .filter(|l| l.contains("CA '"))
                    .map(|l| {
                        let name = l.split('\'').nth(1).unwrap_or("unknown").to_string();
                        serde_json::json!({"name": name})
                    })
                    .collect();
                facts.insert(
                    "infiniband".to_string(),
                    serde_json::json!({
                        "present": true,
                        "device_count": ib_devs.len(),
                        "devices": ib_devs,
                    }),
                );
            } else {
                // Fallback: check lspci
                let lspci = run_cmd_opt(
                    connection,
                    "lspci 2>/dev/null | grep -i 'infiniband\\|mellanox'",
                    context,
                )
                .unwrap_or_default();
                let dev_count = lspci.lines().count();
                facts.insert(
                    "infiniband".to_string(),
                    serde_json::json!({
                        "present": dev_count > 0,
                        "device_count": dev_count,
                        "lspci_devices": lspci.lines().map(|l| l.trim()).collect::<Vec<_>>(),
                    }),
                );
            }
        }

        Ok(ModuleOutput::ok("Gathered HPC facts")
            .with_data("hpc_facts", serde_json::Value::Object(facts)))
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("gather", serde_json::json!(null));
        m
    }
}
