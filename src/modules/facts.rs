//! Facts module - System fact gathering
//!
//! This module gathers facts about the target system including OS, hardware,
//! network, and other system information.
//!
//! ## Remote Facts Gathering
//!
//! Facts can be gathered remotely via SSH or other connections using the async
//! `gather_facts_via_connection` function. This executes commands on the remote
//! host instead of locally.

use super::{Module, ModuleContext, ModuleOutput, ModuleParams, ModuleResult, ParamExt};
use crate::connection::Connection;
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::Arc;
use tracing::debug;

/// Module for gathering system facts
pub struct FactsModule;

impl FactsModule {
    fn gather_os_facts() -> HashMap<String, serde_json::Value> {
        let mut facts = HashMap::new();

        // Get hostname
        if let Ok(output) = Command::new("hostname").arg("-f").output() {
            if output.status.success() {
                let hostname = String::from_utf8_lossy(&output.stdout).trim().to_string();
                facts.insert("hostname".to_string(), serde_json::json!(hostname));

                // Also get short hostname
                if let Some(short) = hostname.split('.').next() {
                    facts.insert("hostname_short".to_string(), serde_json::json!(short));
                }
            }
        }

        // Get kernel info via uname
        if let Ok(output) = Command::new("uname").arg("-s").output() {
            if output.status.success() {
                facts.insert(
                    "system".to_string(),
                    serde_json::json!(String::from_utf8_lossy(&output.stdout).trim()),
                );
            }
        }

        if let Ok(output) = Command::new("uname").arg("-r").output() {
            if output.status.success() {
                facts.insert(
                    "kernel".to_string(),
                    serde_json::json!(String::from_utf8_lossy(&output.stdout).trim()),
                );
            }
        }

        if let Ok(output) = Command::new("uname").arg("-m").output() {
            if output.status.success() {
                let arch = String::from_utf8_lossy(&output.stdout).trim().to_string();
                facts.insert("architecture".to_string(), serde_json::json!(arch));

                // Map to common architecture names
                let machine = match arch.as_str() {
                    "x86_64" | "amd64" => "x86_64",
                    "aarch64" | "arm64" => "aarch64",
                    "armv7l" => "armv7l",
                    "i686" | "i386" => "i386",
                    _ => &arch,
                };
                facts.insert("machine".to_string(), serde_json::json!(machine));
            }
        }

        // Get OS release info
        if let Ok(content) = fs::read_to_string("/etc/os-release") {
            for line in content.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    let value = value.trim_matches('"');
                    match key {
                        "ID" => {
                            facts.insert("distribution".to_string(), serde_json::json!(value));
                        }
                        "VERSION_ID" => {
                            facts.insert(
                                "distribution_version".to_string(),
                                serde_json::json!(value),
                            );
                        }
                        "ID_LIKE" => {
                            facts.insert("os_family".to_string(), serde_json::json!(value));
                        }
                        "PRETTY_NAME" => {
                            facts.insert(
                                "distribution_pretty_name".to_string(),
                                serde_json::json!(value),
                            );
                        }
                        "VERSION_CODENAME" => {
                            facts.insert(
                                "distribution_codename".to_string(),
                                serde_json::json!(value),
                            );
                        }
                        _ => {}
                    }
                }
            }
        }

        // Determine OS family if not set
        if !facts.contains_key("os_family") {
            if let Some(serde_json::Value::String(distro)) = facts.get("distribution") {
                let family = match distro.to_lowercase().as_str() {
                    "ubuntu" | "debian" | "linuxmint" | "pop" | "elementary" => "debian",
                    "fedora" | "centos" | "rhel" | "rocky" | "alma" | "oracle" => "redhat",
                    "arch" | "manjaro" | "endeavouros" => "arch",
                    "opensuse" | "sles" => "suse",
                    "alpine" => "alpine",
                    "gentoo" => "gentoo",
                    _ => "unknown",
                };
                facts.insert("os_family".to_string(), serde_json::json!(family));
            }
        }

        // Get current user
        if let Ok(output) = Command::new("whoami").output() {
            if output.status.success() {
                facts.insert(
                    "user_id".to_string(),
                    serde_json::json!(String::from_utf8_lossy(&output.stdout).trim()),
                );
            }
        }

        // Get user's UID
        if let Ok(output) = Command::new("id").arg("-u").output() {
            if output.status.success() {
                if let Ok(uid) = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                {
                    facts.insert("user_uid".to_string(), serde_json::json!(uid));
                }
            }
        }

        // Get user's GID
        if let Ok(output) = Command::new("id").arg("-g").output() {
            if output.status.success() {
                if let Ok(gid) = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u32>()
                {
                    facts.insert("user_gid".to_string(), serde_json::json!(gid));
                }
            }
        }

        facts
    }

    fn gather_hardware_facts() -> HashMap<String, serde_json::Value> {
        let mut facts = HashMap::new();

        // Get CPU info
        if let Ok(content) = fs::read_to_string("/proc/cpuinfo") {
            let mut processor_count = 0;
            let mut model_name = String::new();
            let mut cpu_cores = 0;

            for line in content.lines() {
                if line.starts_with("processor") {
                    processor_count += 1;
                } else if line.starts_with("model name") {
                    if let Some((_, value)) = line.split_once(':') {
                        model_name = value.trim().to_string();
                    }
                } else if line.starts_with("cpu cores") {
                    if let Some((_, value)) = line.split_once(':') {
                        cpu_cores = value.trim().parse().unwrap_or(0);
                    }
                }
            }

            facts.insert(
                "processor_count".to_string(),
                serde_json::json!(processor_count),
            );
            if !model_name.is_empty() {
                facts.insert("processor".to_string(), serde_json::json!(model_name));
            }
            if cpu_cores > 0 {
                facts.insert("processor_cores".to_string(), serde_json::json!(cpu_cores));
            }
        }

        // Get memory info
        if let Ok(content) = fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            facts.insert("memtotal_mb".to_string(), serde_json::json!(kb / 1024));
                        }
                    }
                } else if line.starts_with("MemFree:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            facts.insert("memfree_mb".to_string(), serde_json::json!(kb / 1024));
                        }
                    }
                } else if line.starts_with("SwapTotal:") {
                    if let Some(kb_str) = line.split_whitespace().nth(1) {
                        if let Ok(kb) = kb_str.parse::<u64>() {
                            facts.insert("swaptotal_mb".to_string(), serde_json::json!(kb / 1024));
                        }
                    }
                }
            }
        }

        // Get disk info - root filesystem
        if let Ok(output) = Command::new("df").args(["-B1", "/"]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(line) = stdout.lines().nth(1) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        if let Ok(total) = parts[1].parse::<u64>() {
                            facts.insert("disk_total_bytes".to_string(), serde_json::json!(total));
                        }
                        if let Ok(used) = parts[2].parse::<u64>() {
                            facts.insert("disk_used_bytes".to_string(), serde_json::json!(used));
                        }
                        if let Ok(avail) = parts[3].parse::<u64>() {
                            facts.insert(
                                "disk_available_bytes".to_string(),
                                serde_json::json!(avail),
                            );
                        }
                    }
                }
            }
        }

        facts
    }

    fn gather_network_facts() -> HashMap<String, serde_json::Value> {
        let mut facts = HashMap::new();
        let mut interfaces: Vec<serde_json::Value> = Vec::new();

        // Get network interfaces
        if let Ok(entries) = fs::read_dir("/sys/class/net") {
            for entry in entries.filter_map(|e| e.ok()) {
                let iface_name = entry.file_name().to_string_lossy().to_string();

                // Skip loopback
                if iface_name == "lo" {
                    continue;
                }

                let mut iface_info = serde_json::Map::new();
                iface_info.insert("device".to_string(), serde_json::json!(iface_name.clone()));

                // Get MAC address
                let mac_path = entry.path().join("address");
                if let Ok(mac) = fs::read_to_string(&mac_path) {
                    let mac = mac.trim();
                    if mac != "00:00:00:00:00:00" {
                        iface_info.insert("macaddress".to_string(), serde_json::json!(mac));
                    }
                }

                // Get MTU
                let mtu_path = entry.path().join("mtu");
                if let Ok(mtu) = fs::read_to_string(&mtu_path) {
                    if let Ok(mtu) = mtu.trim().parse::<u32>() {
                        iface_info.insert("mtu".to_string(), serde_json::json!(mtu));
                    }
                }

                // Get operstate
                let state_path = entry.path().join("operstate");
                if let Ok(state) = fs::read_to_string(&state_path) {
                    iface_info.insert(
                        "active".to_string(),
                        serde_json::json!(state.trim() == "up"),
                    );
                }

                interfaces.push(serde_json::Value::Object(iface_info));
            }
        }

        facts.insert("interfaces".to_string(), serde_json::json!(interfaces));

        // Get default IPv4 address
        if let Ok(output) = Command::new("ip")
            .args(["route", "get", "1.1.1.1"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for part in stdout.split_whitespace() {
                    // Look for src keyword followed by IP
                    if part == "src" {
                        if let Some(ip) = stdout.split("src ").nth(1) {
                            if let Some(ip) = ip.split_whitespace().next() {
                                facts.insert("default_ipv4".to_string(), serde_json::json!(ip));
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Get FQDN
        if let Ok(output) = Command::new("hostname").arg("-f").output() {
            if output.status.success() {
                facts.insert(
                    "fqdn".to_string(),
                    serde_json::json!(String::from_utf8_lossy(&output.stdout).trim()),
                );
            }
        }

        facts
    }

    fn gather_date_facts() -> HashMap<String, serde_json::Value> {
        let mut facts = HashMap::new();

        // Get current date/time info
        if let Ok(output) = Command::new("date").arg("+%Y-%m-%d %H:%M:%S %Z").output() {
            if output.status.success() {
                facts.insert(
                    "date_time".to_string(),
                    serde_json::json!(String::from_utf8_lossy(&output.stdout).trim()),
                );
            }
        }

        // Get epoch
        if let Ok(output) = Command::new("date").arg("+%s").output() {
            if output.status.success() {
                if let Ok(epoch) = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<u64>()
                {
                    facts.insert("epoch".to_string(), serde_json::json!(epoch));
                }
            }
        }

        // Get timezone
        if let Ok(tz) = fs::read_to_string("/etc/timezone") {
            facts.insert("timezone".to_string(), serde_json::json!(tz.trim()));
        } else if let Ok(link) = fs::read_link("/etc/localtime") {
            // Extract timezone from symlink path
            let path = link.to_string_lossy();
            if let Some(tz) = path.strip_prefix("/usr/share/zoneinfo/") {
                facts.insert("timezone".to_string(), serde_json::json!(tz));
            }
        }

        // Get uptime
        if let Ok(content) = fs::read_to_string("/proc/uptime") {
            if let Some(seconds_str) = content.split_whitespace().next() {
                if let Ok(seconds) = seconds_str.parse::<f64>() {
                    facts.insert(
                        "uptime_seconds".to_string(),
                        serde_json::json!(seconds as u64),
                    );
                }
            }
        }

        facts
    }

    fn gather_env_facts() -> HashMap<String, serde_json::Value> {
        let mut facts = HashMap::new();
        let mut env_vars = serde_json::Map::new();

        // Get important environment variables
        for (key, value) in std::env::vars() {
            match key.as_str() {
                "PATH" | "HOME" | "USER" | "SHELL" | "LANG" | "LC_ALL" | "TERM" | "PWD" => {
                    env_vars.insert(key, serde_json::json!(value));
                }
                _ => {}
            }
        }

        facts.insert("env".to_string(), serde_json::Value::Object(env_vars));

        // Get Python version if available
        if let Ok(output) = Command::new("python3").arg("--version").output() {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout);
                if let Some(ver) = version.strip_prefix("Python ") {
                    facts.insert("python_version".to_string(), serde_json::json!(ver.trim()));
                }
            }
        }

        facts
    }
}

// ============================================================================
// Remote Facts Gathering via Connection
// ============================================================================

/// Gather facts from a remote host via a Connection.
///
/// This function executes commands on the remote host using the provided
/// connection and parses the output to build a facts map.
///
/// # Arguments
///
/// * `connection` - The connection to use for remote execution
/// * `gather_subset` - Optional list of fact categories to gather ("all", "os", "hardware", etc.)
///
/// # Returns
///
/// A map of fact names to their values, similar to local facts gathering.
pub async fn gather_facts_via_connection(
    connection: &Arc<dyn Connection + Send + Sync>,
    gather_subset: Option<&[String]>,
) -> HashMap<String, serde_json::Value> {
    let gather_all = gather_subset
        .map(|s| s.iter().any(|x| x == "all"))
        .unwrap_or(true);

    let mut all_facts = HashMap::new();

    // Always gather OS facts (or if specifically requested)
    if gather_all
        || gather_subset
            .map(|s| s.iter().any(|x| x == "os" || x == "min"))
            .unwrap_or(false)
    {
        let os_facts = gather_os_facts_remote(connection).await;
        for (k, v) in os_facts {
            all_facts.insert(k, v);
        }
    }

    // Gather hardware facts
    if gather_all
        || gather_subset
            .map(|s| s.iter().any(|x| x == "hardware"))
            .unwrap_or(false)
    {
        let hw_facts = gather_hardware_facts_remote(connection).await;
        for (k, v) in hw_facts {
            all_facts.insert(k, v);
        }
    }

    // Gather network facts
    if gather_all
        || gather_subset
            .map(|s| s.iter().any(|x| x == "network"))
            .unwrap_or(false)
    {
        let net_facts = gather_network_facts_remote(connection).await;
        for (k, v) in net_facts {
            all_facts.insert(k, v);
        }
    }

    // Gather date/time facts
    if gather_all
        || gather_subset
            .map(|s| s.iter().any(|x| x == "date_time"))
            .unwrap_or(false)
    {
        let date_facts = gather_date_facts_remote(connection).await;
        for (k, v) in date_facts {
            all_facts.insert(k, v);
        }
    }

    // Gather environment facts
    if gather_all
        || gather_subset
            .map(|s| s.iter().any(|x| x == "env"))
            .unwrap_or(false)
    {
        let env_facts = gather_env_facts_remote(connection).await;
        for (k, v) in env_facts {
            all_facts.insert(k, v);
        }
    }

    all_facts
}

/// Helper to execute a command and get stdout if successful
async fn execute_and_get_output(
    connection: &Arc<dyn Connection + Send + Sync>,
    command: &str,
) -> Option<String> {
    match connection.execute(command, None).await {
        Ok(result) if result.success => Some(result.stdout.trim().to_string()),
        Ok(result) => {
            debug!(
                "Command '{}' failed with exit code {}: {}",
                command, result.exit_code, result.stderr
            );
            None
        }
        Err(e) => {
            debug!("Command '{}' execution error: {}", command, e);
            None
        }
    }
}

/// Helper to read a remote file's content
async fn read_remote_file(
    connection: &Arc<dyn Connection + Send + Sync>,
    path: &str,
) -> Option<String> {
    // Use cat to read file content via the connection
    execute_and_get_output(connection, &format!("cat {}", path)).await
}

/// Gather OS facts from remote host
async fn gather_os_facts_remote(
    connection: &Arc<dyn Connection + Send + Sync>,
) -> HashMap<String, serde_json::Value> {
    let mut facts = HashMap::new();

    // Get hostname
    if let Some(hostname) = execute_and_get_output(connection, "hostname -f").await {
        facts.insert("hostname".to_string(), serde_json::json!(hostname));
        if let Some(short) = hostname.split('.').next() {
            facts.insert("hostname_short".to_string(), serde_json::json!(short));
        }
    }

    // Get kernel info via uname
    if let Some(system) = execute_and_get_output(connection, "uname -s").await {
        facts.insert("system".to_string(), serde_json::json!(system));
    }

    if let Some(kernel) = execute_and_get_output(connection, "uname -r").await {
        facts.insert("kernel".to_string(), serde_json::json!(kernel));
    }

    if let Some(arch) = execute_and_get_output(connection, "uname -m").await {
        facts.insert("architecture".to_string(), serde_json::json!(&arch));

        // Map to common architecture names
        let machine = match arch.as_str() {
            "x86_64" | "amd64" => "x86_64",
            "aarch64" | "arm64" => "aarch64",
            "armv7l" => "armv7l",
            "i686" | "i386" => "i386",
            _ => &arch,
        };
        facts.insert("machine".to_string(), serde_json::json!(machine));
    }

    // Get OS release info
    if let Some(content) = read_remote_file(connection, "/etc/os-release").await {
        for line in content.lines() {
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim_matches('"');
                match key {
                    "ID" => {
                        facts.insert("distribution".to_string(), serde_json::json!(value));
                    }
                    "VERSION_ID" => {
                        facts.insert("distribution_version".to_string(), serde_json::json!(value));
                    }
                    "ID_LIKE" => {
                        facts.insert("os_family".to_string(), serde_json::json!(value));
                    }
                    "PRETTY_NAME" => {
                        facts.insert(
                            "distribution_pretty_name".to_string(),
                            serde_json::json!(value),
                        );
                    }
                    "VERSION_CODENAME" => {
                        facts.insert(
                            "distribution_codename".to_string(),
                            serde_json::json!(value),
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    // Determine OS family if not set
    if !facts.contains_key("os_family") {
        if let Some(serde_json::Value::String(distro)) = facts.get("distribution") {
            let family = match distro.to_lowercase().as_str() {
                "ubuntu" | "debian" | "linuxmint" | "pop" | "elementary" => "debian",
                "fedora" | "centos" | "rhel" | "rocky" | "alma" | "oracle" => "redhat",
                "arch" | "manjaro" | "endeavouros" => "arch",
                "opensuse" | "sles" => "suse",
                "alpine" => "alpine",
                "gentoo" => "gentoo",
                _ => "unknown",
            };
            facts.insert("os_family".to_string(), serde_json::json!(family));
        }
    }

    // Get current user
    if let Some(user) = execute_and_get_output(connection, "whoami").await {
        facts.insert("user_id".to_string(), serde_json::json!(user));
    }

    // Get user's UID
    if let Some(uid_str) = execute_and_get_output(connection, "id -u").await {
        if let Ok(uid) = uid_str.parse::<u32>() {
            facts.insert("user_uid".to_string(), serde_json::json!(uid));
        }
    }

    // Get user's GID
    if let Some(gid_str) = execute_and_get_output(connection, "id -g").await {
        if let Ok(gid) = gid_str.parse::<u32>() {
            facts.insert("user_gid".to_string(), serde_json::json!(gid));
        }
    }

    facts
}

/// Gather hardware facts from remote host
async fn gather_hardware_facts_remote(
    connection: &Arc<dyn Connection + Send + Sync>,
) -> HashMap<String, serde_json::Value> {
    let mut facts = HashMap::new();

    // Get CPU info
    if let Some(content) = read_remote_file(connection, "/proc/cpuinfo").await {
        let mut processor_count = 0;
        let mut model_name = String::new();
        let mut cpu_cores = 0;

        for line in content.lines() {
            if line.starts_with("processor") {
                processor_count += 1;
            } else if line.starts_with("model name") {
                if let Some((_, value)) = line.split_once(':') {
                    model_name = value.trim().to_string();
                }
            } else if line.starts_with("cpu cores") {
                if let Some((_, value)) = line.split_once(':') {
                    cpu_cores = value.trim().parse().unwrap_or(0);
                }
            }
        }

        facts.insert(
            "processor_count".to_string(),
            serde_json::json!(processor_count),
        );
        if !model_name.is_empty() {
            facts.insert("processor".to_string(), serde_json::json!(model_name));
        }
        if cpu_cores > 0 {
            facts.insert("processor_cores".to_string(), serde_json::json!(cpu_cores));
        }
    }

    // Get memory info
    if let Some(content) = read_remote_file(connection, "/proc/meminfo").await {
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<u64>() {
                        facts.insert("memtotal_mb".to_string(), serde_json::json!(kb / 1024));
                    }
                }
            } else if line.starts_with("MemFree:") {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<u64>() {
                        facts.insert("memfree_mb".to_string(), serde_json::json!(kb / 1024));
                    }
                }
            } else if line.starts_with("SwapTotal:") {
                if let Some(kb_str) = line.split_whitespace().nth(1) {
                    if let Ok(kb) = kb_str.parse::<u64>() {
                        facts.insert("swaptotal_mb".to_string(), serde_json::json!(kb / 1024));
                    }
                }
            }
        }
    }

    // Get disk info - root filesystem
    if let Some(stdout) = execute_and_get_output(connection, "df -B1 /").await {
        if let Some(line) = stdout.lines().nth(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                if let Ok(total) = parts[1].parse::<u64>() {
                    facts.insert("disk_total_bytes".to_string(), serde_json::json!(total));
                }
                if let Ok(used) = parts[2].parse::<u64>() {
                    facts.insert("disk_used_bytes".to_string(), serde_json::json!(used));
                }
                if let Ok(avail) = parts[3].parse::<u64>() {
                    facts.insert("disk_available_bytes".to_string(), serde_json::json!(avail));
                }
            }
        }
    }

    facts
}

/// Gather network facts from remote host
async fn gather_network_facts_remote(
    connection: &Arc<dyn Connection + Send + Sync>,
) -> HashMap<String, serde_json::Value> {
    let mut facts = HashMap::new();
    let mut interfaces: Vec<serde_json::Value> = Vec::new();

    // Get network interfaces by listing /sys/class/net
    if let Some(iface_list) =
        execute_and_get_output(connection, "ls -1 /sys/class/net 2>/dev/null").await
    {
        for iface_name in iface_list.lines() {
            let iface_name = iface_name.trim();

            // Skip loopback
            if iface_name == "lo" || iface_name.is_empty() {
                continue;
            }

            let mut iface_info = serde_json::Map::new();
            iface_info.insert(
                "device".to_string(),
                serde_json::json!(iface_name.to_string()),
            );

            // Get MAC address
            if let Some(mac) = read_remote_file(
                connection,
                &format!("/sys/class/net/{}/address", iface_name),
            )
            .await
            {
                let mac = mac.trim();
                if mac != "00:00:00:00:00:00" {
                    iface_info.insert("macaddress".to_string(), serde_json::json!(mac));
                }
            }

            // Get MTU
            if let Some(mtu_str) =
                read_remote_file(connection, &format!("/sys/class/net/{}/mtu", iface_name)).await
            {
                if let Ok(mtu) = mtu_str.trim().parse::<u32>() {
                    iface_info.insert("mtu".to_string(), serde_json::json!(mtu));
                }
            }

            // Get operstate
            if let Some(state) = read_remote_file(
                connection,
                &format!("/sys/class/net/{}/operstate", iface_name),
            )
            .await
            {
                iface_info.insert(
                    "active".to_string(),
                    serde_json::json!(state.trim() == "up"),
                );
            }

            interfaces.push(serde_json::Value::Object(iface_info));
        }
    }

    facts.insert("interfaces".to_string(), serde_json::json!(interfaces));

    // Get default IPv4 address
    if let Some(stdout) =
        execute_and_get_output(connection, "ip route get 1.1.1.1 2>/dev/null").await
    {
        if let Some(ip) = stdout.split("src ").nth(1) {
            if let Some(ip) = ip.split_whitespace().next() {
                facts.insert("default_ipv4".to_string(), serde_json::json!(ip));
            }
        }
    }

    // Get FQDN
    if let Some(fqdn) = execute_and_get_output(connection, "hostname -f").await {
        facts.insert("fqdn".to_string(), serde_json::json!(fqdn));
    }

    facts
}

/// Gather date/time facts from remote host
async fn gather_date_facts_remote(
    connection: &Arc<dyn Connection + Send + Sync>,
) -> HashMap<String, serde_json::Value> {
    let mut facts = HashMap::new();

    // Get current date/time info
    if let Some(datetime) = execute_and_get_output(connection, "date '+%Y-%m-%d %H:%M:%S %Z'").await
    {
        facts.insert("date_time".to_string(), serde_json::json!(datetime));
    }

    // Get epoch
    if let Some(epoch_str) = execute_and_get_output(connection, "date +%s").await {
        if let Ok(epoch) = epoch_str.parse::<u64>() {
            facts.insert("epoch".to_string(), serde_json::json!(epoch));
        }
    }

    // Get timezone
    if let Some(tz) = read_remote_file(connection, "/etc/timezone").await {
        facts.insert("timezone".to_string(), serde_json::json!(tz.trim()));
    } else if let Some(link) = execute_and_get_output(connection, "readlink /etc/localtime").await {
        // Extract timezone from symlink path
        if let Some(tz) = link.strip_prefix("/usr/share/zoneinfo/") {
            facts.insert("timezone".to_string(), serde_json::json!(tz));
        }
    }

    // Get uptime
    if let Some(content) = read_remote_file(connection, "/proc/uptime").await {
        if let Some(seconds_str) = content.split_whitespace().next() {
            if let Ok(seconds) = seconds_str.parse::<f64>() {
                facts.insert(
                    "uptime_seconds".to_string(),
                    serde_json::json!(seconds as u64),
                );
            }
        }
    }

    facts
}

/// Gather environment facts from remote host
async fn gather_env_facts_remote(
    connection: &Arc<dyn Connection + Send + Sync>,
) -> HashMap<String, serde_json::Value> {
    let mut facts = HashMap::new();
    let mut env_vars = serde_json::Map::new();

    // Get important environment variables using printenv
    let env_cmd = "printenv PATH HOME USER SHELL LANG LC_ALL TERM PWD 2>/dev/null || echo ''";
    if let Some(env_output) = execute_and_get_output(connection, env_cmd).await {
        for line in env_output.lines() {
            if let Some((key, value)) = line.split_once('=') {
                env_vars.insert(key.to_string(), serde_json::json!(value));
            }
        }
    }

    facts.insert("env".to_string(), serde_json::Value::Object(env_vars));

    // Get Python version if available
    if let Some(version_output) = execute_and_get_output(connection, "python3 --version 2>&1").await
    {
        if let Some(ver) = version_output.strip_prefix("Python ") {
            facts.insert("python_version".to_string(), serde_json::json!(ver.trim()));
        }
    }

    facts
}

impl Module for FactsModule {
    fn name(&self) -> &'static str {
        "gather_facts"
    }

    fn description(&self) -> &'static str {
        "Gather facts about the target system"
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let gather_subset = params
            .get_vec_string("gather_subset")?
            .unwrap_or_else(|| vec!["all".to_string()]);

        let gather_all = gather_subset.contains(&"all".to_string());

        let mut all_facts = HashMap::new();

        // Always gather OS facts
        if gather_all
            || gather_subset.contains(&"os".to_string())
            || gather_subset.contains(&"min".to_string())
        {
            for (k, v) in Self::gather_os_facts() {
                all_facts.insert(k, v);
            }
        }

        // Gather hardware facts
        if gather_all || gather_subset.contains(&"hardware".to_string()) {
            for (k, v) in Self::gather_hardware_facts() {
                all_facts.insert(k, v);
            }
        }

        // Gather network facts
        if gather_all || gather_subset.contains(&"network".to_string()) {
            for (k, v) in Self::gather_network_facts() {
                all_facts.insert(k, v);
            }
        }

        // Gather date/time facts
        if gather_all || gather_subset.contains(&"date_time".to_string()) {
            for (k, v) in Self::gather_date_facts() {
                all_facts.insert(k, v);
            }
        }

        // Gather environment facts
        if gather_all || gather_subset.contains(&"env".to_string()) {
            for (k, v) in Self::gather_env_facts() {
                all_facts.insert(k, v);
            }
        }

        // Convert to serde_json::Value
        let facts_json: serde_json::Map<String, serde_json::Value> =
            all_facts.into_iter().collect();

        let _ = context;

        Ok(ModuleOutput::ok("Facts gathered successfully")
            .with_data("ansible_facts", serde_json::Value::Object(facts_json)))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gather_os_facts() {
        let facts = FactsModule::gather_os_facts();

        // Should always have some OS facts on Linux
        assert!(facts.contains_key("system") || facts.contains_key("hostname"));
    }

    #[test]
    fn test_gather_hardware_facts() {
        let facts = FactsModule::gather_hardware_facts();

        // Should have processor count on Linux
        if std::path::Path::new("/proc/cpuinfo").exists() {
            assert!(facts.contains_key("processor_count"));
        }
    }

    #[test]
    fn test_gather_network_facts() {
        let facts = FactsModule::gather_network_facts();

        // Should have interfaces on Linux
        if std::path::Path::new("/sys/class/net").exists() {
            assert!(facts.contains_key("interfaces"));
        }
    }

    #[test]
    fn test_facts_module_execute() {
        let module = FactsModule;
        let params: ModuleParams = HashMap::new();
        let context = ModuleContext::default();

        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("ansible_facts"));
    }

    #[test]
    fn test_facts_module_with_subset() {
        let module = FactsModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "gather_subset".to_string(),
            serde_json::json!(["os", "hardware"]),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("ansible_facts"));
    }

    // ========================================================================
    // Remote Facts Gathering Tests
    // ========================================================================

    #[tokio::test]
    async fn test_gather_facts_via_connection_all() {
        use crate::connection::local::LocalConnection;

        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let facts = gather_facts_via_connection(&conn, None).await;

        // Should have OS facts
        assert!(
            facts.contains_key("system") || facts.contains_key("hostname"),
            "Expected OS facts"
        );

        // Should have hardware facts if /proc exists
        if std::path::Path::new("/proc/cpuinfo").exists() {
            assert!(
                facts.contains_key("processor_count"),
                "Expected processor_count fact"
            );
        }
    }

    #[tokio::test]
    async fn test_gather_facts_via_connection_os_subset() {
        use crate::connection::local::LocalConnection;

        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let subset = vec!["os".to_string()];
        let facts = gather_facts_via_connection(&conn, Some(&subset)).await;

        // Should have OS facts
        assert!(
            facts.contains_key("system") || facts.contains_key("hostname"),
            "Expected OS facts with 'os' subset"
        );

        // Should NOT have hardware-specific facts when only 'os' is requested
        // Note: processor_count is a hardware fact, not an OS fact
    }

    #[tokio::test]
    async fn test_gather_facts_via_connection_hardware_subset() {
        use crate::connection::local::LocalConnection;

        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let subset = vec!["hardware".to_string()];
        let facts = gather_facts_via_connection(&conn, Some(&subset)).await;

        // Should have hardware facts if /proc exists
        if std::path::Path::new("/proc/cpuinfo").exists() {
            assert!(
                facts.contains_key("processor_count"),
                "Expected hardware facts with 'hardware' subset"
            );
        }
    }

    #[tokio::test]
    async fn test_gather_os_facts_remote() {
        use crate::connection::local::LocalConnection;

        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let facts = gather_os_facts_remote(&conn).await;

        // Should get hostname
        assert!(
            facts.contains_key("hostname") || facts.contains_key("system"),
            "Expected hostname or system fact"
        );

        // Should get user info
        assert!(facts.contains_key("user_id"), "Expected user_id fact");
    }

    #[tokio::test]
    async fn test_gather_hardware_facts_remote() {
        use crate::connection::local::LocalConnection;

        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let facts = gather_hardware_facts_remote(&conn).await;

        // Should have processor info on Linux
        if std::path::Path::new("/proc/cpuinfo").exists() {
            assert!(
                facts.contains_key("processor_count"),
                "Expected processor_count"
            );
        }

        // Should have memory info on Linux
        if std::path::Path::new("/proc/meminfo").exists() {
            assert!(facts.contains_key("memtotal_mb"), "Expected memtotal_mb");
        }
    }

    #[tokio::test]
    async fn test_gather_network_facts_remote() {
        use crate::connection::local::LocalConnection;

        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let facts = gather_network_facts_remote(&conn).await;

        // Should have interfaces list
        assert!(facts.contains_key("interfaces"), "Expected interfaces fact");
    }

    #[tokio::test]
    async fn test_gather_date_facts_remote() {
        use crate::connection::local::LocalConnection;

        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let facts = gather_date_facts_remote(&conn).await;

        // Should have date/time info
        assert!(facts.contains_key("date_time"), "Expected date_time fact");
        assert!(facts.contains_key("epoch"), "Expected epoch fact");
    }

    #[tokio::test]
    async fn test_gather_env_facts_remote() {
        use crate::connection::local::LocalConnection;

        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let facts = gather_env_facts_remote(&conn).await;

        // Should have env fact
        assert!(facts.contains_key("env"), "Expected env fact");
    }
}
