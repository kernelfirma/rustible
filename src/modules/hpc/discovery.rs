//! HPC hardware discovery module
//!
//! Discovers and inventories bare-metal hardware on HPC nodes by parsing
//! `/proc/cpuinfo`, `lspci`, `ip link`, `lsblk`, and BMC address detection.
//! Returns a structured `HardwareInventory` with CPU, GPU, NIC, storage,
//! and memory information.
//!
//! # Parameters
//!
//! - `gather` (optional): List of categories to gather.
//!   Values: "cpu", "gpu", "nic", "storage", "memory", "bmc"
//!   Default: all categories

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use serde::{Deserialize, Serialize};

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

/// Structured hardware inventory of an HPC node.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareInventory {
    pub cpu_count: u32,
    pub cpu_model: String,
    pub gpu_count: u32,
    pub gpu_models: Vec<String>,
    pub nic_count: u32,
    pub nics: Vec<NicInfo>,
    pub storage_devices: Vec<StorageDevice>,
    pub bmc_address: Option<String>,
    pub total_memory_mb: u64,
}

/// Network interface information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NicInfo {
    pub name: String,
    pub mac: Option<String>,
    pub state: String,
}

/// Storage device information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageDevice {
    pub name: String,
    pub size: String,
    pub device_type: String,
    pub model: Option<String>,
}

impl HardwareInventory {
    /// Create a new empty inventory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convert to a serde_json::Value for embedding in ModuleOutput data.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "cpu_count": self.cpu_count,
            "cpu_model": self.cpu_model,
            "gpu_count": self.gpu_count,
            "gpu_models": self.gpu_models,
            "nic_count": self.nic_count,
            "nics": self.nics,
            "storage_devices": self.storage_devices,
            "bmc_address": self.bmc_address,
            "total_memory_mb": self.total_memory_mb,
        })
    }
}

/// Parse CPU information from /proc/cpuinfo output.
fn parse_cpuinfo(cpuinfo: &str) -> (u32, String) {
    let cpu_count = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count() as u32;

    let cpu_model = cpuinfo
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    (cpu_count, cpu_model)
}

/// Parse GPU devices from lspci output.
fn parse_gpu_from_lspci(lspci_output: &str) -> Vec<String> {
    lspci_output
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("vga")
                || lower.contains("3d controller")
                || lower.contains("display controller")
        })
        .map(|line| {
            // Extract the device description after the class code
            // Format: "00:02.0 VGA compatible controller: Intel Corporation ..."
            line.split(':')
                .skip(2)
                .collect::<Vec<_>>()
                .join(":")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse network interfaces from `ip -o link show` output.
fn parse_ip_link(output: &str) -> Vec<NicInfo> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            // Format: "2: eth0: <BROADCAST,...> ... state UP ... link/ether aa:bb:cc:dd:ee:ff ..."
            let parts: Vec<&str> = line.splitn(3, ':').collect();
            if parts.len() < 3 {
                return None;
            }
            let name = parts[1].trim().trim_end_matches('@').to_string();

            // Skip loopback
            if name == "lo" {
                return None;
            }

            let rest = parts[2];

            let state = if rest.contains("state UP") {
                "UP".to_string()
            } else if rest.contains("state DOWN") {
                "DOWN".to_string()
            } else {
                "UNKNOWN".to_string()
            };

            let mac = rest
                .split("link/ether ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .map(|s| s.to_string());

            Some(NicInfo { name, mac, state })
        })
        .collect()
}

/// Parse storage devices from `lsblk -d -n -o NAME,SIZE,TYPE,MODEL` output.
fn parse_lsblk(output: &str) -> Vec<StorageDevice> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                return None;
            }
            let name = parts[0].to_string();
            let size = parts[1].to_string();
            let device_type = parts[2].to_string();
            let model = if parts.len() > 3 {
                Some(parts[3..].join(" "))
            } else {
                None
            };

            Some(StorageDevice {
                name,
                size,
                device_type,
                model,
            })
        })
        .collect()
}

/// Parse total memory in MB from /proc/meminfo output.
fn parse_meminfo(meminfo: &str) -> u64 {
    meminfo
        .lines()
        .find(|l| l.starts_with("MemTotal:"))
        .and_then(|l| {
            l.split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u64>().ok())
        })
        .map(|kb| kb / 1024)
        .unwrap_or(0)
}

/// Parse BMC IP address from `ipmitool lan print` output.
fn parse_bmc_address(output: &str) -> Option<String> {
    output
        .lines()
        .find(|l| l.contains("IP Address") && !l.contains("Source"))
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "0.0.0.0")
}

pub struct HpcDiscoveryModule;

impl Module for HpcDiscoveryModule {
    fn name(&self) -> &'static str {
        "hpc_discovery"
    }

    fn description(&self) -> &'static str {
        "Discover and inventory bare-metal hardware (CPU, GPU, NIC, storage, BMC)"
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
            return Ok(ModuleOutput::ok("Would discover hardware inventory"));
        }

        let gather = params.get_vec_string("gather")?;
        let should_gather = |cat: &str| -> bool {
            match &gather {
                Some(cats) => cats.iter().any(|c| c == cat),
                None => true,
            }
        };

        let mut inventory = HardwareInventory::new();

        // --- CPU discovery ---
        if should_gather("cpu") {
            if let Some(cpuinfo) = run_cmd_opt(connection, "cat /proc/cpuinfo", context) {
                let (count, model) = parse_cpuinfo(&cpuinfo);
                inventory.cpu_count = count;
                inventory.cpu_model = model;
            }
        }

        // --- GPU discovery ---
        if should_gather("gpu") {
            // Try nvidia-smi first for more detail
            let nvidia_output = run_cmd_opt(
                connection,
                "nvidia-smi --query-gpu=gpu_name --format=csv,noheader 2>/dev/null",
                context,
            );
            if let Some(output) = nvidia_output {
                let models: Vec<String> = output
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                inventory.gpu_count = models.len() as u32;
                inventory.gpu_models = models;
            } else if let Some(lspci) = run_cmd_opt(connection, "lspci 2>/dev/null", context) {
                let models = parse_gpu_from_lspci(&lspci);
                inventory.gpu_count = models.len() as u32;
                inventory.gpu_models = models;
            }
        }

        // --- NIC discovery ---
        if should_gather("nic") {
            if let Some(ip_output) =
                run_cmd_opt(connection, "ip -o link show 2>/dev/null", context)
            {
                let nics = parse_ip_link(&ip_output);
                inventory.nic_count = nics.len() as u32;
                inventory.nics = nics;
            }
        }

        // --- Storage discovery ---
        if should_gather("storage") {
            if let Some(lsblk_output) = run_cmd_opt(
                connection,
                "lsblk -d -n -o NAME,SIZE,TYPE,MODEL 2>/dev/null",
                context,
            ) {
                inventory.storage_devices = parse_lsblk(&lsblk_output);
            }
        }

        // --- Memory discovery ---
        if should_gather("memory") {
            if let Some(meminfo) = run_cmd_opt(connection, "cat /proc/meminfo", context) {
                inventory.total_memory_mb = parse_meminfo(&meminfo);
            }
        }

        // --- BMC address discovery ---
        if should_gather("bmc") {
            if let Some(lan_output) = run_cmd_opt(
                connection,
                "ipmitool lan print 2>/dev/null",
                context,
            ) {
                inventory.bmc_address = parse_bmc_address(&lan_output);
            }
        }

        Ok(ModuleOutput::ok("Hardware inventory collected")
            .with_data("inventory", inventory.to_json()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpuinfo() {
        let cpuinfo = "\
processor\t: 0
vendor_id\t: GenuineIntel
model name\t: Intel(R) Xeon(R) Gold 6248 CPU @ 2.50GHz
cpu MHz\t\t: 2500.000
flags\t\t: fpu vme de pse tsc

processor\t: 1
vendor_id\t: GenuineIntel
model name\t: Intel(R) Xeon(R) Gold 6248 CPU @ 2.50GHz
cpu MHz\t\t: 2500.000
flags\t\t: fpu vme de pse tsc
";
        let (count, model) = parse_cpuinfo(cpuinfo);
        assert_eq!(count, 2);
        assert_eq!(model, "Intel(R) Xeon(R) Gold 6248 CPU @ 2.50GHz");
    }

    #[test]
    fn test_parse_cpuinfo_empty() {
        let (count, model) = parse_cpuinfo("");
        assert_eq!(count, 0);
        assert_eq!(model, "");
    }

    #[test]
    fn test_parse_gpu_from_lspci() {
        let lspci = "\
00:02.0 VGA compatible controller: Intel Corporation HD Graphics 530
3b:00.0 3D controller: NVIDIA Corporation Tesla V100 SXM2
86:00.0 3D controller: NVIDIA Corporation Tesla V100 SXM2
af:00.0 Network controller: Mellanox Technologies ConnectX-6
";
        let gpus = parse_gpu_from_lspci(lspci);
        assert_eq!(gpus.len(), 3);
        assert!(gpus[0].contains("Intel"));
        assert!(gpus[1].contains("NVIDIA"));
    }

    #[test]
    fn test_parse_gpu_from_lspci_empty() {
        let gpus = parse_gpu_from_lspci("");
        assert!(gpus.is_empty());
    }

    #[test]
    fn test_parse_ip_link() {
        let output = "\
1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN link/loopback 00:00:00:00:00:00
2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc mq state UP link/ether aa:bb:cc:dd:ee:ff brd ff:ff:ff:ff:ff:ff
3: ib0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 2044 qdisc mq state UP link/infiniband 80:00:00:48:fe:80:00:00
4: eth1: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN link/ether 11:22:33:44:55:66 brd ff:ff:ff:ff:ff:ff
";
        let nics = parse_ip_link(output);
        // lo should be filtered out
        assert_eq!(nics.len(), 3);
        assert_eq!(nics[0].name, "eth0");
        assert_eq!(nics[0].state, "UP");
        assert_eq!(nics[0].mac, Some("aa:bb:cc:dd:ee:ff".to_string()));
        assert_eq!(nics[2].name, "eth1");
        assert_eq!(nics[2].state, "DOWN");
    }

    #[test]
    fn test_parse_lsblk() {
        let output = "\
sda   894.3G disk SAMSUNG MZ7LH960
sdb   894.3G disk SAMSUNG MZ7LH960
nvme0n1 1.5T disk Dell NVMe PE8010
";
        let devices = parse_lsblk(output);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].name, "sda");
        assert_eq!(devices[0].size, "894.3G");
        assert_eq!(devices[0].device_type, "disk");
        assert_eq!(
            devices[0].model,
            Some("SAMSUNG MZ7LH960".to_string())
        );
        assert_eq!(devices[2].name, "nvme0n1");
        assert_eq!(
            devices[2].model,
            Some("Dell NVMe PE8010".to_string())
        );
    }

    #[test]
    fn test_parse_lsblk_empty() {
        let devices = parse_lsblk("");
        assert!(devices.is_empty());
    }

    #[test]
    fn test_parse_meminfo() {
        let meminfo = "\
MemTotal:       131788456 kB
MemFree:         1234567 kB
MemAvailable:   98765432 kB
";
        let mb = parse_meminfo(meminfo);
        assert_eq!(mb, 131788456 / 1024);
    }

    #[test]
    fn test_parse_meminfo_empty() {
        assert_eq!(parse_meminfo(""), 0);
    }

    #[test]
    fn test_parse_bmc_address() {
        let output = "\
Set in Progress         : Set Complete
Auth Type Support       : MD5
IP Address Source       : Static Address
IP Address              : 10.0.1.100
Subnet Mask             : 255.255.255.0
MAC Address             : aa:bb:cc:dd:ee:ff
";
        let addr = parse_bmc_address(output);
        assert_eq!(addr, Some("10.0.1.100".to_string()));
    }

    #[test]
    fn test_parse_bmc_address_zero() {
        let output = "\
IP Address              : 0.0.0.0
";
        let addr = parse_bmc_address(output);
        assert_eq!(addr, None);
    }

    #[test]
    fn test_parse_bmc_address_missing() {
        let addr = parse_bmc_address("no relevant data here");
        assert_eq!(addr, None);
    }

    #[test]
    fn test_hardware_inventory_new() {
        let inv = HardwareInventory::new();
        assert_eq!(inv.cpu_count, 0);
        assert_eq!(inv.cpu_model, "");
        assert_eq!(inv.gpu_count, 0);
        assert!(inv.gpu_models.is_empty());
        assert_eq!(inv.nic_count, 0);
        assert!(inv.nics.is_empty());
        assert!(inv.storage_devices.is_empty());
        assert!(inv.bmc_address.is_none());
        assert_eq!(inv.total_memory_mb, 0);
    }

    #[test]
    fn test_hardware_inventory_to_json() {
        let mut inv = HardwareInventory::new();
        inv.cpu_count = 40;
        inv.cpu_model = "Xeon Gold 6248".to_string();
        inv.total_memory_mb = 128000;
        inv.gpu_count = 2;
        inv.gpu_models = vec!["Tesla V100".to_string(), "Tesla V100".to_string()];

        let json = inv.to_json();
        assert_eq!(json["cpu_count"], 40);
        assert_eq!(json["cpu_model"], "Xeon Gold 6248");
        assert_eq!(json["total_memory_mb"], 128000);
        assert_eq!(json["gpu_count"], 2);
    }

    #[test]
    fn test_module_name_and_description() {
        let module = HpcDiscoveryModule;
        assert_eq!(module.name(), "hpc_discovery");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_module_classification() {
        let module = HpcDiscoveryModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_module_optional_params() {
        let module = HpcDiscoveryModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("gather"));
    }

    #[test]
    fn test_nic_info_serde() {
        let nic = NicInfo {
            name: "eth0".to_string(),
            mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
            state: "UP".to_string(),
        };
        let json = serde_json::to_string(&nic).unwrap();
        assert!(json.contains("eth0"));
        assert!(json.contains("aa:bb:cc:dd:ee:ff"));

        let parsed: NicInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "eth0");
    }

    #[test]
    fn test_storage_device_serde() {
        let dev = StorageDevice {
            name: "sda".to_string(),
            size: "500G".to_string(),
            device_type: "disk".to_string(),
            model: Some("Samsung SSD".to_string()),
        };
        let json = serde_json::to_string(&dev).unwrap();
        let parsed: StorageDevice = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "sda");
        assert_eq!(parsed.model, Some("Samsung SSD".to_string()));
    }
}
