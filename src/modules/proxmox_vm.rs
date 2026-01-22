//! Proxmox VM lifecycle module (API-driven, local execution).

use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use reqwest::blocking::Client;
use reqwest::{header, Certificate, Method};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use url::Url;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_REQUESTS_PER_SECOND: u32 = 5;
const DEFAULT_WAIT_TIMEOUT_SECS: u64 = 300;
const DEFAULT_WAIT_INTERVAL_SECS: u64 = 2;

/// Allowed parameters for VM creation (POST /nodes/{node}/qemu)
/// Based on Proxmox VE API documentation
const ALLOWED_CREATE_PARAMS: &[&str] = &[
    // Required
    "vmid",
    // General
    "name",
    "description",
    "tags",
    "pool",
    "onboot",
    "startup",
    "protection",
    "lock",
    // CPU
    "sockets",
    "cores",
    "vcpus",
    "cpu",
    "cpulimit",
    "cpuunits",
    "numa",
    "affinity",
    // Memory
    "memory",
    "balloon",
    "shares",
    // BIOS/Boot
    "bios",
    "boot",
    "bootdisk",
    "efidisk0",
    "tpmstate0",
    // OS
    "ostype",
    "machine",
    "arch",
    // Display
    "vga",
    "spice_enhancements",
    // Cloud-init
    "cicustom",
    "cipassword",
    "citype",
    "ciupgrade",
    "ciuser",
    "ipconfig0",
    "ipconfig1",
    "ipconfig2",
    "ipconfig3",
    "ipconfig4",
    "ipconfig5",
    "ipconfig6",
    "ipconfig7",
    "ipconfig8",
    "ipconfig9",
    "nameserver",
    "searchdomain",
    "sshkeys",
    // Storage (virtio, scsi, ide, sata, etc.)
    "virtio0",
    "virtio1",
    "virtio2",
    "virtio3",
    "virtio4",
    "virtio5",
    "virtio6",
    "virtio7",
    "virtio8",
    "virtio9",
    "virtio10",
    "virtio11",
    "virtio12",
    "virtio13",
    "virtio14",
    "virtio15",
    "scsi0",
    "scsi1",
    "scsi2",
    "scsi3",
    "scsi4",
    "scsi5",
    "scsi6",
    "scsi7",
    "scsi8",
    "scsi9",
    "scsi10",
    "scsi11",
    "scsi12",
    "scsi13",
    "scsi14",
    "scsi15",
    "scsi16",
    "scsi17",
    "scsi18",
    "scsi19",
    "scsi20",
    "scsi21",
    "scsi22",
    "scsi23",
    "scsi24",
    "scsi25",
    "scsi26",
    "scsi27",
    "scsi28",
    "scsi29",
    "scsi30",
    "ide0",
    "ide1",
    "ide2",
    "ide3",
    "sata0",
    "sata1",
    "sata2",
    "sata3",
    "sata4",
    "sata5",
    "scsihw",
    // Network
    "net0",
    "net1",
    "net2",
    "net3",
    "net4",
    "net5",
    "net6",
    "net7",
    "net8",
    "net9",
    "net10",
    "net11",
    "net12",
    "net13",
    "net14",
    "net15",
    // Serial/Parallel
    "serial0",
    "serial1",
    "serial2",
    "serial3",
    "parallel0",
    "parallel1",
    "parallel2",
    // USB
    "usb0",
    "usb1",
    "usb2",
    "usb3",
    "usb4",
    // PCI passthrough
    "hostpci0",
    "hostpci1",
    "hostpci2",
    "hostpci3",
    "hostpci4",
    "hostpci5",
    "hostpci6",
    "hostpci7",
    "hostpci8",
    "hostpci9",
    "hostpci10",
    "hostpci11",
    "hostpci12",
    "hostpci13",
    "hostpci14",
    "hostpci15",
    // Audio
    "audio0",
    // Agent
    "agent",
    // ACPI/APIC
    "acpi",
    "hotplug",
    "localtime",
    "freeze",
    "kvm",
    "tablet",
    // Watchdog
    "watchdog",
    // RNG
    "rng0",
    // NUMA nodes
    "numa0",
    "numa1",
    "numa2",
    "numa3",
    "numa4",
    "numa5",
    "numa6",
    "numa7",
    // Misc
    "args",
    "autostart",
    "cdrom",
    "hookscript",
    "hugepages",
    "ivshmem",
    "keephugepages",
    "keyboard",
    "live-restore",
    "migrate_downtime",
    "migrate_speed",
    "reboot",
    "smbios1",
    "template",
    "unique",
    "vmgenid",
    "vmstatestorage",
    // Import
    "archive",
    "bwlimit",
    "force",
    "start",
    "storage",
];

/// Allowed parameters for VM cloning (POST /nodes/{node}/qemu/{vmid}/clone)
const ALLOWED_CLONE_PARAMS: &[&str] = &[
    "newid", // Required: target VMID
    "name",  // VM name
    "description",
    "pool",     // Resource pool
    "target",   // Target node
    "storage",  // Target storage
    "format",   // Target format (raw, qcow2, vmdk)
    "full",     // Full clone (1) or linked clone (0)
    "snapname", // Source snapshot name
    "bwlimit",  // I/O bandwidth limit
];

/// Allowed parameters for VM deletion (DELETE /nodes/{node}/qemu/{vmid})
const ALLOWED_DELETE_PARAMS: &[&str] = &[
    "purge",                      // Remove from backup jobs and HA
    "destroy-unreferenced-disks", // Remove unreferenced disks
    "skiplock",                   // Ignore lock
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesiredState {
    Status,
    Started,
    Stopped,
    Restarted,
    Present,
    Absent,
    Cloned,
}

impl DesiredState {
    fn from_str(value: &str) -> ModuleResult<Self> {
        match value.to_lowercase().as_str() {
            "status" => Ok(Self::Status),
            "started" | "start" | "running" => Ok(Self::Started),
            "stopped" | "stop" => Ok(Self::Stopped),
            "restarted" | "restart" | "rebooted" | "reboot" => Ok(Self::Restarted),
            "present" | "create" => Ok(Self::Present),
            "absent" | "delete" => Ok(Self::Absent),
            "cloned" | "clone" => Ok(Self::Cloned),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: status, started, stopped, restarted, present, absent, cloned",
                value
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopMethod {
    Shutdown,
    Stop,
}

impl StopMethod {
    fn from_str(value: &str) -> ModuleResult<Self> {
        match value.to_lowercase().as_str() {
            "shutdown" => Ok(Self::Shutdown),
            "stop" => Ok(Self::Stop),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid stop_method '{}'. Valid values: shutdown, stop",
                value
            ))),
        }
    }

    fn endpoint(self) -> &'static str {
        match self {
            Self::Shutdown => "shutdown",
            Self::Stop => "stop",
        }
    }
}

#[derive(Debug, Clone)]
struct WaitConfig {
    enabled: bool,
    timeout_secs: u64,
    interval_secs: u64,
}

impl Default for WaitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_secs: DEFAULT_WAIT_TIMEOUT_SECS,
            interval_secs: DEFAULT_WAIT_INTERVAL_SECS,
        }
    }
}

/// TLS configuration for Proxmox API connections
#[derive(Debug, Clone, Default)]
struct TlsConfig {
    /// Custom server name for TLS SNI and certificate verification.
    /// Use when api_url hostname differs from certificate CN/SAN.
    server_name: Option<String>,
    /// Path to CA certificate file (PEM format) for custom CA trust.
    /// Use for self-signed certificates or private CAs.
    ca_cert_path: Option<String>,
}

#[derive(Debug, Clone)]
struct ProxmoxVmConfig {
    api_base: String,
    token_id: String,
    token_secret: String,
    node: String,
    vmid: u64,
    state: DesiredState,
    stop_method: StopMethod,
    timeout_secs: u64,
    validate_certs: bool,
    wait: WaitConfig,
    /// Validate create/clone/delete params against allowlists
    strict_params: bool,
    /// Desired VM configuration fields for idempotent updates
    config_params: Option<HashMap<String, String>>,
    /// TLS configuration for custom CA and server name
    tls: TlsConfig,
}

impl ProxmoxVmConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let api_url = params.get_string_required("api_url")?;
        let token_id = params.get_string_required("api_token_id")?;
        let token_secret = params.get_string_required("api_token_secret")?;
        let node = params.get_string_required("node")?;
        let vmid = parse_vmid(params)?;

        let state = if let Some(state) = params.get_string("state")? {
            DesiredState::from_str(&state)?
        } else {
            DesiredState::Status
        };

        let stop_method = if let Some(method) = params.get_string("stop_method")? {
            StopMethod::from_str(&method)?
        } else {
            StopMethod::Shutdown
        };

        let timeout_secs = params
            .get_u32("timeout")?
            .map(u64::from)
            .or_else(|| {
                params
                    .get_string("timeout")
                    .ok()
                    .flatten()
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        let validate_certs = params.get_bool_or("validate_certs", true);
        let strict_params = params.get_bool_or("strict_params", true);

        let config_params = if let Some(value) = params.get("config") {
            Some(parse_string_map("config", value)?)
        } else {
            None
        };

        if strict_params {
            if let Some(ref config_params) = config_params {
                validate_config_params(config_params)?;
            }
        }

        // TLS configuration
        let tls_server_name = params.get_string("tls_server_name")?;
        let ca_cert_path = params.get_string("ca_cert_path")?;

        let wait_enabled = params.get_bool_or("wait", false);
        let wait_timeout = params
            .get_u32("wait_timeout")?
            .map(u64::from)
            .unwrap_or(DEFAULT_WAIT_TIMEOUT_SECS);
        let wait_interval = params
            .get_u32("wait_interval")?
            .map(u64::from)
            .unwrap_or(DEFAULT_WAIT_INTERVAL_SECS)
            .max(1); // Minimum 1 second interval

        Ok(Self {
            api_base: normalize_api_base(&api_url)?,
            token_id: token_id.trim().to_string(),
            token_secret: token_secret.trim().to_string(),
            node: node.trim().to_string(),
            vmid,
            state,
            stop_method,
            timeout_secs,
            validate_certs,
            wait: WaitConfig {
                enabled: wait_enabled,
                timeout_secs: wait_timeout,
                interval_secs: wait_interval,
            },
            strict_params,
            config_params,
            tls: TlsConfig {
                server_name: tls_server_name,
                ca_cert_path,
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VmPowerState {
    Running,
    Stopped,
    Unknown(String),
}

impl VmPowerState {
    fn from_str(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "running" => Self::Running,
            "stopped" => Self::Stopped,
            other => Self::Unknown(other.to_string()),
        }
    }

    fn as_str(&self) -> &str {
        match self {
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Unknown(value) => value.as_str(),
        }
    }

    fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    fn is_stopped(&self) -> bool {
        matches!(self, Self::Stopped)
    }
}

#[derive(Debug, Clone)]
struct VmStatus {
    power_state: VmPowerState,
    raw: Value,
}

#[derive(Debug, Clone)]
struct ConfigChange {
    key: String,
    current: Option<String>,
    desired: String,
}

#[derive(Debug, Clone)]
struct VmConfigDiff {
    changes: Vec<ConfigChange>,
    update_params: HashMap<String, String>,
}

impl VmConfigDiff {
    fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    fn to_diff(&self) -> Diff {
        let mut before = Vec::new();
        let mut after = Vec::new();

        for change in &self.changes {
            let current = change.current.as_deref().unwrap_or("(unset)");
            before.push(format!("{}: {}", change.key, current));
            after.push(format!("{}: {}", change.key, change.desired));
        }

        Diff::new(before.join("\n"), after.join("\n"))
    }

    fn to_value(&self) -> Value {
        let items: Vec<Value> = self
            .changes
            .iter()
            .map(|change| {
                json!({
                    "key": change.key,
                    "from": change.current,
                    "to": change.desired,
                })
            })
            .collect();
        Value::Array(items)
    }
}

/// Task completion information from UPID polling
#[derive(Debug, Clone)]
struct TaskInfo {
    /// The UPID string
    upid: String,
    /// Task status: "running", "stopped", etc.
    status: String,
    /// Exit status: "OK", "ERROR", etc. (only set when stopped)
    exitstatus: Option<String>,
    /// Start time (Unix timestamp)
    starttime: Option<i64>,
    /// End time (Unix timestamp)
    endtime: Option<i64>,
    /// Duration in seconds (computed)
    duration_secs: Option<i64>,
    /// Node where task ran
    node: Option<String>,
    /// Task type
    task_type: Option<String>,
}

impl TaskInfo {
    fn from_api_response(upid: &str, data: &Value) -> Self {
        let status = data
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let exitstatus = data
            .get("exitstatus")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let starttime = data.get("starttime").and_then(|v| v.as_i64());
        let endtime = data.get("endtime").and_then(|v| v.as_i64());

        let duration_secs = match (starttime, endtime) {
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        };

        let node = data
            .get("node")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let task_type = data
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            upid: upid.to_string(),
            status,
            exitstatus,
            starttime,
            endtime,
            duration_secs,
            node,
            task_type,
        }
    }

    fn is_complete(&self) -> bool {
        self.status == "stopped"
    }

    fn is_success(&self) -> bool {
        self.is_complete() && self.exitstatus.as_deref() == Some("OK")
    }

    fn to_json(&self) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("upid".to_string(), Value::String(self.upid.clone()));
        obj.insert("status".to_string(), Value::String(self.status.clone()));

        if let Some(ref es) = self.exitstatus {
            obj.insert("exitstatus".to_string(), Value::String(es.clone()));
        }
        if let Some(st) = self.starttime {
            obj.insert("starttime".to_string(), Value::Number(st.into()));
        }
        if let Some(et) = self.endtime {
            obj.insert("endtime".to_string(), Value::Number(et.into()));
        }
        if let Some(d) = self.duration_secs {
            obj.insert("duration_secs".to_string(), Value::Number(d.into()));
        }
        if let Some(ref n) = self.node {
            obj.insert("node".to_string(), Value::String(n.clone()));
        }
        if let Some(ref t) = self.task_type {
            obj.insert("task_type".to_string(), Value::String(t.clone()));
        }

        Value::Object(obj)
    }
}

fn normalize_api_base(api_url: &str) -> ModuleResult<String> {
    let trimmed = api_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "api_url cannot be empty".to_string(),
        ));
    }

    let base = if trimmed.ends_with("/api2/json") || trimmed.ends_with("api2/json") {
        trimmed.to_string()
    } else {
        format!("{}/api2/json", trimmed)
    };

    let parsed = Url::parse(&base).map_err(|e| {
        ModuleError::InvalidParameter(format!("Invalid api_url '{}': {}", api_url, e))
    })?;

    Ok(parsed.to_string().trim_end_matches('/').to_string())
}

fn parse_vmid(params: &ModuleParams) -> ModuleResult<u64> {
    parse_vmid_from_param(params, "vmid")
}

fn parse_vmid_from_param(params: &ModuleParams, key: &str) -> ModuleResult<u64> {
    if let Some(vmid) = params.get_i64(key)? {
        if vmid <= 0 {
            return Err(ModuleError::InvalidParameter(format!(
                "{} must be a positive integer",
                key
            )));
        }
        return Ok(vmid as u64);
    }

    if let Some(vmid) = params.get_string(key)? {
        let parsed: u64 = vmid
            .parse()
            .map_err(|_| ModuleError::InvalidParameter(format!("Invalid {} '{}'", key, vmid)))?;
        if parsed == 0 {
            return Err(ModuleError::InvalidParameter(format!(
                "{} must be a positive integer",
                key
            )));
        }
        return Ok(parsed);
    }

    Err(ModuleError::MissingParameter(key.to_string()))
}

/// Load a CA certificate from a PEM file
fn load_ca_certificate(path: &str) -> ModuleResult<Certificate> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(ModuleError::InvalidParameter(format!(
            "CA certificate file not found: {}",
            path.display()
        )));
    }

    let cert_data = std::fs::read(path).map_err(|e| {
        ModuleError::ExecutionFailed(format!(
            "Failed to read CA certificate '{}': {}",
            path.display(),
            e
        ))
    })?;

    // Try PEM format first, then DER
    Certificate::from_pem(&cert_data)
        .or_else(|_| Certificate::from_der(&cert_data))
        .map_err(|e| {
            ModuleError::InvalidParameter(format!(
                "Failed to parse CA certificate '{}': {}. Ensure it's in PEM or DER format.",
                path.display(),
                e
            ))
        })
}

fn build_client(timeout_secs: u64, validate_certs: bool, tls: &TlsConfig) -> ModuleResult<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .danger_accept_invalid_certs(!validate_certs);

    // Add custom CA certificate if specified
    if let Some(ref ca_path) = tls.ca_cert_path {
        let cert = load_ca_certificate(ca_path)?;
        builder = builder.add_root_certificate(cert);
    }

    // Configure TLS server name for SNI if specified
    // This allows connecting to a server using a different hostname than the certificate CN
    if let Some(ref server_name) = tls.server_name {
        // reqwest doesn't have direct SNI override, but we can use tls_built_in_root_certs
        // combined with the server_name in the URL. The practical approach is to document
        // that users should either:
        // 1. Use the certificate's CN/SAN in api_url and set tls_server_name for logging
        // 2. Use validate_certs=false (not recommended)
        // 3. Add the server to /etc/hosts
        //
        // For now, we store the server_name for potential future use and documentation
        tracing::debug!(
            tls_server_name = %server_name,
            "TLS server name configured (used for SNI verification hints)"
        );
    }

    builder
        .build()
        .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to build HTTP client: {}", e)))
}

struct RawResponse {
    status: reqwest::StatusCode,
    body: String,
}

fn send_request(
    client: &Client,
    method: Method,
    url: &str,
    token_id: &str,
    token_secret: &str,
    form: Option<&HashMap<String, String>>,
) -> ModuleResult<RawResponse> {
    let auth = format!("PVEAPIToken={}={}", token_id, token_secret);
    let is_delete = method == Method::DELETE;
    let mut request = client
        .request(method, url)
        .header(header::AUTHORIZATION, auth)
        .header(header::ACCEPT, "application/json");

    if let Some(form) = form {
        if is_delete {
            request = request.query(form);
        } else {
            request = request.form(form);
        }
    }

    let response = request.send().map_err(|e| {
        let msg = if e.is_timeout() {
            format!("Proxmox API request timed out: {}", e)
        } else if e.is_connect() {
            format!("Failed to connect to Proxmox API: {}", e)
        } else {
            format!("Proxmox API request failed: {}", e)
        };
        ModuleError::ExecutionFailed(msg)
    })?;

    let status = response.status();
    let body = response.text().map_err(|e| {
        ModuleError::ExecutionFailed(format!("Failed to read Proxmox API response: {}", e))
    })?;

    Ok(RawResponse { status, body })
}

fn request_json(
    client: &Client,
    method: Method,
    url: &str,
    token_id: &str,
    token_secret: &str,
    form: Option<&HashMap<String, String>>,
) -> ModuleResult<Value> {
    let raw = send_request(client, method, url, token_id, token_secret, form)?;

    if !raw.status.is_success() {
        return Err(ModuleError::ExecutionFailed(format!(
            "Proxmox API error {} {}: {}",
            raw.status.as_u16(),
            raw.status.canonical_reason().unwrap_or("Unknown"),
            raw.body
        )));
    }

    serde_json::from_str(&raw.body)
        .map_err(|e| ModuleError::ParseError(format!("Invalid JSON response: {}", e)))
}

fn request_json_optional(
    client: &Client,
    method: Method,
    url: &str,
    token_id: &str,
    token_secret: &str,
    form: Option<&HashMap<String, String>>,
) -> ModuleResult<Option<Value>> {
    let raw = send_request(client, method, url, token_id, token_secret, form)?;

    if !raw.status.is_success() {
        if is_missing_vm_response(raw.status, &raw.body) {
            return Ok(None);
        }
        return Err(ModuleError::ExecutionFailed(format!(
            "Proxmox API error {} {}: {}",
            raw.status.as_u16(),
            raw.status.canonical_reason().unwrap_or("Unknown"),
            raw.body
        )));
    }

    let value = serde_json::from_str(&raw.body)
        .map_err(|e| ModuleError::ParseError(format!("Invalid JSON response: {}", e)))?;
    Ok(Some(value))
}

fn is_missing_vm_response(status: reqwest::StatusCode, body: &str) -> bool {
    if status == reqwest::StatusCode::NOT_FOUND {
        return true;
    }
    if status == reqwest::StatusCode::INTERNAL_SERVER_ERROR {
        let msg = body.to_lowercase();
        return msg.contains("no such vm")
            || msg.contains("no such vmid")
            || msg.contains("not found");
    }
    false
}

fn api_url(config: &ProxmoxVmConfig, path: &str) -> String {
    format!("{}/{}", config.api_base, path.trim_start_matches('/'))
}

fn fetch_status_optional(
    client: &Client,
    config: &ProxmoxVmConfig,
) -> ModuleResult<Option<VmStatus>> {
    let url = api_url(
        config,
        &format!("nodes/{}/qemu/{}/status/current", config.node, config.vmid),
    );
    let response = request_json_optional(
        client,
        Method::GET,
        &url,
        &config.token_id,
        &config.token_secret,
        None,
    )?;

    let Some(response) = response else {
        return Ok(None);
    };

    let data = response
        .get("data")
        .cloned()
        .ok_or_else(|| ModuleError::ParseError("Proxmox response missing data".to_string()))?;

    let status = data
        .get("status")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            ModuleError::ParseError("Proxmox response missing data.status".to_string())
        })?;

    Ok(Some(VmStatus {
        power_state: VmPowerState::from_str(status),
        raw: data,
    }))
}

fn fetch_vm_config(client: &Client, config: &ProxmoxVmConfig) -> ModuleResult<Value> {
    let url = api_url(
        config,
        &format!("nodes/{}/qemu/{}/config", config.node, config.vmid),
    );
    let response = request_json(
        client,
        Method::GET,
        &url,
        &config.token_id,
        &config.token_secret,
        None,
    )?;

    response
        .get("data")
        .cloned()
        .ok_or_else(|| ModuleError::ParseError("Proxmox response missing data".to_string()))
}

fn normalize_config_value(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(if *value {
            "1".to_string()
        } else {
            "0".to_string()
        }),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn diff_vm_config(
    current: &Value,
    desired: &HashMap<String, String>,
) -> ModuleResult<VmConfigDiff> {
    let current_map = current.as_object().ok_or_else(|| {
        ModuleError::ParseError("Proxmox response data is not an object".to_string())
    })?;

    let mut changes = Vec::new();
    let mut update_params = HashMap::new();

    for (key, desired_value) in desired {
        let current_value = current_map.get(key).and_then(normalize_config_value);
        if current_value.as_deref() != Some(desired_value.as_str()) {
            changes.push(ConfigChange {
                key: key.clone(),
                current: current_value.clone(),
                desired: desired_value.clone(),
            });
            update_params.insert(key.clone(), desired_value.clone());
        }
    }

    Ok(VmConfigDiff {
        changes,
        update_params,
    })
}

fn update_vm_config(
    client: &Client,
    config: &ProxmoxVmConfig,
    updates: &HashMap<String, String>,
) -> ModuleResult<Value> {
    let url = api_url(
        config,
        &format!("nodes/{}/qemu/{}/config", config.node, config.vmid),
    );
    request_json(
        client,
        Method::POST,
        &url,
        &config.token_id,
        &config.token_secret,
        Some(updates),
    )
}

fn perform_action(client: &Client, config: &ProxmoxVmConfig, action: &str) -> ModuleResult<Value> {
    let url = api_url(
        config,
        &format!(
            "nodes/{}/qemu/{}/status/{}",
            config.node, config.vmid, action
        ),
    );
    request_json(
        client,
        Method::POST,
        &url,
        &config.token_id,
        &config.token_secret,
        None,
    )
}

/// Get task status from UPID
fn get_task_status(
    client: &Client,
    config: &ProxmoxVmConfig,
    upid: &str,
) -> ModuleResult<TaskInfo> {
    // Parse node from UPID if possible, otherwise use config.node
    // UPID format: UPID:{node}:{pid}:{pstart}:{starttime}:{type}:{id}:{user}
    let node = upid
        .strip_prefix("UPID:")
        .and_then(|s| s.split(':').next())
        .unwrap_or(&config.node);

    let url = api_url(
        config,
        &format!("nodes/{}/tasks/{}/status", node, urlencoding::encode(upid)),
    );

    let response = request_json(
        client,
        Method::GET,
        &url,
        &config.token_id,
        &config.token_secret,
        None,
    )?;

    let data = response.get("data").cloned().unwrap_or(Value::Null);
    Ok(TaskInfo::from_api_response(upid, &data))
}

/// Wait for a task to complete, polling until done or timeout
fn wait_for_task(client: &Client, config: &ProxmoxVmConfig, upid: &str) -> ModuleResult<TaskInfo> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(config.wait.timeout_secs);
    let interval = std::time::Duration::from_secs(config.wait.interval_secs);

    loop {
        let task_info = get_task_status(client, config, upid)?;

        if task_info.is_complete() {
            if task_info.is_success() {
                return Ok(task_info);
            } else {
                let exit_status = task_info.exitstatus.as_deref().unwrap_or("unknown");
                return Err(ModuleError::ExecutionFailed(format!(
                    "Proxmox task {} failed with exit status: {}",
                    upid, exit_status
                )));
            }
        }

        if start.elapsed() >= timeout {
            return Err(ModuleError::ExecutionFailed(format!(
                "Timed out waiting for task {} after {} seconds (status: {})",
                upid, config.wait.timeout_secs, task_info.status
            )));
        }

        std::thread::sleep(interval);
    }
}

/// Extract UPID from API response data
fn extract_upid(response: &Value) -> Option<String> {
    response
        .get("data")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
}

fn value_to_param_string(key: &str, value: &Value) -> ModuleResult<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(if *value {
            "1".to_string()
        } else {
            "0".to_string()
        }),
        Value::Null => Err(ModuleError::InvalidParameter(format!(
            "Parameter '{}' cannot be null",
            key
        ))),
        _ => Err(ModuleError::InvalidParameter(format!(
            "Parameter '{}' must be a string, number, or bool",
            key
        ))),
    }
}

fn parse_string_map(field: &str, value: &Value) -> ModuleResult<HashMap<String, String>> {
    let object = value
        .as_object()
        .ok_or_else(|| ModuleError::InvalidParameter(format!("'{}' must be an object", field)))?;

    let mut map = HashMap::new();
    for (key, value) in object {
        let value = value_to_param_string(key, value)?;
        map.insert(key.clone(), value);
    }

    Ok(map)
}

fn is_allowed_config_key(key: &str) -> bool {
    key != "vmid" && ALLOWED_CREATE_PARAMS.contains(&key)
}

fn validate_config_params(params: &HashMap<String, String>) -> ModuleResult<()> {
    let unknown: Vec<&String> = params
        .keys()
        .filter(|k| !is_allowed_config_key(k.as_str()))
        .collect();

    if unknown.is_empty() {
        Ok(())
    } else {
        let mut unknown_sorted: Vec<&str> = unknown.iter().map(|s| s.as_str()).collect();
        unknown_sorted.sort();

        Err(ModuleError::InvalidParameter(format!(
            "Unknown config parameter(s): {}. Set strict_params=false to allow pass-through.",
            unknown_sorted.join(", ")
        )))
    }
}

/// Validate parameter keys against an allowlist
///
/// Returns an error listing unknown keys if any are found.
fn validate_params(
    params: &HashMap<String, String>,
    allowlist: &[&str],
    operation: &str,
) -> ModuleResult<()> {
    let unknown: Vec<&String> = params
        .keys()
        .filter(|k| !allowlist.contains(&k.as_str()))
        .collect();

    if unknown.is_empty() {
        Ok(())
    } else {
        // Sort for consistent error messages
        let mut unknown_sorted: Vec<&str> = unknown.iter().map(|s| s.as_str()).collect();
        unknown_sorted.sort();

        Err(ModuleError::InvalidParameter(format!(
            "Unknown {} parameter(s): {}. Set strict_params=false to allow pass-through.",
            operation,
            unknown_sorted.join(", ")
        )))
    }
}

fn build_create_params(
    config: &ProxmoxVmConfig,
    params: &ModuleParams,
) -> ModuleResult<HashMap<String, String>> {
    let mut body = if let Some(value) = params.get("create") {
        parse_string_map("create", value)?
    } else {
        HashMap::new()
    };

    body.insert("vmid".to_string(), config.vmid.to_string());

    if let Some(name) = params.get_string("name")? {
        body.entry("name".to_string()).or_insert(name);
    }
    if let Some(description) = params.get_string("description")? {
        body.entry("description".to_string()).or_insert(description);
    }
    if let Some(tags) = params.get_string("tags")? {
        body.entry("tags".to_string()).or_insert(tags);
    }

    if !body.contains_key("name") {
        if params.get_bool_or("auto_name", false) {
            body.insert("name".to_string(), format!("rustible-vm-{}", config.vmid));
        } else {
            return Err(ModuleError::MissingParameter(
                "name (required for create)".to_string(),
            ));
        }
    }

    // Validate params if strict mode is enabled
    if config.strict_params {
        validate_params(&body, ALLOWED_CREATE_PARAMS, "create")?;
    }

    Ok(body)
}

fn build_clone_params(
    config: &ProxmoxVmConfig,
    params: &ModuleParams,
) -> ModuleResult<HashMap<String, String>> {
    let mut body = if let Some(value) = params.get("clone") {
        parse_string_map("clone", value)?
    } else {
        HashMap::new()
    };

    body.insert("newid".to_string(), config.vmid.to_string());

    let full = params.get_bool_or("clone_full", true);
    body.entry("full".to_string()).or_insert(if full {
        "1".to_string()
    } else {
        "0".to_string()
    });

    if let Some(name) = params.get_string("name")? {
        body.entry("name".to_string()).or_insert(name);
    }
    if let Some(description) = params.get_string("description")? {
        body.entry("description".to_string()).or_insert(description);
    }
    if let Some(tags) = params.get_string("tags")? {
        body.entry("tags".to_string()).or_insert(tags);
    }
    if let Some(target) = params.get_string("clone_target_node")? {
        body.entry("target".to_string()).or_insert(target);
    }
    if let Some(storage) = params.get_string("clone_storage")? {
        body.entry("storage".to_string()).or_insert(storage);
    }
    if let Some(pool) = params.get_string("clone_pool")? {
        body.entry("pool".to_string()).or_insert(pool);
    }
    if let Some(snapname) = params.get_string("clone_snapname")? {
        body.entry("snapname".to_string()).or_insert(snapname);
    }

    if !body.contains_key("name") && params.get_bool_or("auto_name", false) {
        body.insert(
            "name".to_string(),
            format!("rustible-clone-{}", config.vmid),
        );
    }

    // Validate params if strict mode is enabled
    if config.strict_params {
        validate_params(&body, ALLOWED_CLONE_PARAMS, "clone")?;
    }

    Ok(body)
}

fn build_delete_params(
    config: &ProxmoxVmConfig,
    params: &ModuleParams,
) -> ModuleResult<HashMap<String, String>> {
    let body = if let Some(value) = params.get("delete") {
        parse_string_map("delete", value)?
    } else {
        HashMap::new()
    };

    // Validate params if strict mode is enabled
    if config.strict_params && !body.is_empty() {
        validate_params(&body, ALLOWED_DELETE_PARAMS, "delete")?;
    }

    Ok(body)
}

fn create_vm(
    client: &Client,
    config: &ProxmoxVmConfig,
    params: &ModuleParams,
) -> ModuleResult<Value> {
    let url = api_url(config, &format!("nodes/{}/qemu", config.node));
    let body = build_create_params(config, params)?;
    request_json(
        client,
        Method::POST,
        &url,
        &config.token_id,
        &config.token_secret,
        Some(&body),
    )
}

fn clone_vm(
    client: &Client,
    config: &ProxmoxVmConfig,
    params: &ModuleParams,
    source_vmid: u64,
) -> ModuleResult<Value> {
    let url = api_url(
        config,
        &format!("nodes/{}/qemu/{}/clone", config.node, source_vmid),
    );
    let body = build_clone_params(config, params)?;
    request_json(
        client,
        Method::POST,
        &url,
        &config.token_id,
        &config.token_secret,
        Some(&body),
    )
}

fn delete_vm(
    client: &Client,
    config: &ProxmoxVmConfig,
    params: &ModuleParams,
) -> ModuleResult<Value> {
    let url = api_url(
        config,
        &format!("nodes/{}/qemu/{}", config.node, config.vmid),
    );
    let body = build_delete_params(config, params)?;
    request_json(
        client,
        Method::DELETE,
        &url,
        &config.token_id,
        &config.token_secret,
        if body.is_empty() { None } else { Some(&body) },
    )
}

/// Handle waiting for task completion and updating output with task info
fn handle_task_wait(
    client: &Client,
    config: &ProxmoxVmConfig,
    response: &Value,
    mut output: ModuleOutput,
) -> ModuleResult<ModuleOutput> {
    // Extract UPID from response
    let upid = extract_upid(response);

    if let Some(ref upid_str) = upid {
        output = output.with_data("upid", json!(upid_str));

        // If wait is enabled, poll until completion
        if config.wait.enabled {
            let task_info = wait_for_task(client, config, upid_str)?;
            output = output.with_data("task", task_info.to_json());
        }
    }

    Ok(output)
}

fn with_common_data(
    output: ModuleOutput,
    config: &ProxmoxVmConfig,
    status: Option<&VmStatus>,
) -> ModuleOutput {
    let present = status.is_some();
    let status_value = status
        .map(|status| status.power_state.as_str())
        .unwrap_or("absent");
    let status_raw = status
        .map(|status| status.raw.clone())
        .unwrap_or_else(|| json!({}));
    output
        .with_data("vmid", json!(config.vmid))
        .with_data("node", json!(config.node))
        .with_data("present", json!(present))
        .with_data("status", json!(status_value))
        .with_data("status_raw", status_raw)
        .with_data(
            "desired_state",
            json!(match config.state {
                DesiredState::Status => "status",
                DesiredState::Started => "started",
                DesiredState::Stopped => "stopped",
                DesiredState::Restarted => "restarted",
                DesiredState::Present => "present",
                DesiredState::Absent => "absent",
                DesiredState::Cloned => "cloned",
            }),
        )
}

/// Proxmox VM lifecycle module.
pub struct ProxmoxVmModule;

impl Module for ProxmoxVmModule {
    fn name(&self) -> &'static str {
        "proxmox_vm"
    }

    fn description(&self) -> &'static str {
        "Manage Proxmox VE VM lifecycle via the Proxmox API"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: DEFAULT_REQUESTS_PER_SECOND,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &[
            "api_url",
            "api_token_id",
            "api_token_secret",
            "node",
            "vmid",
        ]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut params = HashMap::new();
        params.insert("state", json!("status"));
        params.insert("stop_method", json!("shutdown"));
        params.insert("name", json!("vm-name"));
        params.insert("description", json!("optional description"));
        params.insert("tags", json!("tag1,tag2"));
        params.insert("auto_name", json!(false));
        params.insert("clone_from", json!(0));
        params.insert("clone_full", json!(true));
        params.insert("clone_target_node", json!("target-node"));
        params.insert("clone_storage", json!("storage"));
        params.insert("clone_pool", json!("pool"));
        params.insert("clone_snapname", json!("snapshot"));
        params.insert("create", json!({}));
        params.insert("clone", json!({}));
        params.insert("delete", json!({}));
        params.insert("config", json!({}));
        params.insert("timeout", json!(DEFAULT_TIMEOUT_SECS));
        params.insert("validate_certs", json!(true));
        params
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let _ = ProxmoxVmConfig::from_params(params)?;
        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ProxmoxVmConfig::from_params(params)?;
        let client = build_client(config.timeout_secs, config.validate_certs, &config.tls)?;
        let status_opt = fetch_status_optional(&client, &config)?;

        let mut output = match config.state {
            DesiredState::Status => {
                if status_opt.is_some() {
                    ModuleOutput::ok("VM status retrieved")
                } else {
                    ModuleOutput::ok("VM not found")
                }
            }
            DesiredState::Present => {
                if status_opt.is_some() {
                    if let Some(desired_config) =
                        config.config_params.as_ref().filter(|cfg| !cfg.is_empty())
                    {
                        let current_config = fetch_vm_config(&client, &config)?;
                        let diff = diff_vm_config(&current_config, desired_config)?;

                        if diff.is_empty() {
                            ModuleOutput::ok("VM already exists and matches desired config")
                        } else if context.check_mode {
                            let mut output =
                                ModuleOutput::changed("VM config would be updated (check mode)")
                                    .with_data("action", json!("update"))
                                    .with_data("config_changes", diff.to_value());
                            if context.diff_mode {
                                output = output.with_diff(diff.to_diff());
                            }
                            output
                        } else {
                            let response = update_vm_config(&client, &config, &diff.update_params)?;
                            let mut output = ModuleOutput::changed("VM config update requested")
                                .with_data("action", json!("update"))
                                .with_data("config_changes", diff.to_value())
                                .with_data("action_response", response.clone());
                            if context.diff_mode {
                                output = output.with_diff(diff.to_diff());
                            }
                            handle_task_wait(&client, &config, &response, output)?
                        }
                    } else {
                        ModuleOutput::ok("VM already exists")
                    }
                } else if context.check_mode {
                    let _ = build_create_params(&config, params)?;
                    ModuleOutput::changed("VM would be created (check mode)")
                } else {
                    let response = create_vm(&client, &config, params)?;
                    let output = ModuleOutput::changed("VM create requested")
                        .with_data("action", json!("create"))
                        .with_data("action_response", response.clone());
                    handle_task_wait(&client, &config, &response, output)?
                }
            }
            DesiredState::Absent => {
                if status_opt.is_some() {
                    if context.check_mode {
                        ModuleOutput::changed("VM would be deleted (check mode)")
                    } else {
                        let response = delete_vm(&client, &config, params)?;
                        let output = ModuleOutput::changed("VM delete requested")
                            .with_data("action", json!("delete"))
                            .with_data("action_response", response.clone());
                        handle_task_wait(&client, &config, &response, output)?
                    }
                } else {
                    ModuleOutput::ok("VM already absent")
                }
            }
            DesiredState::Cloned => {
                if status_opt.is_some() {
                    ModuleOutput::ok("VM already exists")
                } else if context.check_mode {
                    let _ = build_clone_params(&config, params)?;
                    let _ = parse_vmid_from_param(params, "clone_from")?;
                    ModuleOutput::changed("VM would be cloned (check mode)")
                } else {
                    let source_vmid = parse_vmid_from_param(params, "clone_from")?;
                    let response = clone_vm(&client, &config, params, source_vmid)?;
                    let output = ModuleOutput::changed("VM clone requested")
                        .with_data("action", json!("clone"))
                        .with_data("action_response", response.clone())
                        .with_data("clone_from", json!(source_vmid));
                    handle_task_wait(&client, &config, &response, output)?
                }
            }
            DesiredState::Started => {
                let status = status_opt
                    .as_ref()
                    .ok_or_else(|| ModuleError::ExecutionFailed("VM not found".to_string()))?;
                if status.power_state.is_running() {
                    ModuleOutput::ok("VM already running")
                } else if context.check_mode {
                    ModuleOutput::changed("VM would be started (check mode)")
                } else {
                    let response = perform_action(&client, &config, "start")?;
                    let output = ModuleOutput::changed("VM start requested")
                        .with_data("action", json!("start"))
                        .with_data("action_response", response.clone());
                    handle_task_wait(&client, &config, &response, output)?
                }
            }
            DesiredState::Stopped => {
                let status = status_opt
                    .as_ref()
                    .ok_or_else(|| ModuleError::ExecutionFailed("VM not found".to_string()))?;
                if status.power_state.is_stopped() {
                    ModuleOutput::ok("VM already stopped")
                } else if context.check_mode {
                    ModuleOutput::changed("VM would be stopped (check mode)")
                } else {
                    let response = perform_action(&client, &config, config.stop_method.endpoint())?;
                    let output = ModuleOutput::changed("VM stop requested")
                        .with_data("action", json!(config.stop_method.endpoint()))
                        .with_data("action_response", response.clone());
                    handle_task_wait(&client, &config, &response, output)?
                }
            }
            DesiredState::Restarted => {
                let status = status_opt
                    .as_ref()
                    .ok_or_else(|| ModuleError::ExecutionFailed("VM not found".to_string()))?;
                if context.check_mode {
                    ModuleOutput::changed("VM would be restarted (check mode)")
                } else if status.power_state.is_running() {
                    let response = perform_action(&client, &config, "reboot")?;
                    let output = ModuleOutput::changed("VM reboot requested")
                        .with_data("action", json!("reboot"))
                        .with_data("action_response", response.clone());
                    handle_task_wait(&client, &config, &response, output)?
                } else {
                    let response = perform_action(&client, &config, "start")?;
                    let output = ModuleOutput::changed("VM start requested")
                        .with_data("action", json!("start"))
                        .with_data("action_response", response.clone());
                    handle_task_wait(&client, &config, &response, output)?
                }
            }
        };

        output = with_common_data(output, &config, status_opt.as_ref());
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_api_base() {
        let base = normalize_api_base("https://pve.example:8006").unwrap();
        assert_eq!(base, "https://pve.example:8006/api2/json");

        let base = normalize_api_base("https://pve.example:8006/api2/json/").unwrap();
        assert_eq!(base, "https://pve.example:8006/api2/json");
    }

    #[test]
    fn test_task_info_from_response() {
        let upid = "UPID:pve:00001234:00000001:12345678:qmstart:101:user@pam:";
        let data = json!({
            "status": "stopped",
            "exitstatus": "OK",
            "starttime": 1705600000,
            "endtime": 1705600010,
            "node": "pve",
            "type": "qmstart"
        });
        let task_info = TaskInfo::from_api_response(upid, &data);

        assert_eq!(task_info.upid, upid);
        assert_eq!(task_info.status, "stopped");
        assert_eq!(task_info.exitstatus, Some("OK".to_string()));
        assert_eq!(task_info.starttime, Some(1705600000));
        assert_eq!(task_info.endtime, Some(1705600010));
        assert_eq!(task_info.duration_secs, Some(10));
        assert!(task_info.is_complete());
        assert!(task_info.is_success());
    }

    #[test]
    fn test_task_info_running() {
        let upid = "UPID:pve:00001234:00000001:12345678:qmstart:101:user@pam:";
        let data = json!({
            "status": "running",
            "starttime": 1705600000,
        });
        let task_info = TaskInfo::from_api_response(upid, &data);

        assert!(!task_info.is_complete());
        assert!(!task_info.is_success());
    }

    #[test]
    fn test_task_info_failed() {
        let upid = "UPID:pve:00001234:00000001:12345678:qmstart:101:user@pam:";
        let data = json!({
            "status": "stopped",
            "exitstatus": "ERROR",
        });
        let task_info = TaskInfo::from_api_response(upid, &data);

        assert!(task_info.is_complete());
        assert!(!task_info.is_success());
    }

    #[test]
    fn test_task_info_to_json() {
        let upid = "UPID:pve:00001234:00000001:12345678:qmstart:101:user@pam:";
        let data = json!({
            "status": "stopped",
            "exitstatus": "OK",
            "starttime": 1705600000,
            "endtime": 1705600010,
        });
        let task_info = TaskInfo::from_api_response(upid, &data);
        let json = task_info.to_json();

        assert_eq!(json.get("status"), Some(&json!("stopped")));
        assert_eq!(json.get("exitstatus"), Some(&json!("OK")));
        assert_eq!(json.get("duration_secs"), Some(&json!(10)));
    }

    #[test]
    fn test_wait_config_default() {
        let wait = WaitConfig::default();
        assert!(!wait.enabled);
        assert_eq!(wait.timeout_secs, DEFAULT_WAIT_TIMEOUT_SECS);
        assert_eq!(wait.interval_secs, DEFAULT_WAIT_INTERVAL_SECS);
    }

    #[test]
    fn test_desired_state_parse() {
        assert_eq!(
            DesiredState::from_str("status").unwrap(),
            DesiredState::Status
        );
        assert_eq!(
            DesiredState::from_str("running").unwrap(),
            DesiredState::Started
        );
        assert_eq!(
            DesiredState::from_str("stopped").unwrap(),
            DesiredState::Stopped
        );
        assert_eq!(
            DesiredState::from_str("restarted").unwrap(),
            DesiredState::Restarted
        );
        assert_eq!(
            DesiredState::from_str("present").unwrap(),
            DesiredState::Present
        );
        assert_eq!(
            DesiredState::from_str("absent").unwrap(),
            DesiredState::Absent
        );
        assert_eq!(
            DesiredState::from_str("cloned").unwrap(),
            DesiredState::Cloned
        );
    }

    #[test]
    fn test_stop_method_parse() {
        assert_eq!(
            StopMethod::from_str("shutdown").unwrap(),
            StopMethod::Shutdown
        );
        assert_eq!(StopMethod::from_str("stop").unwrap(), StopMethod::Stop);
    }

    #[test]
    fn test_parse_vmid() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("vmid".to_string(), json!(101));
        assert_eq!(parse_vmid(&params).unwrap(), 101);

        let mut params: ModuleParams = HashMap::new();
        params.insert("vmid".to_string(), json!("102"));
        assert_eq!(parse_vmid(&params).unwrap(), 102);
    }

    #[test]
    fn test_parse_string_map() {
        let value = json!({
            "name": "test-vm",
            "memory": 1024,
            "onboot": true,
        });
        let parsed = parse_string_map("create", &value).unwrap();
        assert_eq!(parsed.get("name"), Some(&"test-vm".to_string()));
        assert_eq!(parsed.get("memory"), Some(&"1024".to_string()));
        assert_eq!(parsed.get("onboot"), Some(&"1".to_string()));
    }

    #[test]
    fn test_validate_config_params_unknown() {
        let mut params = HashMap::new();
        params.insert("bad_param".to_string(), "value".to_string());

        let result = validate_config_params(&params);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unknown config parameter(s)"));
        assert!(msg.contains("bad_param"));
    }

    #[test]
    fn test_diff_vm_config_detects_changes() {
        let current = json!({
            "memory": "1024",
            "cores": 2,
            "onboot": 1,
        });
        let mut desired = HashMap::new();
        desired.insert("memory".to_string(), "2048".to_string());
        desired.insert("cores".to_string(), "2".to_string());
        desired.insert("onboot".to_string(), "1".to_string());

        let diff = diff_vm_config(&current, &desired).unwrap();
        assert_eq!(diff.changes.len(), 1);
        assert_eq!(diff.update_params.get("memory"), Some(&"2048".to_string()));
    }

    #[test]
    fn test_build_create_params_auto_name() {
        let config = ProxmoxVmConfig {
            api_base: "https://pve.example/api2/json".to_string(),
            token_id: "id".to_string(),
            token_secret: "secret".to_string(),
            node: "node".to_string(),
            vmid: 123,
            state: DesiredState::Present,
            stop_method: StopMethod::Shutdown,
            timeout_secs: 30,
            validate_certs: true,
            wait: WaitConfig::default(),
            strict_params: false, // Disable for this test
            config_params: None,
            tls: TlsConfig::default(),
        };

        let mut params: ModuleParams = HashMap::new();
        params.insert("auto_name".to_string(), json!(true));
        let body = build_create_params(&config, &params).unwrap();
        assert_eq!(body.get("name"), Some(&"rustible-vm-123".to_string()));
    }

    #[test]
    fn test_build_clone_params_auto_name() {
        let config = ProxmoxVmConfig {
            api_base: "https://pve.example/api2/json".to_string(),
            token_id: "id".to_string(),
            token_secret: "secret".to_string(),
            node: "node".to_string(),
            vmid: 456,
            state: DesiredState::Cloned,
            stop_method: StopMethod::Shutdown,
            timeout_secs: 30,
            validate_certs: true,
            wait: WaitConfig::default(),
            strict_params: false, // Disable for this test
            config_params: None,
            tls: TlsConfig::default(),
        };

        let mut params: ModuleParams = HashMap::new();
        params.insert("auto_name".to_string(), json!(true));
        let body = build_clone_params(&config, &params).unwrap();
        assert_eq!(body.get("name"), Some(&"rustible-clone-456".to_string()));
    }

    #[test]
    fn test_validate_params_valid() {
        let mut params = HashMap::new();
        params.insert("name".to_string(), "test-vm".to_string());
        params.insert("memory".to_string(), "1024".to_string());
        params.insert("cores".to_string(), "2".to_string());

        let result = validate_params(&params, ALLOWED_CREATE_PARAMS, "create");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_params_unknown() {
        let mut params = HashMap::new();
        params.insert("name".to_string(), "test-vm".to_string());
        params.insert("typo_param".to_string(), "value".to_string());
        params.insert("another_bad".to_string(), "value".to_string());

        let result = validate_params(&params, ALLOWED_CREATE_PARAMS, "create");
        assert!(result.is_err());

        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Unknown create parameter(s)"));
        assert!(msg.contains("typo_param"));
        assert!(msg.contains("another_bad"));
        assert!(msg.contains("strict_params=false"));
    }

    #[test]
    fn test_strict_params_catches_typo_in_create() {
        let config = ProxmoxVmConfig {
            api_base: "https://pve.example/api2/json".to_string(),
            token_id: "id".to_string(),
            token_secret: "secret".to_string(),
            node: "node".to_string(),
            vmid: 100,
            state: DesiredState::Present,
            stop_method: StopMethod::Shutdown,
            timeout_secs: 30,
            validate_certs: true,
            wait: WaitConfig::default(),
            strict_params: true, // Enable strict validation
            config_params: None,
            tls: TlsConfig::default(),
        };

        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), json!("test-vm"));
        // Typo: "memorry" instead of "memory"
        params.insert(
            "create".to_string(),
            json!({
                "memorry": 1024,
                "cores": 2
            }),
        );

        let result = build_create_params(&config, &params);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("memorry"));
    }

    #[test]
    fn test_strict_params_disabled_allows_unknown() {
        let config = ProxmoxVmConfig {
            api_base: "https://pve.example/api2/json".to_string(),
            token_id: "id".to_string(),
            token_secret: "secret".to_string(),
            node: "node".to_string(),
            vmid: 100,
            state: DesiredState::Present,
            stop_method: StopMethod::Shutdown,
            timeout_secs: 30,
            validate_certs: true,
            wait: WaitConfig::default(),
            strict_params: false, // Disable strict validation
            config_params: None,
            tls: TlsConfig::default(),
        };

        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), json!("test-vm"));
        params.insert(
            "create".to_string(),
            json!({
                "custom_param": "value",
                "another_custom": 123
            }),
        );

        let result = build_create_params(&config, &params);
        assert!(result.is_ok());
        let body = result.unwrap();
        assert_eq!(body.get("custom_param"), Some(&"value".to_string()));
    }

    #[test]
    fn test_strict_params_clone_validation() {
        let config = ProxmoxVmConfig {
            api_base: "https://pve.example/api2/json".to_string(),
            token_id: "id".to_string(),
            token_secret: "secret".to_string(),
            node: "node".to_string(),
            vmid: 200,
            state: DesiredState::Cloned,
            stop_method: StopMethod::Shutdown,
            timeout_secs: 30,
            validate_certs: true,
            wait: WaitConfig::default(),
            strict_params: true,
            config_params: None,
            tls: TlsConfig::default(),
        };

        let mut params: ModuleParams = HashMap::new();
        // Typo: "storge" instead of "storage"
        params.insert(
            "clone".to_string(),
            json!({
                "storge": "local-lvm"
            }),
        );

        let result = build_clone_params(&config, &params);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("storge"));
        assert!(msg.contains("clone"));
    }

    #[test]
    fn test_strict_params_delete_validation() {
        let config = ProxmoxVmConfig {
            api_base: "https://pve.example/api2/json".to_string(),
            token_id: "id".to_string(),
            token_secret: "secret".to_string(),
            node: "node".to_string(),
            vmid: 100,
            state: DesiredState::Absent,
            stop_method: StopMethod::Shutdown,
            timeout_secs: 30,
            validate_certs: true,
            wait: WaitConfig::default(),
            strict_params: true,
            config_params: None,
            tls: TlsConfig::default(),
        };

        let mut params: ModuleParams = HashMap::new();
        // Typo: "purg" instead of "purge"
        params.insert(
            "delete".to_string(),
            json!({
                "purg": true
            }),
        );

        let result = build_delete_params(&config, &params);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("purg"));
        assert!(msg.contains("delete"));
    }

    #[test]
    fn test_tls_config_default() {
        let tls = TlsConfig::default();
        assert!(tls.server_name.is_none());
        assert!(tls.ca_cert_path.is_none());
    }

    #[test]
    fn test_tls_config_custom() {
        let tls = TlsConfig {
            server_name: Some("custom.example.com".to_string()),
            ca_cert_path: Some("/path/to/ca.pem".to_string()),
        };
        assert_eq!(tls.server_name, Some("custom.example.com".to_string()));
        assert_eq!(tls.ca_cert_path, Some("/path/to/ca.pem".to_string()));
    }

    #[test]
    fn test_load_ca_certificate_not_found() {
        let result = load_ca_certificate("/nonexistent/path/ca.pem");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("No such file")
                || msg.contains("not found")
                || msg.contains("Failed to read")
        );
    }
}
