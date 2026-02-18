//! Redfish Provider Implementation
//!
//! This module provides bare-metal server management via the DMTF Redfish REST API.
//! It supports power control, boot device configuration, and hardware inventory
//! reads against any BMC (Baseboard Management Controller) that exposes the
//! standard Redfish `/redfish/v1/Systems/{SystemId}` endpoint.
//!
//! ## Configuration
//!
//! ```yaml
//! providers:
//!   redfish:
//!     endpoint: "https://bmc.example.com"
//!     username: "admin"
//!     password: "changeme"
//! ```
//!
//! ## Resource Types
//!
//! - `redfish_machine` - Manage a single server's power state and boot order
//!
//! ## Feature Gate
//!
//! Real HTTP calls require the `redfish` Cargo feature. Without it the provider
//! compiles but every operation returns a stub error.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::{debug, info};

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, DataSource, FieldConstraint, FieldType, Provider, ProviderConfig, ProviderContext,
    ProviderCredentials, ProviderSchema, Resource, ResourceDependency, ResourceDiff,
    ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts, RetryConfig, SchemaField,
};

// ============================================================================
// Constants
// ============================================================================

const PROVIDER_NAME: &str = "redfish";
const PROVIDER_VERSION: &str = "0.1.0";
const DEFAULT_TIMEOUT: u64 = 120;

// ============================================================================
// Credentials
// ============================================================================

/// Basic-auth credentials for Redfish BMC access.
#[derive(Debug, Clone)]
pub struct RedfishCredentials {
    pub endpoint: String,
    pub username: String,
    pub password: String,
}

impl ProviderCredentials for RedfishCredentials {
    fn credential_type(&self) -> &str {
        "redfish_basic"
    }

    fn is_expired(&self) -> bool {
        false
    }

    fn as_value(&self) -> Value {
        serde_json::json!({
            "type": "redfish_basic",
            "endpoint": self.endpoint,
            "username": self.username,
        })
    }
}

// ============================================================================
// Provider
// ============================================================================

/// Redfish bare-metal provider.
///
/// Manages servers through the DMTF Redfish REST interface exposed by the
/// BMC/iLO/iDRAC/etc.
pub struct RedfishProvider {
    name: String,
    endpoint: Option<String>,
    username: Option<String>,
    password: Option<String>,
    config: Value,
    resources: HashMap<String, Arc<dyn Resource>>,
    timeout_seconds: u64,
    verify_ssl: bool,
}

impl Default for RedfishProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl RedfishProvider {
    /// Create a new, unconfigured Redfish provider.
    pub fn new() -> Self {
        Self {
            name: PROVIDER_NAME.to_string(),
            endpoint: None,
            username: None,
            password: None,
            config: Value::Null,
            resources: HashMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT,
            verify_ssl: false,
        }
    }
}

impl std::fmt::Debug for RedfishProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedfishProvider")
            .field("name", &self.name)
            .field("endpoint", &self.endpoint)
            .field("username", &self.username)
            .field("resources", &self.resources.keys().collect::<Vec<_>>())
            .field("timeout_seconds", &self.timeout_seconds)
            .finish()
    }
}

#[async_trait]
impl Provider for RedfishProvider {
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
                    name: "endpoint".to_string(),
                    field_type: FieldType::String,
                    description: "Redfish BMC base URL (e.g. https://bmc.example.com)".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "username".to_string(),
                    field_type: FieldType::String,
                    description: "BMC username for basic auth".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "password".to_string(),
                    field_type: FieldType::String,
                    description: "BMC password for basic auth".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
            ],
            optional_fields: vec![
                SchemaField {
                    name: "timeout".to_string(),
                    field_type: FieldType::Integer,
                    description: "HTTP request timeout in seconds".to_string(),
                    default: Some(Value::Number(DEFAULT_TIMEOUT.into())),
                    constraints: vec![
                        FieldConstraint::MinValue { value: 10 },
                        FieldConstraint::MaxValue { value: 600 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "verify_ssl".to_string(),
                    field_type: FieldType::Boolean,
                    description:
                        "Verify TLS certificates (default: false, BMCs often use self-signed certs)"
                            .to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            regions: None,
        }
    }

    async fn configure(&mut self, config: ProviderConfig) -> ProvisioningResult<()> {
        info!("Configuring Redfish provider");

        let settings = &config.settings;

        let endpoint = settings
            .get("endpoint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    "Missing required field: endpoint",
                )
            })?
            .to_string();

        let username = settings
            .get("username")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    "Missing required field: username",
                )
            })?
            .to_string();

        let password = settings
            .get("password")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::provider_config(
                    PROVIDER_NAME,
                    "Missing required field: password",
                )
            })?
            .to_string();

        if let Some(t) = settings.get("timeout").and_then(|v| v.as_u64()) {
            self.timeout_seconds = t;
        }

        if let Some(v) = settings.get("verify_ssl").and_then(|v| v.as_bool()) {
            self.verify_ssl = v;
        }

        self.endpoint = Some(endpoint.clone());
        self.username = Some(username);
        self.password = Some(password);
        self.config = config.settings.clone();

        // Register resources
        self.resources.insert(
            "redfish_machine".to_string(),
            Arc::new(RedfishMachineResource {
                endpoint: endpoint.clone(),
            }),
        );

        info!("Redfish provider configured for endpoint: {}", endpoint);
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
        vec!["redfish_machine".to_string()]
    }

    fn data_source_types(&self) -> Vec<String> {
        Vec::new()
    }

    fn validate_config(&self, config: &Value) -> ProvisioningResult<()> {
        if config.get("endpoint").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::provider_config(
                PROVIDER_NAME,
                "endpoint is required",
            ));
        }
        if config.get("username").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::provider_config(
                PROVIDER_NAME,
                "username is required",
            ));
        }
        if config.get("password").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::provider_config(
                PROVIDER_NAME,
                "password is required",
            ));
        }
        Ok(())
    }

    fn context(&self) -> ProvisioningResult<ProviderContext> {
        let endpoint = self.endpoint.as_ref().ok_or_else(|| {
            ProvisioningError::provider_config(
                PROVIDER_NAME,
                "Provider not configured. Call configure() first.",
            )
        })?;

        let creds = RedfishCredentials {
            endpoint: endpoint.clone(),
            username: self.username.clone().unwrap_or_default(),
            password: self.password.clone().unwrap_or_default(),
        };

        Ok(ProviderContext {
            provider: PROVIDER_NAME.to_string(),
            region: None,
            config: self.config.clone(),
            credentials: Arc::new(creds),
            timeout_seconds: self.timeout_seconds,
            retry_config: RetryConfig {
                max_retries: 2,
                initial_backoff_ms: 2000,
                max_backoff_ms: 15000,
                backoff_multiplier: 2.0,
            },
            default_tags: HashMap::new(),
        })
    }
}

// ============================================================================
// Redfish Machine Resource
// ============================================================================

/// Manages a single Redfish-controlled server (power state, boot device, inventory).
#[derive(Debug, Clone)]
pub struct RedfishMachineResource {
    endpoint: String,
}

impl RedfishMachineResource {
    /// Build the Redfish system URL for the given `system_id`.
    fn system_url(&self, ctx: &ProviderContext, system_id: &str) -> String {
        let base = ctx
            .config
            .get("endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.endpoint);
        let base = base.trim_end_matches('/');
        format!("{}/redfish/v1/Systems/{}", base, system_id)
    }

    // ------------------------------------------------------------------
    // Feature-gated HTTP helpers
    // ------------------------------------------------------------------

    #[cfg(feature = "redfish")]
    async fn http_get(&self, url: &str, ctx: &ProviderContext) -> ProvisioningResult<Value> {
        let creds = ctx.credentials.as_value();
        let username = creds
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        // Recover password from context config (not serialised in as_value for safety).
        let password = ctx
            .config
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let verify_ssl = ctx
            .config
            .get("verify_ssl")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(!verify_ssl)
            .timeout(std::time::Duration::from_secs(ctx.timeout_seconds))
            .build()
            .map_err(|e| ProvisioningError::CloudApiError(format!("HTTP client error: {}", e)))?;

        let resp = client
            .get(url)
            .basic_auth(&username, Some(&password))
            .send()
            .await
            .map_err(|e| ProvisioningError::CloudApiError(format!("Redfish GET {}: {}", url, e)))?;

        if !resp.status().is_success() {
            return Err(ProvisioningError::CloudApiError(format!(
                "Redfish GET {} returned {}",
                url,
                resp.status()
            )));
        }

        resp.json::<Value>()
            .await
            .map_err(|e| ProvisioningError::CloudApiError(format!("JSON parse error: {}", e)))
    }

    #[cfg(not(feature = "redfish"))]
    async fn http_get(&self, _url: &str, _ctx: &ProviderContext) -> ProvisioningResult<Value> {
        Err(ProvisioningError::ConfigError(
            "Redfish feature not enabled. Rebuild with --features redfish".to_string(),
        ))
    }

    #[cfg(feature = "redfish")]
    async fn http_post(
        &self,
        url: &str,
        body: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<()> {
        let creds = ctx.credentials.as_value();
        let username = creds
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let password = ctx
            .config
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let verify_ssl = ctx
            .config
            .get("verify_ssl")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(!verify_ssl)
            .timeout(std::time::Duration::from_secs(ctx.timeout_seconds))
            .build()
            .map_err(|e| ProvisioningError::CloudApiError(format!("HTTP client error: {}", e)))?;

        let resp = client
            .post(url)
            .basic_auth(&username, Some(&password))
            .json(body)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Redfish POST {}: {}", url, e))
            })?;

        if !resp.status().is_success() {
            return Err(ProvisioningError::CloudApiError(format!(
                "Redfish POST {} returned {}",
                url,
                resp.status()
            )));
        }

        Ok(())
    }

    #[cfg(not(feature = "redfish"))]
    async fn http_post(
        &self,
        _url: &str,
        _body: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<()> {
        Err(ProvisioningError::ConfigError(
            "Redfish feature not enabled. Rebuild with --features redfish".to_string(),
        ))
    }

    #[cfg(feature = "redfish")]
    async fn http_patch(
        &self,
        url: &str,
        body: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<()> {
        let creds = ctx.credentials.as_value();
        let username = creds
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let password = ctx
            .config
            .get("password")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let verify_ssl = ctx
            .config
            .get("verify_ssl")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(!verify_ssl)
            .timeout(std::time::Duration::from_secs(ctx.timeout_seconds))
            .build()
            .map_err(|e| ProvisioningError::CloudApiError(format!("HTTP client error: {}", e)))?;

        let resp = client
            .patch(url)
            .basic_auth(&username, Some(&password))
            .json(body)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Redfish PATCH {}: {}", url, e))
            })?;

        if !resp.status().is_success() {
            return Err(ProvisioningError::CloudApiError(format!(
                "Redfish PATCH {} returned {}",
                url,
                resp.status()
            )));
        }

        Ok(())
    }

    #[cfg(not(feature = "redfish"))]
    async fn http_patch(
        &self,
        _url: &str,
        _body: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<()> {
        Err(ProvisioningError::ConfigError(
            "Redfish feature not enabled. Rebuild with --features redfish".to_string(),
        ))
    }

    /// Apply the desired power state via `ComputerSystem.Reset` action.
    async fn apply_power_state(
        &self,
        system_url: &str,
        desired: &str,
        current: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<()> {
        let reset_type = match (desired, current) {
            ("On", "Off") | ("On", "Unknown") => "On",
            ("Off", _) => "ForceOff",
            ("On", "On") => return Ok(()), // already on
            ("GracefulRestart", _) => "GracefulRestart",
            ("ForceRestart", _) => "ForceRestart",
            _ => "ForceOff",
        };

        let action_url = format!("{}/Actions/ComputerSystem.Reset", system_url);
        let body = serde_json::json!({ "ResetType": reset_type });
        info!("Redfish reset action: {} -> {}", action_url, reset_type);
        self.http_post(&action_url, &body, ctx).await
    }

    /// Set the one-time boot device via PATCH on the system resource.
    async fn set_boot_device(
        &self,
        system_url: &str,
        device: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<()> {
        let body = serde_json::json!({
            "Boot": {
                "BootSourceOverrideTarget": device,
                "BootSourceOverrideEnabled": "Once"
            }
        });
        info!("Redfish set boot device: {} -> {}", system_url, device);
        self.http_patch(system_url, &body, ctx).await
    }
}

#[async_trait]
impl Resource for RedfishMachineResource {
    fn resource_type(&self) -> &str {
        "redfish_machine"
    }

    fn provider(&self) -> &str {
        PROVIDER_NAME
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "redfish_machine".to_string(),
            description: "Manage a bare-metal server via Redfish BMC API".to_string(),
            required_args: vec![SchemaField {
                name: "system_id".to_string(),
                field_type: FieldType::String,
                description: "Redfish system identifier (e.g. 'System.Embedded.1')".to_string(),
                default: None,
                constraints: vec![FieldConstraint::MinLength { min: 1 }],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "desired_power_state".to_string(),
                    field_type: FieldType::String,
                    description: "Target power state".to_string(),
                    default: Some(Value::String("On".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "On".to_string(),
                            "Off".to_string(),
                            "GracefulRestart".to_string(),
                            "ForceRestart".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "boot_device".to_string(),
                    field_type: FieldType::String,
                    description: "One-time boot source override target".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "None".to_string(),
                            "Pxe".to_string(),
                            "Cd".to_string(),
                            "Hdd".to_string(),
                            "BiosSetup".to_string(),
                            "UefiTarget".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "power_state".to_string(),
                    field_type: FieldType::String,
                    description: "Current power state reported by BMC".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "model".to_string(),
                    field_type: FieldType::String,
                    description: "Server model string".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "manufacturer".to_string(),
                    field_type: FieldType::String,
                    description: "Server manufacturer".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "serial_number".to_string(),
                    field_type: FieldType::String,
                    description: "Server serial number".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["system_id".to_string()],
            timeouts: ResourceTimeouts {
                create: 300,
                read: 60,
                update: 300,
                delete: 300,
            },
        }
    }

    async fn read(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        let url = self.system_url(ctx, id);
        debug!("Redfish read: GET {}", url);

        let data = match self.http_get(&url, ctx).await {
            Ok(d) => d,
            Err(_) => return Ok(ResourceReadResult::not_found()),
        };

        let power_state = data
            .get("PowerState")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string();

        let boot_device = data
            .pointer("/Boot/BootSourceOverrideTarget")
            .and_then(|v| v.as_str())
            .unwrap_or("None")
            .to_string();

        let attrs = serde_json::json!({
            "system_id": id,
            "power_state": power_state,
            "boot_device": boot_device,
            "model": data.get("Model").and_then(|v| v.as_str()).unwrap_or(""),
            "manufacturer": data.get("Manufacturer").and_then(|v| v.as_str()).unwrap_or(""),
            "serial_number": data.get("SerialNumber").and_then(|v| v.as_str()).unwrap_or(""),
            "bios_version": data.get("BiosVersion").and_then(|v| v.as_str()).unwrap_or(""),
            "total_memory_gib": data.get("MemorySummary")
                .and_then(|m| m.get("TotalSystemMemoryGiB"))
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            "processor_count": data.get("ProcessorSummary")
                .and_then(|p| p.get("Count"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
        });

        Ok(ResourceReadResult::found(id, attrs))
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        let current = match current {
            Some(c) => c,
            None => return Ok(ResourceDiff::create(desired.clone())),
        };

        let desired_power = desired
            .get("desired_power_state")
            .and_then(|v| v.as_str())
            .unwrap_or("On");
        let current_power = current
            .get("power_state")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");

        let desired_boot = desired.get("boot_device").and_then(|v| v.as_str());
        let current_boot = current.get("boot_device").and_then(|v| v.as_str());

        let mut modifications = HashMap::new();

        if desired_power != current_power {
            modifications.insert(
                "power_state".to_string(),
                (
                    Value::String(current_power.to_string()),
                    Value::String(desired_power.to_string()),
                ),
            );
        }

        if let Some(db) = desired_boot {
            let cb = current_boot.unwrap_or("None");
            if db != cb {
                modifications.insert(
                    "boot_device".to_string(),
                    (Value::String(cb.to_string()), Value::String(db.to_string())),
                );
            }
        }

        if modifications.is_empty() {
            return Ok(ResourceDiff::no_change());
        }

        Ok(ResourceDiff {
            change_type: ChangeType::Update,
            additions: HashMap::new(),
            modifications,
            deletions: Vec::new(),
            requires_replacement: false,
            replacement_fields: Vec::new(),
        })
    }

    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let system_id = config
            .get("system_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("system_id is required".to_string())
            })?;

        let system_url = self.system_url(ctx, system_id);

        // Apply desired power state
        let desired_power = config
            .get("desired_power_state")
            .and_then(|v| v.as_str())
            .unwrap_or("On");

        self.apply_power_state(&system_url, desired_power, "Unknown", ctx)
            .await?;

        // Apply boot device if specified
        if let Some(boot_dev) = config.get("boot_device").and_then(|v| v.as_str()) {
            self.set_boot_device(&system_url, boot_dev, ctx).await?;
        }

        let attrs = serde_json::json!({
            "system_id": system_id,
            "power_state": desired_power,
            "boot_device": config.get("boot_device").and_then(|v| v.as_str()).unwrap_or("None"),
        });

        Ok(ResourceResult::success(system_id, attrs))
    }

    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let system_url = self.system_url(ctx, id);

        let current_power = old
            .get("power_state")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown");
        let desired_power = new
            .get("desired_power_state")
            .and_then(|v| v.as_str())
            .unwrap_or("On");

        if desired_power != current_power {
            self.apply_power_state(&system_url, desired_power, current_power, ctx)
                .await?;
        }

        if let Some(boot_dev) = new.get("boot_device").and_then(|v| v.as_str()) {
            let old_boot = old
                .get("boot_device")
                .and_then(|v| v.as_str())
                .unwrap_or("None");
            if boot_dev != old_boot {
                self.set_boot_device(&system_url, boot_dev, ctx).await?;
            }
        }

        let attrs = serde_json::json!({
            "system_id": id,
            "power_state": desired_power,
            "boot_device": new.get("boot_device").and_then(|v| v.as_str()).unwrap_or("None"),
        });

        Ok(ResourceResult::success(id, attrs))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let system_url = self.system_url(ctx, id);
        info!("Powering off Redfish system: {}", id);

        self.apply_power_state(&system_url, "Off", "On", ctx)
            .await?;

        Ok(ResourceResult::success(
            id,
            serde_json::json!({ "system_id": id, "power_state": "Off" }),
        ))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let read_result = self.read(id, ctx).await?;
        if !read_result.exists {
            return Err(ProvisioningError::ImportError {
                resource_type: "redfish_machine".to_string(),
                resource_id: id.to_string(),
                message: "System not found at BMC endpoint".to_string(),
            });
        }

        Ok(ResourceResult::success(id, read_result.attributes))
    }

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        Vec::new()
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["system_id".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        if config.get("system_id").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::ValidationError(
                "system_id is required for redfish_machine".to_string(),
            ));
        }

        if let Some(ps) = config.get("desired_power_state").and_then(|v| v.as_str()) {
            match ps {
                "On" | "Off" | "GracefulRestart" | "ForceRestart" => {}
                other => {
                    return Err(ProvisioningError::ValidationError(format!(
                        "Invalid desired_power_state '{}'. Valid: On, Off, GracefulRestart, ForceRestart",
                        other
                    )))
                }
            }
        }

        if let Some(bd) = config.get("boot_device").and_then(|v| v.as_str()) {
            match bd {
                "None" | "Pxe" | "Cd" | "Hdd" | "BiosSetup" | "UefiTarget" => {}
                other => {
                    return Err(ProvisioningError::ValidationError(format!(
                    "Invalid boot_device '{}'. Valid: None, Pxe, Cd, Hdd, BiosSetup, UefiTarget",
                    other
                )))
                }
            }
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
        let provider = RedfishProvider::new();
        assert_eq!(provider.name(), "redfish");
        assert_eq!(provider.version(), "0.1.0");
    }

    #[test]
    fn test_config_schema() {
        let provider = RedfishProvider::new();
        let schema = provider.config_schema();
        assert_eq!(schema.name, "redfish");
        assert_eq!(schema.required_fields.len(), 3);
        assert!(schema.regions.is_none());
    }

    #[test]
    fn test_resource_types() {
        let provider = RedfishProvider::new();
        let types = provider.resource_types();
        assert_eq!(types, vec!["redfish_machine".to_string()]);
    }

    #[test]
    fn test_validate_config_valid() {
        let provider = RedfishProvider::new();
        let config = serde_json::json!({
            "endpoint": "https://bmc.example.com",
            "username": "admin",
            "password": "changeme"
        });
        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_missing_endpoint() {
        let provider = RedfishProvider::new();
        let config = serde_json::json!({
            "username": "admin",
            "password": "changeme"
        });
        assert!(provider.validate_config(&config).is_err());
    }

    #[test]
    fn test_validate_config_missing_username() {
        let provider = RedfishProvider::new();
        let config = serde_json::json!({
            "endpoint": "https://bmc.example.com",
            "password": "changeme"
        });
        assert!(provider.validate_config(&config).is_err());
    }

    #[test]
    fn test_context_not_configured() {
        let provider = RedfishProvider::new();
        assert!(provider.context().is_err());
    }

    #[tokio::test]
    async fn test_configure() {
        let mut provider = RedfishProvider::new();
        let config = ProviderConfig {
            name: "redfish".to_string(),
            region: None,
            settings: serde_json::json!({
                "endpoint": "https://bmc.example.com",
                "username": "admin",
                "password": "changeme",
                "timeout": 60
            }),
        };

        let result = provider.configure(config).await;
        assert!(result.is_ok());
        assert_eq!(
            provider.endpoint,
            Some("https://bmc.example.com".to_string())
        );
        assert_eq!(provider.timeout_seconds, 60);

        // Should now be able to get context
        let ctx = provider.context();
        assert!(ctx.is_ok());

        // Should have the resource registered
        let res = provider.resource("redfish_machine");
        assert!(res.is_ok());
    }

    #[test]
    fn test_machine_resource_schema() {
        let res = RedfishMachineResource {
            endpoint: "https://bmc.example.com".to_string(),
        };
        let schema = res.schema();
        assert_eq!(schema.resource_type, "redfish_machine");
        assert_eq!(schema.required_args.len(), 1);
        assert_eq!(schema.required_args[0].name, "system_id");
        assert_eq!(schema.optional_args.len(), 2);
    }

    #[test]
    fn test_machine_validate_valid() {
        let res = RedfishMachineResource {
            endpoint: "https://bmc.example.com".to_string(),
        };
        let config = serde_json::json!({
            "system_id": "System.Embedded.1",
            "desired_power_state": "On"
        });
        assert!(res.validate(&config).is_ok());
    }

    #[test]
    fn test_machine_validate_invalid_power_state() {
        let res = RedfishMachineResource {
            endpoint: "https://bmc.example.com".to_string(),
        };
        let config = serde_json::json!({
            "system_id": "System.Embedded.1",
            "desired_power_state": "Invalid"
        });
        assert!(res.validate(&config).is_err());
    }

    #[test]
    fn test_machine_validate_missing_system_id() {
        let res = RedfishMachineResource {
            endpoint: "https://bmc.example.com".to_string(),
        };
        let config = serde_json::json!({});
        assert!(res.validate(&config).is_err());
    }

    #[test]
    fn test_provider_debug() {
        let provider = RedfishProvider::new();
        let debug_str = format!("{:?}", provider);
        assert!(debug_str.contains("RedfishProvider"));
    }

    #[test]
    fn test_credentials() {
        let creds = RedfishCredentials {
            endpoint: "https://bmc.example.com".to_string(),
            username: "admin".to_string(),
            password: "secret".to_string(),
        };
        assert_eq!(creds.credential_type(), "redfish_basic");
        assert!(!creds.is_expired());

        let val = creds.as_value();
        assert_eq!(
            val.get("type").and_then(|v| v.as_str()),
            Some("redfish_basic")
        );
        // Password must not appear in serialized credentials
        assert!(val.get("password").is_none());
    }
}
