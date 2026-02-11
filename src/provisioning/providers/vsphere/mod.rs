//! vSphere Provider Implementation (Experimental)
//!
//! This module provides VMware vSphere virtual machine management by shelling
//! out to the `govc` CLI tool (<https://github.com/vmware/govmomi/tree/main/govc>).
//! All JSON output is parsed from `govc -json` invocations.
//!
//! **This provider is experimental.** The vSphere feature is always compiled
//! but `govc` must be installed and reachable on `$PATH` for operations to
//! succeed at runtime.
//!
//! ## Configuration
//!
//! ```yaml
//! providers:
//!   vsphere:
//!     url: "https://vcenter.example.com/sdk"
//!     username: "administrator@vsphere.local"
//!     password: "changeme"
//!     datacenter: "DC0"
//!     insecure: true  # skip TLS verification
//! ```
//!
//! ## Resource Types
//!
//! - `vsphere_vm` - Virtual machine lifecycle via govc
//!
//! ## Environment
//!
//! The provider sets the following environment variables for `govc`:
//! - `GOVC_URL`
//! - `GOVC_USERNAME`
//! - `GOVC_PASSWORD`
//! - `GOVC_DATACENTER`
//! - `GOVC_INSECURE`

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, DataSource, FieldConstraint, FieldType, Provider, ProviderConfig, ProviderContext,
    ProviderCredentials, ProviderSchema, Resource, ResourceDependency, ResourceDiff,
    ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts, RetryConfig, SchemaField,
};

// ============================================================================
// Constants
// ============================================================================

const PROVIDER_NAME: &str = "vsphere";
const PROVIDER_VERSION: &str = "0.1.0-experimental";
const DEFAULT_TIMEOUT: u64 = 600;

// ============================================================================
// Credentials
// ============================================================================

/// vSphere credentials (vCenter URL + username + password).
#[derive(Debug, Clone)]
pub struct VsphereCredentials {
    pub url: String,
    pub username: String,
    pub datacenter: Option<String>,
}

impl ProviderCredentials for VsphereCredentials {
    fn credential_type(&self) -> &str {
        "vsphere_govc"
    }

    fn is_expired(&self) -> bool {
        false
    }

    fn as_value(&self) -> Value {
        serde_json::json!({
            "type": "vsphere_govc",
            "url": self.url,
            "username": self.username,
            "datacenter": self.datacenter,
        })
    }
}

// ============================================================================
// Provider
// ============================================================================

/// VMware vSphere provider (experimental).
///
/// Delegates to the `govc` CLI for all vCenter interactions.
pub struct VsphereProvider {
    name: String,
    url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    datacenter: Option<String>,
    insecure: bool,
    config: Value,
    resources: HashMap<String, Arc<dyn Resource>>,
    timeout_seconds: u64,
}

impl Default for VsphereProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl VsphereProvider {
    pub fn new() -> Self {
        Self {
            name: PROVIDER_NAME.to_string(),
            url: None,
            username: None,
            password: None,
            datacenter: None,
            insecure: false,
            config: Value::Null,
            resources: HashMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT,
        }
    }

    /// Build the environment variables map for govc subprocesses.
    fn govc_env(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        if let Some(ref url) = self.url {
            env.insert("GOVC_URL".to_string(), url.clone());
        }
        if let Some(ref user) = self.username {
            env.insert("GOVC_USERNAME".to_string(), user.clone());
        }
        if let Some(ref pass) = self.password {
            env.insert("GOVC_PASSWORD".to_string(), pass.clone());
        }
        if let Some(ref dc) = self.datacenter {
            env.insert("GOVC_DATACENTER".to_string(), dc.clone());
        }
        if self.insecure {
            env.insert("GOVC_INSECURE".to_string(), "true".to_string());
        }
        env
    }
}

impl std::fmt::Debug for VsphereProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VsphereProvider")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("username", &self.username)
            .field("datacenter", &self.datacenter)
            .field("insecure", &self.insecure)
            .field("resources", &self.resources.keys().collect::<Vec<_>>())
            .field("timeout_seconds", &self.timeout_seconds)
            .finish()
    }
}

#[async_trait]
impl Provider for VsphereProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        PROVIDER_VERSION
    }

    fn config_schema(&self) -> ProviderSchema {
        ProviderSchema {
            name: PROVIDER_NAME.to_string(),
            version: PROVIDER_VERSION.to_string(),
            required_fields: vec![
                SchemaField {
                    name: "url".to_string(),
                    field_type: FieldType::String,
                    description: "vCenter SDK URL (e.g. https://vcenter/sdk)".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "username".to_string(),
                    field_type: FieldType::String,
                    description: "vSphere username".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "password".to_string(),
                    field_type: FieldType::String,
                    description: "vSphere password".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
            ],
            optional_fields: vec![
                SchemaField {
                    name: "datacenter".to_string(),
                    field_type: FieldType::String,
                    description: "Default datacenter name".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "insecure".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Skip TLS certificate verification".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "timeout".to_string(),
                    field_type: FieldType::Integer,
                    description: "Command timeout in seconds".to_string(),
                    default: Some(Value::Number(DEFAULT_TIMEOUT.into())),
                    constraints: vec![
                        FieldConstraint::MinValue { value: 30 },
                        FieldConstraint::MaxValue { value: 3600 },
                    ],
                    sensitive: false,
                },
            ],
            regions: None,
        }
    }

    async fn configure(&mut self, config: ProviderConfig) -> ProvisioningResult<()> {
        info!("Configuring vSphere provider (experimental)");

        let settings = &config.settings;

        self.url = Some(
            settings
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ProvisioningError::provider_config(PROVIDER_NAME, "Missing required field: url")
                })?
                .to_string(),
        );

        self.username = Some(
            settings
                .get("username")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ProvisioningError::provider_config(
                        PROVIDER_NAME,
                        "Missing required field: username",
                    )
                })?
                .to_string(),
        );

        self.password = Some(
            settings
                .get("password")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ProvisioningError::provider_config(
                        PROVIDER_NAME,
                        "Missing required field: password",
                    )
                })?
                .to_string(),
        );

        self.datacenter = settings
            .get("datacenter")
            .and_then(|v| v.as_str())
            .map(String::from);

        self.insecure = settings
            .get("insecure")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if let Some(t) = settings.get("timeout").and_then(|v| v.as_u64()) {
            self.timeout_seconds = t;
        }

        self.config = config.settings.clone();

        // Register the VM resource
        let govc_env = self.govc_env();
        self.resources.insert(
            "vsphere_vm".to_string(),
            Arc::new(VsphereVmResource {
                govc_env,
                timeout_seconds: self.timeout_seconds,
            }),
        );

        info!(
            "vSphere provider configured for: {} (datacenter: {:?})",
            self.url.as_deref().unwrap_or("?"),
            self.datacenter
        );
        Ok(())
    }

    fn resource(&self, resource_type: &str) -> ProvisioningResult<Arc<dyn Resource>> {
        self.resources
            .get(resource_type)
            .cloned()
            .ok_or_else(|| ProvisioningError::resource_not_found(PROVIDER_NAME, resource_type))
    }

    fn data_source(&self, ds_type: &str) -> ProvisioningResult<Arc<dyn DataSource>> {
        Err(ProvisioningError::resource_not_found(
            PROVIDER_NAME,
            format!("data.{}", ds_type),
        ))
    }

    fn resource_types(&self) -> Vec<String> {
        vec!["vsphere_vm".to_string()]
    }

    fn data_source_types(&self) -> Vec<String> {
        Vec::new()
    }

    fn validate_config(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["url", "username", "password"] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    format!("{} is required", field),
                ));
            }
        }
        Ok(())
    }

    fn context(&self) -> ProvisioningResult<ProviderContext> {
        let url = self.url.as_ref().ok_or_else(|| {
            ProvisioningError::provider_config(
                PROVIDER_NAME,
                "Provider not configured. Call configure() first.",
            )
        })?;

        let creds = VsphereCredentials {
            url: url.clone(),
            username: self.username.clone().unwrap_or_default(),
            datacenter: self.datacenter.clone(),
        };

        Ok(ProviderContext {
            provider: PROVIDER_NAME.to_string(),
            region: self.datacenter.clone(),
            config: self.config.clone(),
            credentials: Arc::new(creds),
            timeout_seconds: self.timeout_seconds,
            retry_config: RetryConfig {
                max_retries: 2,
                initial_backoff_ms: 3000,
                max_backoff_ms: 30000,
                backoff_multiplier: 2.0,
            },
            default_tags: HashMap::new(),
        })
    }
}

// ============================================================================
// govc helper
// ============================================================================

/// Run a `govc` command and return parsed JSON output.
async fn run_govc(
    args: &[&str],
    env: &HashMap<String, String>,
    timeout_seconds: u64,
) -> ProvisioningResult<Value> {
    use tokio::process::Command;

    debug!("govc {}", args.join(" "));

    let mut cmd = Command::new("govc");
    cmd.args(args).arg("-json");

    for (k, v) in env {
        cmd.env(k, v);
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_seconds),
        cmd.output(),
    )
    .await
    .map_err(|_| ProvisioningError::Timeout {
        operation: format!("govc {}", args.first().unwrap_or(&"")),
        seconds: timeout_seconds,
    })?
    .map_err(|e| {
        ProvisioningError::CloudApiError(format!(
            "Failed to execute govc: {}. Is govc installed and on PATH?",
            e
        ))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProvisioningError::CloudApiError(format!(
            "govc {} failed (exit {}): {}",
            args.first().unwrap_or(&""),
            output.status,
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_str(stdout.trim()).map_err(|e| {
        ProvisioningError::CloudApiError(format!(
            "Failed to parse govc JSON output: {}",
            e
        ))
    })
}

/// Run a `govc` command without expecting JSON output (fire-and-forget).
async fn run_govc_no_json(
    args: &[&str],
    env: &HashMap<String, String>,
    timeout_seconds: u64,
) -> ProvisioningResult<()> {
    use tokio::process::Command;

    debug!("govc {}", args.join(" "));

    let mut cmd = Command::new("govc");
    cmd.args(args);

    for (k, v) in env {
        cmd.env(k, v);
    }

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_seconds),
        cmd.output(),
    )
    .await
    .map_err(|_| ProvisioningError::Timeout {
        operation: format!("govc {}", args.first().unwrap_or(&"")),
        seconds: timeout_seconds,
    })?
    .map_err(|e| {
        ProvisioningError::CloudApiError(format!("Failed to execute govc: {}", e))
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProvisioningError::CloudApiError(format!(
            "govc {} failed (exit {}): {}",
            args.first().unwrap_or(&""),
            output.status,
            stderr.trim()
        )));
    }

    Ok(())
}

// ============================================================================
// vSphere VM Resource
// ============================================================================

/// Manages VMs on vSphere via `govc` commands.
#[derive(Debug, Clone)]
pub struct VsphereVmResource {
    govc_env: HashMap<String, String>,
    timeout_seconds: u64,
}

#[async_trait]
impl Resource for VsphereVmResource {
    fn resource_type(&self) -> &str {
        "vsphere_vm"
    }

    fn provider(&self) -> &str {
        PROVIDER_NAME
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "vsphere_vm".to_string(),
            description: "VMware vSphere virtual machine (managed via govc CLI)".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "VM name".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "template".to_string(),
                    field_type: FieldType::String,
                    description: "Template or VM to clone from".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "num_cpus".to_string(),
                    field_type: FieldType::Integer,
                    description: "Number of virtual CPUs".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 1 },
                        FieldConstraint::MaxValue { value: 256 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "memory_mb".to_string(),
                    field_type: FieldType::Integer,
                    description: "Memory in megabytes".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 128 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "datastore".to_string(),
                    field_type: FieldType::String,
                    description: "Target datastore".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "resource_pool".to_string(),
                    field_type: FieldType::String,
                    description: "Resource pool path".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "folder".to_string(),
                    field_type: FieldType::String,
                    description: "VM folder path".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "network".to_string(),
                    field_type: FieldType::String,
                    description: "Network to attach".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "power_on".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Power on after creation".to_string(),
                    default: Some(Value::Bool(true)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "annotation".to_string(),
                    field_type: FieldType::String,
                    description: "VM annotation / notes".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "uuid".to_string(),
                    field_type: FieldType::String,
                    description: "VM instance UUID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "power_state".to_string(),
                    field_type: FieldType::String,
                    description: "Current power state".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "guest_ip".to_string(),
                    field_type: FieldType::String,
                    description: "Guest OS primary IP address".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["template".to_string()],
            timeouts: ResourceTimeouts {
                create: 600,
                read: 60,
                update: 300,
                delete: 300,
            },
        }
    }

    async fn read(
        &self,
        id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        debug!("vSphere VM read: {}", id);

        let data = match run_govc(&["vm.info", id], &self.govc_env, self.timeout_seconds).await {
            Ok(d) => d,
            Err(_) => return Ok(ResourceReadResult::not_found()),
        };

        // govc vm.info -json wraps in {"virtualMachines": [...]}
        let vms = data
            .get("virtualMachines")
            .or_else(|| data.get("VirtualMachines"))
            .and_then(|v| v.as_array());

        let vm = match vms.and_then(|arr| arr.first()) {
            Some(v) => v,
            None => return Ok(ResourceReadResult::not_found()),
        };

        let config_section = vm.get("Config").or_else(|| vm.get("config"));
        let runtime_section = vm.get("Runtime").or_else(|| vm.get("runtime"));
        let guest_section = vm.get("Guest").or_else(|| vm.get("guest"));

        let uuid = config_section
            .and_then(|c| c.get("Uuid").or_else(|| c.get("uuid")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let name = config_section
            .and_then(|c| c.get("Name").or_else(|| c.get("name")))
            .and_then(|v| v.as_str())
            .unwrap_or(id)
            .to_string();

        let num_cpus = config_section
            .and_then(|c| c.get("Hardware").or_else(|| c.get("hardware")))
            .and_then(|h| h.get("NumCPU").or_else(|| h.get("numCPU")))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let memory_mb = config_section
            .and_then(|c| c.get("Hardware").or_else(|| c.get("hardware")))
            .and_then(|h| h.get("MemoryMB").or_else(|| h.get("memoryMB")))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let power_state = runtime_section
            .and_then(|r| r.get("PowerState").or_else(|| r.get("powerState")))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let guest_ip = guest_section
            .and_then(|g| g.get("IpAddress").or_else(|| g.get("ipAddress")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let annotation = config_section
            .and_then(|c| c.get("Annotation").or_else(|| c.get("annotation")))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let attrs = serde_json::json!({
            "name": name,
            "uuid": uuid,
            "num_cpus": num_cpus,
            "memory_mb": memory_mb,
            "power_state": power_state,
            "guest_ip": guest_ip,
            "annotation": annotation,
        });

        let cloud_id = if uuid.is_empty() { id.to_string() } else { uuid };
        Ok(ResourceReadResult::found(cloud_id, attrs))
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        match current {
            None => Ok(ResourceDiff::create(desired.clone())),
            Some(cur) => {
                let mut modifications = HashMap::new();
                let mut requires_replacement = false;
                let mut replacement_fields = Vec::new();

                // Template change forces replacement
                if let (Some(d_tpl), Some(_c_tpl)) = (
                    desired.get("template").and_then(|v| v.as_str()),
                    cur.get("template").and_then(|v| v.as_str()),
                ) {
                    // If template is specified in desired and differs, replace
                    if desired.get("template") != cur.get("template") {
                        requires_replacement = true;
                        replacement_fields.push("template".to_string());
                    }
                    let _ = d_tpl; // suppress unused warning
                }

                // In-place updatable fields
                for field in &["num_cpus", "memory_mb", "annotation"] {
                    if let (Some(d), Some(c)) = (desired.get(*field), cur.get(*field)) {
                        if d != c {
                            modifications.insert(field.to_string(), (c.clone(), d.clone()));
                        }
                    }
                }

                if modifications.is_empty() && !requires_replacement {
                    return Ok(ResourceDiff::no_change());
                }

                let change_type = if requires_replacement {
                    ChangeType::Replace
                } else {
                    ChangeType::Update
                };

                Ok(ResourceDiff {
                    change_type,
                    additions: HashMap::new(),
                    modifications,
                    deletions: Vec::new(),
                    requires_replacement,
                    replacement_fields,
                })
            }
        }
    }

    async fn create(
        &self,
        config: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("name is required".to_string())
            })?;

        // Build govc vm.clone args
        let template = config
            .get("template")
            .and_then(|v| v.as_str());

        if let Some(tpl) = template {
            let mut args: Vec<String> = vec![
                "vm.clone".to_string(),
                "-vm".to_string(),
                tpl.to_string(),
            ];

            if let Some(ds) = config.get("datastore").and_then(|v| v.as_str()) {
                args.push("-ds".to_string());
                args.push(ds.to_string());
            }
            if let Some(pool) = config.get("resource_pool").and_then(|v| v.as_str()) {
                args.push("-pool".to_string());
                args.push(pool.to_string());
            }
            if let Some(folder) = config.get("folder").and_then(|v| v.as_str()) {
                args.push("-folder".to_string());
                args.push(folder.to_string());
            }

            let power_on = config
                .get("power_on")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if power_on {
                args.push("-on".to_string());
            }

            args.push(name.to_string());

            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            info!("Creating vSphere VM from template: {} -> {}", tpl, name);
            run_govc_no_json(&arg_refs, &self.govc_env, self.timeout_seconds).await?;
        } else {
            // Create an empty VM (less common, but supported)
            warn!("Creating vSphere VM without a template is limited");
            let args: Vec<String> = vec!["vm.create".to_string(), name.to_string()];
            let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            run_govc_no_json(&arg_refs, &self.govc_env, self.timeout_seconds).await?;
        }

        // Apply CPU/memory changes if specified
        let mut change_args: Vec<String> = vec!["vm.change".to_string(), "-vm".to_string(), name.to_string()];
        let mut has_changes = false;

        if let Some(cpus) = config.get("num_cpus").and_then(|v| v.as_u64()) {
            change_args.push("-c".to_string());
            change_args.push(cpus.to_string());
            has_changes = true;
        }
        if let Some(mem) = config.get("memory_mb").and_then(|v| v.as_u64()) {
            change_args.push("-m".to_string());
            change_args.push(mem.to_string());
            has_changes = true;
        }
        if let Some(annotation) = config.get("annotation").and_then(|v| v.as_str()) {
            change_args.push("-annotation".to_string());
            change_args.push(annotation.to_string());
            has_changes = true;
        }

        if has_changes {
            let arg_refs: Vec<&str> = change_args.iter().map(|s| s.as_str()).collect();
            run_govc_no_json(&arg_refs, &self.govc_env, self.timeout_seconds).await?;
        }

        // Attach network if specified
        if let Some(network) = config.get("network").and_then(|v| v.as_str()) {
            let net_args = vec!["vm.network.add", "-vm", name, "-net", network];
            let _ = run_govc_no_json(&net_args, &self.govc_env, self.timeout_seconds).await;
        }

        let attrs = serde_json::json!({
            "name": name,
            "template": template.unwrap_or(""),
            "num_cpus": config.get("num_cpus").and_then(|v| v.as_u64()).unwrap_or(0),
            "memory_mb": config.get("memory_mb").and_then(|v| v.as_u64()).unwrap_or(0),
            "power_state": if config.get("power_on").and_then(|v| v.as_bool()).unwrap_or(true) {
                "poweredOn"
            } else {
                "poweredOff"
            },
        });

        Ok(ResourceResult::success(name, attrs))
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let name = new
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(id);

        let mut change_args: Vec<String> = vec!["vm.change".to_string(), "-vm".to_string(), name.to_string()];
        let mut has_changes = false;

        if let Some(cpus) = new.get("num_cpus").and_then(|v| v.as_u64()) {
            change_args.push("-c".to_string());
            change_args.push(cpus.to_string());
            has_changes = true;
        }
        if let Some(mem) = new.get("memory_mb").and_then(|v| v.as_u64()) {
            change_args.push("-m".to_string());
            change_args.push(mem.to_string());
            has_changes = true;
        }
        if let Some(annotation) = new.get("annotation").and_then(|v| v.as_str()) {
            change_args.push("-annotation".to_string());
            change_args.push(annotation.to_string());
            has_changes = true;
        }

        if has_changes {
            info!("Updating vSphere VM: {}", name);
            let arg_refs: Vec<&str> = change_args.iter().map(|s| s.as_str()).collect();
            run_govc_no_json(&arg_refs, &self.govc_env, self.timeout_seconds).await?;
        }

        let attrs = serde_json::json!({
            "name": name,
            "num_cpus": new.get("num_cpus").and_then(|v| v.as_u64()).unwrap_or(0),
            "memory_mb": new.get("memory_mb").and_then(|v| v.as_u64()).unwrap_or(0),
        });

        Ok(ResourceResult::success(id, attrs))
    }

    async fn destroy(
        &self,
        id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        info!("Destroying vSphere VM: {}", id);

        // Power off first (ignore error if already off)
        let _ = run_govc_no_json(
            &["vm.power", "-off", "-force", id],
            &self.govc_env,
            self.timeout_seconds,
        )
        .await;

        // Destroy
        run_govc_no_json(
            &["vm.destroy", id],
            &self.govc_env,
            self.timeout_seconds,
        )
        .await?;

        Ok(ResourceResult::success(
            id,
            serde_json::json!({"name": id, "status": "destroyed"}),
        ))
    }

    async fn import(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let read_result = self.read(id, ctx).await?;
        if !read_result.exists {
            return Err(ProvisioningError::ImportError {
                resource_type: "vsphere_vm".to_string(),
                resource_id: id.to_string(),
                message: "VM not found in vSphere inventory".to_string(),
            });
        }
        Ok(ResourceResult::success(
            read_result.cloud_id.unwrap_or_else(|| id.to_string()),
            read_result.attributes,
        ))
    }

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        Vec::new()
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["template".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        if config.get("name").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::ValidationError(
                "name is required for vsphere_vm".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_metadata() {
        let provider = VsphereProvider::new();
        assert_eq!(provider.name(), "vsphere");
        assert!(provider.version().contains("experimental"));
    }

    #[test]
    fn test_config_schema() {
        let provider = VsphereProvider::new();
        let schema = provider.config_schema();
        assert_eq!(schema.name, "vsphere");
        assert_eq!(schema.required_fields.len(), 3);
        assert!(schema.regions.is_none());

        let pw = schema.required_fields.iter().find(|f| f.name == "password");
        assert!(pw.is_some());
        assert!(pw.unwrap().sensitive);
    }

    #[test]
    fn test_resource_types() {
        let provider = VsphereProvider::new();
        assert_eq!(provider.resource_types(), vec!["vsphere_vm".to_string()]);
    }

    #[test]
    fn test_validate_config_valid() {
        let provider = VsphereProvider::new();
        let config = serde_json::json!({
            "url": "https://vcenter/sdk",
            "username": "admin@vsphere.local",
            "password": "secret"
        });
        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_missing_url() {
        let provider = VsphereProvider::new();
        let config = serde_json::json!({
            "username": "admin",
            "password": "secret"
        });
        assert!(provider.validate_config(&config).is_err());
    }

    #[test]
    fn test_context_not_configured() {
        let provider = VsphereProvider::new();
        assert!(provider.context().is_err());
    }

    #[tokio::test]
    async fn test_configure() {
        let mut provider = VsphereProvider::new();
        let config = ProviderConfig {
            name: "vsphere".to_string(),
            region: None,
            settings: serde_json::json!({
                "url": "https://vcenter/sdk",
                "username": "admin@vsphere.local",
                "password": "secret",
                "datacenter": "DC0",
                "insecure": true,
                "timeout": 120
            }),
        };

        let result = provider.configure(config).await;
        assert!(result.is_ok());
        assert_eq!(provider.url, Some("https://vcenter/sdk".to_string()));
        assert_eq!(provider.datacenter, Some("DC0".to_string()));
        assert!(provider.insecure);
        assert_eq!(provider.timeout_seconds, 120);

        let ctx = provider.context();
        assert!(ctx.is_ok());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.provider, "vsphere");
        assert_eq!(ctx.region, Some("DC0".to_string()));

        assert!(provider.resource("vsphere_vm").is_ok());
        assert!(provider.resource("nonexistent").is_err());
    }

    #[test]
    fn test_govc_env() {
        let mut provider = VsphereProvider::new();
        provider.url = Some("https://vcenter/sdk".to_string());
        provider.username = Some("admin".to_string());
        provider.password = Some("secret".to_string());
        provider.datacenter = Some("DC0".to_string());
        provider.insecure = true;

        let env = provider.govc_env();
        assert_eq!(env.get("GOVC_URL"), Some(&"https://vcenter/sdk".to_string()));
        assert_eq!(env.get("GOVC_USERNAME"), Some(&"admin".to_string()));
        assert_eq!(env.get("GOVC_PASSWORD"), Some(&"secret".to_string()));
        assert_eq!(env.get("GOVC_DATACENTER"), Some(&"DC0".to_string()));
        assert_eq!(env.get("GOVC_INSECURE"), Some(&"true".to_string()));
    }

    #[test]
    fn test_vm_resource_schema() {
        let res = VsphereVmResource {
            govc_env: HashMap::new(),
            timeout_seconds: 60,
        };
        let schema = res.schema();
        assert_eq!(schema.resource_type, "vsphere_vm");
        assert_eq!(schema.required_args.len(), 1);
        assert_eq!(schema.required_args[0].name, "name");
    }

    #[test]
    fn test_vm_validate() {
        let res = VsphereVmResource {
            govc_env: HashMap::new(),
            timeout_seconds: 60,
        };
        assert!(res.validate(&serde_json::json!({"name": "my-vm"})).is_ok());
        assert!(res.validate(&serde_json::json!({})).is_err());
    }

    #[test]
    fn test_credentials() {
        let creds = VsphereCredentials {
            url: "https://vcenter/sdk".to_string(),
            username: "admin".to_string(),
            datacenter: Some("DC0".to_string()),
        };
        assert_eq!(creds.credential_type(), "vsphere_govc");
        assert!(!creds.is_expired());

        let val = creds.as_value();
        assert_eq!(val.get("type").and_then(|v| v.as_str()), Some("vsphere_govc"));
        // Password should not appear
        assert!(val.get("password").is_none());
    }

    #[test]
    fn test_provider_debug() {
        let provider = VsphereProvider::new();
        let debug_str = format!("{:?}", provider);
        assert!(debug_str.contains("VsphereProvider"));
    }
}
