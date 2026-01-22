//! Proxmox VM integration tests (requires Proxmox API access).
//!
//! To run:
//!   export RUSTIBLE_PVE_TESTS=1
//!   export RUSTIBLE_PVE_API_URL=https://svr-host:8006
//!   export RUSTIBLE_PVE_TOKEN_ID='root@pam!token'
//!   export RUSTIBLE_PVE_TOKEN_SECRET='secret'
//!   export RUSTIBLE_PVE_NODE=svr-host
//!   export RUSTIBLE_PVE_VMID=100
//!   export RUSTIBLE_PVE_TARGET_VMID=101
//!   export RUSTIBLE_PVE_CLONE_FROM=9000
//!   export RUSTIBLE_PVE_CREATE_PARAMS='{"memory":1024,"cores":1,"net0":"virtio,bridge=vmbr0"}'
//!   export RUSTIBLE_PVE_ALLOW_MUTATION=1
//!   export RUSTIBLE_PVE_CLEANUP=1
//!   export RUSTIBLE_PVE_CHECK_MODE=1
//!   cargo test --test proxmox_vm_tests -- --ignored

use std::collections::HashMap;
use std::env;

use rustible::modules::proxmox_vm::ProxmoxVmModule;
use rustible::modules::{Module, ModuleContext, ModuleParams, ModuleStatus};

struct ProxmoxTestConfig {
    api_url: String,
    token_id: String,
    token_secret: String,
    node: String,
    vmid: u64,
    validate_certs: Option<bool>,
    timeout_secs: Option<u64>,
    stop_method: Option<String>,
    name: Option<String>,
    description: Option<String>,
    tags: Option<String>,
    clone_from: Option<u64>,
    clone_full: Option<bool>,
    clone_target_node: Option<String>,
    clone_storage: Option<String>,
    clone_pool: Option<String>,
    clone_snapname: Option<String>,
}

impl ProxmoxTestConfig {
    fn from_env() -> Option<Self> {
        if !env_flag("RUSTIBLE_PVE_TESTS") {
            eprintln!("Skipping Proxmox tests (RUSTIBLE_PVE_TESTS not set)");
            return None;
        }

        let api_url = require_env("RUSTIBLE_PVE_API_URL")?;
        let token_id = require_env("RUSTIBLE_PVE_TOKEN_ID")?;
        let token_secret = require_env("RUSTIBLE_PVE_TOKEN_SECRET")?;
        let node = require_env("RUSTIBLE_PVE_NODE")?;
        let vmid = require_env("RUSTIBLE_PVE_VMID")?
            .parse::<u64>()
            .ok()?;

        let validate_certs = optional_bool_env("RUSTIBLE_PVE_VALIDATE_CERTS");
        let timeout_secs = optional_u64_env("RUSTIBLE_PVE_TIMEOUT");
        let stop_method = optional_string_env("RUSTIBLE_PVE_STOP_METHOD");
        let name = optional_string_env("RUSTIBLE_PVE_NAME");
        let description = optional_string_env("RUSTIBLE_PVE_DESCRIPTION");
        let tags = optional_string_env("RUSTIBLE_PVE_TAGS");
        let clone_from = optional_u64_env("RUSTIBLE_PVE_CLONE_FROM");
        let clone_full = optional_bool_env("RUSTIBLE_PVE_CLONE_FULL");
        let clone_target_node = optional_string_env("RUSTIBLE_PVE_CLONE_TARGET_NODE");
        let clone_storage = optional_string_env("RUSTIBLE_PVE_CLONE_STORAGE");
        let clone_pool = optional_string_env("RUSTIBLE_PVE_CLONE_POOL");
        let clone_snapname = optional_string_env("RUSTIBLE_PVE_CLONE_SNAPNAME");

        Some(Self {
            api_url,
            token_id,
            token_secret,
            node,
            vmid,
            validate_certs,
            timeout_secs,
            stop_method,
            name,
            description,
            tags,
            clone_from,
            clone_full,
            clone_target_node,
            clone_storage,
            clone_pool,
            clone_snapname,
        })
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| parse_bool(&value))
        .unwrap_or(false)
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn require_env(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("Missing required env var {}", name);
            None
        }
    }
}

fn optional_string_env(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

fn optional_u64_env(name: &str) -> Option<u64> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => value.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn optional_bool_env(name: &str) -> Option<bool> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(parse_bool(&value)),
        _ => None,
    }
}

fn optional_json_env(name: &str) -> Option<serde_json::Value> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => match serde_json::from_str::<serde_json::Value>(&value) {
            Ok(json) if json.is_object() => Some(json),
            Ok(_) => {
                eprintln!("{} must be a JSON object", name);
                None
            }
            Err(err) => {
                eprintln!("Invalid JSON for {}: {}", name, err);
                None
            }
        },
        _ => None,
    }
}

fn build_params_with_vmid(
    config: &ProxmoxTestConfig,
    vmid: u64,
    state: Option<&str>,
) -> ModuleParams {
    let mut params: ModuleParams = HashMap::new();
    params.insert("api_url".to_string(), serde_json::json!(config.api_url));
    params.insert(
        "api_token_id".to_string(),
        serde_json::json!(config.token_id),
    );
    params.insert(
        "api_token_secret".to_string(),
        serde_json::json!(config.token_secret),
    );
    params.insert("node".to_string(), serde_json::json!(config.node));
    params.insert("vmid".to_string(), serde_json::json!(vmid));

    if let Some(state) = state {
        params.insert("state".to_string(), serde_json::json!(state));
    }
    if let Some(validate_certs) = config.validate_certs {
        params.insert(
            "validate_certs".to_string(),
            serde_json::json!(validate_certs),
        );
    }
    if let Some(timeout) = config.timeout_secs {
        params.insert("timeout".to_string(), serde_json::json!(timeout));
    }
    if let Some(stop_method) = config.stop_method.as_ref() {
        params.insert(
            "stop_method".to_string(),
            serde_json::json!(stop_method),
        );
    }
    if let Some(name) = config.name.as_ref() {
        params.insert("name".to_string(), serde_json::json!(name));
    }
    if let Some(description) = config.description.as_ref() {
        params.insert(
            "description".to_string(),
            serde_json::json!(description),
        );
    }
    if let Some(tags) = config.tags.as_ref() {
        params.insert("tags".to_string(), serde_json::json!(tags));
    }
    if let Some(clone_from) = config.clone_from {
        params.insert("clone_from".to_string(), serde_json::json!(clone_from));
    }
    if let Some(clone_full) = config.clone_full {
        params.insert("clone_full".to_string(), serde_json::json!(clone_full));
    }
    if let Some(target_node) = config.clone_target_node.as_ref() {
        params.insert(
            "clone_target_node".to_string(),
            serde_json::json!(target_node),
        );
    }
    if let Some(storage) = config.clone_storage.as_ref() {
        params.insert(
            "clone_storage".to_string(),
            serde_json::json!(storage),
        );
    }
    if let Some(pool) = config.clone_pool.as_ref() {
        params.insert("clone_pool".to_string(), serde_json::json!(pool));
    }
    if let Some(snapname) = config.clone_snapname.as_ref() {
        params.insert(
            "clone_snapname".to_string(),
            serde_json::json!(snapname),
        );
    }

    params
}

fn build_params(config: &ProxmoxTestConfig, state: Option<&str>) -> ModuleParams {
    build_params_with_vmid(config, config.vmid, state)
}

#[test]
#[ignore = "Requires Proxmox API access; set RUSTIBLE_PVE_* env vars"]
fn test_proxmox_vm_status_and_optional_lifecycle() {
    let config = match ProxmoxTestConfig::from_env() {
        Some(config) => config,
        None => return,
    };

    let module = ProxmoxVmModule;
    let check_mode = env_flag("RUSTIBLE_PVE_CHECK_MODE");
    let context = ModuleContext::default().with_check_mode(check_mode);
    let params = build_params(&config, Some("status"));

    let output = module
        .execute(&params, &context)
        .expect("Proxmox status query failed");

    assert_eq!(output.status, ModuleStatus::Ok);
    let status = output
        .data
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    assert!(!status.is_empty());

    let allow_mutation =
        env_flag("RUSTIBLE_PVE_ALLOW_MUTATION") || env_flag("RUSTIBLE_PVE_ALLOW_POWER");
    if !allow_mutation {
        eprintln!("Skipping lifecycle actions (RUSTIBLE_PVE_ALLOW_MUTATION not set)");
        return;
    }

    let desired_state = match env::var("RUSTIBLE_PVE_DESIRED_STATE") {
        Ok(state) if !state.trim().is_empty() => state,
        _ => {
            eprintln!("Skipping lifecycle actions (RUSTIBLE_PVE_DESIRED_STATE not set)");
            return;
        }
    };

    if desired_state == "cloned" && config.clone_from.is_none() {
        eprintln!("Skipping clone (RUSTIBLE_PVE_CLONE_FROM not set)");
        return;
    }

    let mut params = build_params(&config, Some(&desired_state));
    if matches!(desired_state.as_str(), "present" | "cloned") && config.name.is_none() {
        params.insert("auto_name".to_string(), serde_json::json!(true));
    }
    let output = module
        .execute(&params, &context)
        .expect("Proxmox lifecycle action failed");

    assert!(matches!(
        output.status,
        ModuleStatus::Ok | ModuleStatus::Changed
    ));
}

#[test]
#[ignore = "Requires Proxmox API access; set RUSTIBLE_PVE_* env vars"]
fn test_proxmox_vm_present_clone_absent_safe() {
    let config = match ProxmoxTestConfig::from_env() {
        Some(config) => config,
        None => return,
    };

    let module = ProxmoxVmModule;
    let allow_mutation = env_flag("RUSTIBLE_PVE_ALLOW_MUTATION");
    let cleanup = env_flag("RUSTIBLE_PVE_CLEANUP");
    let check_mode = env_flag("RUSTIBLE_PVE_CHECK_MODE") || !allow_mutation;
    let context = ModuleContext::default().with_check_mode(check_mode);

    let target_vmid = optional_u64_env("RUSTIBLE_PVE_TARGET_VMID").unwrap_or(config.vmid);
    let create_params = optional_json_env("RUSTIBLE_PVE_CREATE_PARAMS");

    let mut params = build_params_with_vmid(&config, target_vmid, Some("present"));
    if config.name.is_none() {
        params.insert("auto_name".to_string(), serde_json::json!(true));
    }
    if let Some(create_params) = create_params.clone() {
        params.insert("create".to_string(), create_params);
    }

    if allow_mutation && !check_mode && create_params.is_none() {
        eprintln!("Skipping create (RUSTIBLE_PVE_CREATE_PARAMS not set)");
    } else {
        let output = module
            .execute(&params, &context)
            .expect("Proxmox create failed");
        assert!(matches!(
            output.status,
            ModuleStatus::Ok | ModuleStatus::Changed
        ));
    }

    if let Some(clone_from) = config.clone_from {
        let mut params = build_params_with_vmid(&config, target_vmid, Some("cloned"));
        params.insert("clone_from".to_string(), serde_json::json!(clone_from));
        if config.name.is_none() {
            params.insert("auto_name".to_string(), serde_json::json!(true));
        }
        let output = module
            .execute(&params, &context)
            .expect("Proxmox clone failed");
        assert!(matches!(
            output.status,
            ModuleStatus::Ok | ModuleStatus::Changed
        ));
    } else {
        eprintln!("Skipping clone (RUSTIBLE_PVE_CLONE_FROM not set)");
    }

    if check_mode || cleanup {
        let params = build_params_with_vmid(&config, target_vmid, Some("absent"));
        let output = module
            .execute(&params, &context)
            .expect("Proxmox delete failed");
        assert!(matches!(
            output.status,
            ModuleStatus::Ok | ModuleStatus::Changed
        ));
    } else {
        eprintln!("Skipping delete (RUSTIBLE_PVE_CLEANUP not set)");
    }
}
