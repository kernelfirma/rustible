//! Proxmox VM lifecycle module (API-driven, local execution).

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use reqwest::blocking::Client;
use reqwest::{header, Method};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use url::Url;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_REQUESTS_PER_SECOND: u32 = 5;

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
            return Err(ModuleError::InvalidParameter(
                format!("{} must be a positive integer", key),
            ));
        }
        return Ok(vmid as u64);
    }

    if let Some(vmid) = params.get_string(key)? {
        let parsed: u64 = vmid.parse().map_err(|_| {
            ModuleError::InvalidParameter(format!("Invalid {} '{}'", key, vmid))
        })?;
        if parsed == 0 {
            return Err(ModuleError::InvalidParameter(
                format!("{} must be a positive integer", key),
            ));
        }
        return Ok(parsed);
    }

    Err(ModuleError::MissingParameter(key.to_string()))
}

fn build_client(timeout_secs: u64, validate_certs: bool) -> ModuleResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .danger_accept_invalid_certs(!validate_certs)
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
        return msg.contains("no such vm") || msg.contains("no such vmid") || msg.contains("not found");
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
        &format!(
            "nodes/{}/qemu/{}/status/current",
            config.node, config.vmid
        ),
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

fn perform_action(
    client: &Client,
    config: &ProxmoxVmConfig,
    action: &str,
) -> ModuleResult<Value> {
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

fn value_to_param_string(key: &str, value: &Value) -> ModuleResult<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(if *value { "1".to_string() } else { "0".to_string() }),
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
    let object = value.as_object().ok_or_else(|| {
        ModuleError::InvalidParameter(format!("'{}' must be an object", field))
    })?;

    let mut map = HashMap::new();
    for (key, value) in object {
        let value = value_to_param_string(key, value)?;
        map.insert(key.clone(), value);
    }

    Ok(map)
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
        body.entry("description".to_string())
            .or_insert(description);
    }
    if let Some(tags) = params.get_string("tags")? {
        body.entry("tags".to_string()).or_insert(tags);
    }

    if !body.contains_key("name") {
        if params.get_bool_or("auto_name", false) {
            body.insert(
                "name".to_string(),
                format!("rustible-vm-{}", config.vmid),
            );
        } else {
            return Err(ModuleError::MissingParameter(
                "name (required for create)".to_string(),
            ));
        }
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
    body.entry("full".to_string())
        .or_insert(if full { "1".to_string() } else { "0".to_string() });

    if let Some(name) = params.get_string("name")? {
        body.entry("name".to_string()).or_insert(name);
    }
    if let Some(description) = params.get_string("description")? {
        body.entry("description".to_string())
            .or_insert(description);
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

    Ok(body)
}

fn build_delete_params(params: &ModuleParams) -> ModuleResult<HashMap<String, String>> {
    if let Some(value) = params.get("delete") {
        parse_string_map("delete", value)
    } else {
        Ok(HashMap::new())
    }
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
    let url = api_url(config, &format!("nodes/{}/qemu/{}", config.node, config.vmid));
    let body = build_delete_params(params)?;
    request_json(
        client,
        Method::DELETE,
        &url,
        &config.token_id,
        &config.token_secret,
        if body.is_empty() { None } else { Some(&body) },
    )
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
        &["api_url", "api_token_id", "api_token_secret", "node", "vmid"]
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
        let client = build_client(config.timeout_secs, config.validate_certs)?;
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
                    ModuleOutput::ok("VM already exists")
                } else if context.check_mode {
                    let _ = build_create_params(&config, params)?;
                    ModuleOutput::changed("VM would be created (check mode)")
                } else {
                    let response = create_vm(&client, &config, params)?;
                    ModuleOutput::changed("VM create requested")
                        .with_data("action", json!("create"))
                        .with_data("action_response", response)
                }
            }
            DesiredState::Absent => {
                if status_opt.is_some() {
                    if context.check_mode {
                        ModuleOutput::changed("VM would be deleted (check mode)")
                    } else {
                        let response = delete_vm(&client, &config, params)?;
                        ModuleOutput::changed("VM delete requested")
                            .with_data("action", json!("delete"))
                            .with_data("action_response", response)
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
                    ModuleOutput::changed("VM clone requested")
                        .with_data("action", json!("clone"))
                        .with_data("action_response", response)
                        .with_data("clone_from", json!(source_vmid))
                }
            }
            DesiredState::Started => {
                let status = status_opt.as_ref().ok_or_else(|| {
                    ModuleError::ExecutionFailed("VM not found".to_string())
                })?;
                if status.power_state.is_running() {
                    ModuleOutput::ok("VM already running")
                } else if context.check_mode {
                    ModuleOutput::changed("VM would be started (check mode)")
                } else {
                    let response = perform_action(&client, &config, "start")?;
                    ModuleOutput::changed("VM start requested")
                        .with_data("action", json!("start"))
                        .with_data("action_response", response)
                }
            }
            DesiredState::Stopped => {
                let status = status_opt.as_ref().ok_or_else(|| {
                    ModuleError::ExecutionFailed("VM not found".to_string())
                })?;
                if status.power_state.is_stopped() {
                    ModuleOutput::ok("VM already stopped")
                } else if context.check_mode {
                    ModuleOutput::changed("VM would be stopped (check mode)")
                } else {
                    let response = perform_action(&client, &config, config.stop_method.endpoint())?;
                    ModuleOutput::changed("VM stop requested")
                        .with_data("action", json!(config.stop_method.endpoint()))
                        .with_data("action_response", response)
                }
            }
            DesiredState::Restarted => {
                let status = status_opt.as_ref().ok_or_else(|| {
                    ModuleError::ExecutionFailed("VM not found".to_string())
                })?;
                if context.check_mode {
                    ModuleOutput::changed("VM would be restarted (check mode)")
                } else if status.power_state.is_running() {
                    let response = perform_action(&client, &config, "reboot")?;
                    ModuleOutput::changed("VM reboot requested")
                        .with_data("action", json!("reboot"))
                        .with_data("action_response", response)
                } else {
                    let response = perform_action(&client, &config, "start")?;
                    ModuleOutput::changed("VM start requested")
                        .with_data("action", json!("start"))
                        .with_data("action_response", response)
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
    fn test_desired_state_parse() {
        assert_eq!(DesiredState::from_str("status").unwrap(), DesiredState::Status);
        assert_eq!(DesiredState::from_str("running").unwrap(), DesiredState::Started);
        assert_eq!(DesiredState::from_str("stopped").unwrap(), DesiredState::Stopped);
        assert_eq!(
            DesiredState::from_str("restarted").unwrap(),
            DesiredState::Restarted
        );
        assert_eq!(DesiredState::from_str("present").unwrap(), DesiredState::Present);
        assert_eq!(DesiredState::from_str("absent").unwrap(), DesiredState::Absent);
        assert_eq!(DesiredState::from_str("cloned").unwrap(), DesiredState::Cloned);
    }

    #[test]
    fn test_stop_method_parse() {
        assert_eq!(StopMethod::from_str("shutdown").unwrap(), StopMethod::Shutdown);
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
        };

        let mut params: ModuleParams = HashMap::new();
        params.insert("auto_name".to_string(), json!(true));
        let body = build_create_params(&config, &params).unwrap();
        assert_eq!(
            body.get("name"),
            Some(&"rustible-vm-123".to_string())
        );
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
        };

        let mut params: ModuleParams = HashMap::new();
        params.insert("auto_name".to_string(), json!(true));
        let body = build_clone_params(&config, &params).unwrap();
        assert_eq!(
            body.get("name"),
            Some(&"rustible-clone-456".to_string())
        );
    }
}
