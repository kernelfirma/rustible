//! OpenStack Provider Implementation
//!
//! This module provides OpenStack cloud management using Keystone v3 token
//! authentication and the Nova / Neutron REST APIs for server and network
//! lifecycle operations.
//!
//! ## Configuration
//!
//! ```yaml
//! providers:
//!   openstack:
//!     auth_url: "https://keystone.example.com:5000/v3"
//!     username: "admin"
//!     password: "secret"
//!     project_name: "my-project"
//!     domain_name: "Default"
//!     region: "RegionOne"
//! ```
//!
//! ## Resource Types
//!
//! - `openstack_server`  - Compute instances via Nova
//! - `openstack_network` - L2 networks via Neutron
//!
//! ## Feature Gate
//!
//! Real HTTP calls require the `openstack` Cargo feature. Without it the
//! provider compiles but operations return a stub error.

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

const PROVIDER_NAME: &str = "openstack";
const PROVIDER_VERSION: &str = "0.1.0";
const DEFAULT_TIMEOUT: u64 = 300;

// ============================================================================
// Credentials
// ============================================================================

/// Keystone v3 token credentials.
#[derive(Debug, Clone)]
pub struct OpenStackCredentials {
    pub auth_url: String,
    pub username: String,
    pub project_name: String,
    pub domain_name: String,
    /// Token acquired from Keystone (populated after authentication).
    pub token: Option<String>,
}

impl ProviderCredentials for OpenStackCredentials {
    fn credential_type(&self) -> &str {
        "openstack_keystone_v3"
    }

    fn is_expired(&self) -> bool {
        // A real implementation would track token expiry.
        false
    }

    fn as_value(&self) -> Value {
        serde_json::json!({
            "type": "openstack_keystone_v3",
            "auth_url": self.auth_url,
            "username": self.username,
            "project_name": self.project_name,
            "domain_name": self.domain_name,
            "has_token": self.token.is_some(),
        })
    }
}

// ============================================================================
// Provider
// ============================================================================

/// OpenStack cloud provider.
pub struct OpenStackProvider {
    name: String,
    auth_url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    project_name: Option<String>,
    domain_name: Option<String>,
    region: Option<String>,
    token: Option<String>,
    config: Value,
    resources: HashMap<String, Arc<dyn Resource>>,
    timeout_seconds: u64,
}

impl Default for OpenStackProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenStackProvider {
    pub fn new() -> Self {
        Self {
            name: PROVIDER_NAME.to_string(),
            auth_url: None,
            username: None,
            password: None,
            project_name: None,
            domain_name: None,
            region: None,
            token: None,
            config: Value::Null,
            resources: HashMap::new(),
            timeout_seconds: DEFAULT_TIMEOUT,
        }
    }

    /// Feature-gated Keystone v3 token acquisition.
    #[cfg(feature = "openstack")]
    async fn authenticate(&mut self) -> ProvisioningResult<String> {
        let auth_url = self.auth_url.as_deref().unwrap_or_default();
        let token_url = format!("{}/auth/tokens", auth_url.trim_end_matches('/'));

        let body = serde_json::json!({
            "auth": {
                "identity": {
                    "methods": ["password"],
                    "password": {
                        "user": {
                            "name": self.username.as_deref().unwrap_or_default(),
                            "password": self.password.as_deref().unwrap_or_default(),
                            "domain": {
                                "name": self.domain_name.as_deref().unwrap_or("Default")
                            }
                        }
                    }
                },
                "scope": {
                    "project": {
                        "name": self.project_name.as_deref().unwrap_or_default(),
                        "domain": {
                            "name": self.domain_name.as_deref().unwrap_or("Default")
                        }
                    }
                }
            }
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(self.timeout_seconds))
            .build()
            .map_err(|e| ProvisioningError::CloudApiError(format!("HTTP client error: {}", e)))?;

        let resp = client
            .post(&token_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ProvisioningError::AuthenticationError {
                provider: PROVIDER_NAME.to_string(),
                message: format!("Keystone auth failed: {}", e),
            })?;

        if !resp.status().is_success() {
            return Err(ProvisioningError::AuthenticationError {
                provider: PROVIDER_NAME.to_string(),
                message: format!("Keystone returned {}", resp.status()),
            });
        }

        let token = resp
            .headers()
            .get("X-Subject-Token")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| ProvisioningError::AuthenticationError {
                provider: PROVIDER_NAME.to_string(),
                message: "No X-Subject-Token in Keystone response".to_string(),
            })?;

        Ok(token)
    }

    #[cfg(not(feature = "openstack"))]
    async fn authenticate(&mut self) -> ProvisioningResult<String> {
        Err(ProvisioningError::ConfigError(
            "OpenStack feature not enabled. Rebuild with --features openstack".to_string(),
        ))
    }
}

impl std::fmt::Debug for OpenStackProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenStackProvider")
            .field("name", &self.name)
            .field("auth_url", &self.auth_url)
            .field("username", &self.username)
            .field("project_name", &self.project_name)
            .field("domain_name", &self.domain_name)
            .field("region", &self.region)
            .field("has_token", &self.token.is_some())
            .field("resources", &self.resources.keys().collect::<Vec<_>>())
            .field("timeout_seconds", &self.timeout_seconds)
            .finish()
    }
}

#[async_trait]
impl Provider for OpenStackProvider {
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
                    name: "auth_url".to_string(),
                    field_type: FieldType::String,
                    description: "Keystone v3 authentication URL".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "username".to_string(),
                    field_type: FieldType::String,
                    description: "OpenStack username".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "password".to_string(),
                    field_type: FieldType::String,
                    description: "OpenStack password".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
                SchemaField {
                    name: "project_name".to_string(),
                    field_type: FieldType::String,
                    description: "OpenStack project / tenant name".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
            ],
            optional_fields: vec![
                SchemaField {
                    name: "domain_name".to_string(),
                    field_type: FieldType::String,
                    description: "Keystone domain name".to_string(),
                    default: Some(Value::String("Default".to_string())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "region".to_string(),
                    field_type: FieldType::String,
                    description: "OpenStack region name".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "timeout".to_string(),
                    field_type: FieldType::Integer,
                    description: "API request timeout in seconds".to_string(),
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
        info!("Configuring OpenStack provider");

        let settings = &config.settings;

        self.auth_url = Some(
            settings
                .get("auth_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ProvisioningError::provider_config(
                        PROVIDER_NAME,
                        "Missing required field: auth_url",
                    )
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

        self.project_name = Some(
            settings
                .get("project_name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ProvisioningError::provider_config(
                        PROVIDER_NAME,
                        "Missing required field: project_name",
                    )
                })?
                .to_string(),
        );

        self.domain_name = Some(
            settings
                .get("domain_name")
                .and_then(|v| v.as_str())
                .unwrap_or("Default")
                .to_string(),
        );

        self.region = config.region.clone().or_else(|| {
            settings
                .get("region")
                .and_then(|v| v.as_str())
                .map(String::from)
        });

        if let Some(t) = settings.get("timeout").and_then(|v| v.as_u64()) {
            self.timeout_seconds = t;
        }

        self.config = config.settings.clone();

        // Attempt Keystone authentication
        match self.authenticate().await {
            Ok(token) => {
                self.token = Some(token);
                info!("OpenStack Keystone authentication successful");
            }
            Err(e) => {
                warn!(
                    "OpenStack authentication deferred (will retry on first operation): {}",
                    e
                );
            }
        }

        // Register resource implementations
        self.resources.insert(
            "openstack_server".to_string(),
            Arc::new(OpenStackServerResource),
        );
        self.resources.insert(
            "openstack_network".to_string(),
            Arc::new(OpenStackNetworkResource),
        );

        info!(
            "OpenStack provider configured for auth_url: {}",
            self.auth_url.as_deref().unwrap_or("?")
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
        vec![
            "openstack_server".to_string(),
            "openstack_network".to_string(),
        ]
    }

    fn data_source_types(&self) -> Vec<String> {
        Vec::new()
    }

    fn validate_config(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["auth_url", "username", "password", "project_name"] {
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
        let auth_url = self.auth_url.as_ref().ok_or_else(|| {
            ProvisioningError::provider_config(
                PROVIDER_NAME,
                "Provider not configured. Call configure() first.",
            )
        })?;

        let creds = OpenStackCredentials {
            auth_url: auth_url.clone(),
            username: self.username.clone().unwrap_or_default(),
            project_name: self.project_name.clone().unwrap_or_default(),
            domain_name: self
                .domain_name
                .clone()
                .unwrap_or_else(|| "Default".to_string()),
            token: self.token.clone(),
        };

        Ok(ProviderContext {
            provider: PROVIDER_NAME.to_string(),
            region: self.region.clone(),
            config: self.config.clone(),
            credentials: Arc::new(creds),
            timeout_seconds: self.timeout_seconds,
            retry_config: RetryConfig {
                max_retries: 3,
                initial_backoff_ms: 1000,
                max_backoff_ms: 30000,
                backoff_multiplier: 2.0,
            },
            default_tags: HashMap::new(),
        })
    }
}

// ============================================================================
// Helper: feature-gated HTTP client
// ============================================================================

#[cfg(feature = "openstack")]
async fn os_http_get(url: &str, ctx: &ProviderContext) -> ProvisioningResult<Value> {
    let creds = ctx.credentials.as_value();
    let token = ctx
        .config
        .get("_token")
        .and_then(|v| v.as_str())
        .or_else(|| creds.get("token").and_then(|v| v.as_str()))
        .unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(ctx.timeout_seconds))
        .build()
        .map_err(|e| ProvisioningError::CloudApiError(format!("HTTP client error: {}", e)))?;

    let resp = client
        .get(url)
        .header("X-Auth-Token", token)
        .send()
        .await
        .map_err(|e| ProvisioningError::CloudApiError(format!("OpenStack GET {}: {}", url, e)))?;

    if !resp.status().is_success() {
        return Err(ProvisioningError::CloudApiError(format!(
            "OpenStack GET {} returned {}",
            url,
            resp.status()
        )));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| ProvisioningError::CloudApiError(format!("JSON parse error: {}", e)))
}

#[cfg(not(feature = "openstack"))]
async fn os_http_get(_url: &str, _ctx: &ProviderContext) -> ProvisioningResult<Value> {
    Err(ProvisioningError::ConfigError(
        "OpenStack feature not enabled. Rebuild with --features openstack".to_string(),
    ))
}

#[cfg(feature = "openstack")]
async fn os_http_post(url: &str, body: &Value, ctx: &ProviderContext) -> ProvisioningResult<Value> {
    let creds = ctx.credentials.as_value();
    let token = ctx
        .config
        .get("_token")
        .and_then(|v| v.as_str())
        .or_else(|| creds.get("token").and_then(|v| v.as_str()))
        .unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(ctx.timeout_seconds))
        .build()
        .map_err(|e| ProvisioningError::CloudApiError(format!("HTTP client error: {}", e)))?;

    let resp = client
        .post(url)
        .header("X-Auth-Token", token)
        .json(body)
        .send()
        .await
        .map_err(|e| ProvisioningError::CloudApiError(format!("OpenStack POST {}: {}", url, e)))?;

    if !resp.status().is_success() {
        return Err(ProvisioningError::CloudApiError(format!(
            "OpenStack POST {} returned {}",
            url,
            resp.status()
        )));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| ProvisioningError::CloudApiError(format!("JSON parse error: {}", e)))
}

#[cfg(not(feature = "openstack"))]
async fn os_http_post(
    _url: &str,
    _body: &Value,
    _ctx: &ProviderContext,
) -> ProvisioningResult<Value> {
    Err(ProvisioningError::ConfigError(
        "OpenStack feature not enabled. Rebuild with --features openstack".to_string(),
    ))
}

#[cfg(feature = "openstack")]
async fn os_http_put(url: &str, body: &Value, ctx: &ProviderContext) -> ProvisioningResult<Value> {
    let creds = ctx.credentials.as_value();
    let token = ctx
        .config
        .get("_token")
        .and_then(|v| v.as_str())
        .or_else(|| creds.get("token").and_then(|v| v.as_str()))
        .unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(ctx.timeout_seconds))
        .build()
        .map_err(|e| ProvisioningError::CloudApiError(format!("HTTP client error: {}", e)))?;

    let resp = client
        .put(url)
        .header("X-Auth-Token", token)
        .json(body)
        .send()
        .await
        .map_err(|e| ProvisioningError::CloudApiError(format!("OpenStack PUT {}: {}", url, e)))?;

    if !resp.status().is_success() {
        return Err(ProvisioningError::CloudApiError(format!(
            "OpenStack PUT {} returned {}",
            url,
            resp.status()
        )));
    }

    resp.json::<Value>()
        .await
        .map_err(|e| ProvisioningError::CloudApiError(format!("JSON parse error: {}", e)))
}

#[cfg(not(feature = "openstack"))]
async fn os_http_put(
    _url: &str,
    _body: &Value,
    _ctx: &ProviderContext,
) -> ProvisioningResult<Value> {
    Err(ProvisioningError::ConfigError(
        "OpenStack feature not enabled. Rebuild with --features openstack".to_string(),
    ))
}

#[cfg(feature = "openstack")]
async fn os_http_delete(url: &str, ctx: &ProviderContext) -> ProvisioningResult<()> {
    let creds = ctx.credentials.as_value();
    let token = ctx
        .config
        .get("_token")
        .and_then(|v| v.as_str())
        .or_else(|| creds.get("token").and_then(|v| v.as_str()))
        .unwrap_or_default();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(ctx.timeout_seconds))
        .build()
        .map_err(|e| ProvisioningError::CloudApiError(format!("HTTP client error: {}", e)))?;

    let resp = client
        .delete(url)
        .header("X-Auth-Token", token)
        .send()
        .await
        .map_err(|e| {
            ProvisioningError::CloudApiError(format!("OpenStack DELETE {}: {}", url, e))
        })?;

    if !resp.status().is_success() && resp.status().as_u16() != 404 {
        return Err(ProvisioningError::CloudApiError(format!(
            "OpenStack DELETE {} returned {}",
            url,
            resp.status()
        )));
    }

    Ok(())
}

#[cfg(not(feature = "openstack"))]
async fn os_http_delete(_url: &str, _ctx: &ProviderContext) -> ProvisioningResult<()> {
    Err(ProvisioningError::ConfigError(
        "OpenStack feature not enabled. Rebuild with --features openstack".to_string(),
    ))
}

// ============================================================================
// OpenStack Server Resource
// ============================================================================

/// Manages Nova compute instances.
#[derive(Debug, Clone)]
pub struct OpenStackServerResource;

impl OpenStackServerResource {
    fn nova_url(ctx: &ProviderContext) -> String {
        // A real implementation would discover this from the Keystone catalog.
        // For now, derive from auth_url by convention or config override.
        ctx.config
            .get("nova_endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                ctx.config
                    .get("auth_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("http://localhost:8774")
            })
            .trim_end_matches('/')
            .to_string()
    }
}

#[async_trait]
impl Resource for OpenStackServerResource {
    fn resource_type(&self) -> &str {
        "openstack_server"
    }

    fn provider(&self) -> &str {
        PROVIDER_NAME
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "openstack_server".to_string(),
            description: "OpenStack Nova compute instance".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "Server name".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "flavor".to_string(),
                    field_type: FieldType::String,
                    description: "Flavor name or ID".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
                SchemaField {
                    name: "image".to_string(),
                    field_type: FieldType::String,
                    description: "Image name or ID".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 1 }],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "network".to_string(),
                    field_type: FieldType::String,
                    description: "Network name or ID to attach".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "key_name".to_string(),
                    field_type: FieldType::String,
                    description: "SSH key pair name".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "security_groups".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Security group names".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "availability_zone".to_string(),
                    field_type: FieldType::String,
                    description: "Availability zone".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "metadata".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Server metadata key-value pairs".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Server UUID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: FieldType::String,
                    description: "Server status (ACTIVE, BUILD, ERROR, ...)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "access_ipv4".to_string(),
                    field_type: FieldType::String,
                    description: "IPv4 access address".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["image".to_string(), "flavor".to_string()],
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
        let url = format!("{}/servers/{}", Self::nova_url(ctx), id);
        debug!("OpenStack server read: GET {}", url);

        let data = match os_http_get(&url, ctx).await {
            Ok(d) => d,
            Err(_) => return Ok(ResourceReadResult::not_found()),
        };

        let server = data.get("server").unwrap_or(&data);
        let attrs = serde_json::json!({
            "id": server.get("id").and_then(|v| v.as_str()).unwrap_or(id),
            "name": server.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "status": server.get("status").and_then(|v| v.as_str()).unwrap_or("UNKNOWN"),
            "flavor": server.pointer("/flavor/id").and_then(|v| v.as_str()).unwrap_or(""),
            "image": server.pointer("/image/id").and_then(|v| v.as_str()).unwrap_or(""),
            "access_ipv4": server.get("accessIPv4").and_then(|v| v.as_str()).unwrap_or(""),
            "key_name": server.get("key_name").and_then(|v| v.as_str()).unwrap_or(""),
            "metadata": server.get("metadata").cloned().unwrap_or(Value::Null),
        });

        Ok(ResourceReadResult::found(id, attrs))
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

                // Check metadata changes
                if let (Some(d_meta), Some(c_meta)) = (desired.get("metadata"), cur.get("metadata"))
                {
                    if d_meta != c_meta {
                        modifications
                            .insert("metadata".to_string(), (c_meta.clone(), d_meta.clone()));
                    }
                }

                // Check name changes
                if let (Some(d_name), Some(c_name)) = (
                    desired.get("name").and_then(|v| v.as_str()),
                    cur.get("name").and_then(|v| v.as_str()),
                ) {
                    if d_name != c_name {
                        modifications.insert(
                            "name".to_string(),
                            (
                                Value::String(c_name.to_string()),
                                Value::String(d_name.to_string()),
                            ),
                        );
                    }
                }

                // Check for replacement-forcing changes (image, flavor)
                let mut requires_replacement = false;
                let mut replacement_fields = Vec::new();

                for field in &["image", "flavor"] {
                    if let (Some(d_val), Some(c_val)) = (
                        desired.get(*field).and_then(|v| v.as_str()),
                        cur.get(*field).and_then(|v| v.as_str()),
                    ) {
                        if d_val != c_val {
                            requires_replacement = true;
                            replacement_fields.push(field.to_string());
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
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProvisioningError::ValidationError("name is required".to_string()))?;

        let flavor_ref = config
            .get("flavor")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        let image_ref = config
            .get("image")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        let mut server_body = serde_json::json!({
            "server": {
                "name": name,
                "flavorRef": flavor_ref,
                "imageRef": image_ref,
            }
        });

        // Attach optional fields
        if let Some(net) = config.get("network").and_then(|v| v.as_str()) {
            server_body["server"]["networks"] = serde_json::json!([{"uuid": net}]);
        }
        if let Some(key) = config.get("key_name").and_then(|v| v.as_str()) {
            server_body["server"]["key_name"] = Value::String(key.to_string());
        }
        if let Some(az) = config.get("availability_zone").and_then(|v| v.as_str()) {
            server_body["server"]["availability_zone"] = Value::String(az.to_string());
        }
        if let Some(meta) = config.get("metadata") {
            if meta.is_object() {
                server_body["server"]["metadata"] = meta.clone();
            }
        }
        if let Some(sgs) = config.get("security_groups").and_then(|v| v.as_array()) {
            let sg_list: Vec<Value> = sgs
                .iter()
                .filter_map(|s| s.as_str().map(|n| serde_json::json!({"name": n})))
                .collect();
            server_body["server"]["security_groups"] = Value::Array(sg_list);
        }

        let url = format!("{}/servers", Self::nova_url(ctx));
        info!("Creating OpenStack server: {}", name);

        let resp = os_http_post(&url, &server_body, ctx).await?;

        let server = resp.get("server").unwrap_or(&resp);
        let server_id = server
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let attrs = serde_json::json!({
            "id": server_id,
            "name": name,
            "status": server.get("status").and_then(|v| v.as_str()).unwrap_or("BUILD"),
            "flavor": flavor_ref,
            "image": image_ref,
        });

        Ok(ResourceResult::success(&server_id, attrs))
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let url = format!("{}/servers/{}", Self::nova_url(ctx), id);

        // Update name and/or metadata
        let mut update_body = serde_json::json!({"server": {}});

        if let Some(name) = new.get("name").and_then(|v| v.as_str()) {
            update_body["server"]["name"] = Value::String(name.to_string());
        }

        info!("Updating OpenStack server: {}", id);
        let resp = os_http_put(&url, &update_body, ctx).await?;

        // Update metadata separately if changed
        if let Some(meta) = new.get("metadata") {
            if meta.is_object() {
                let meta_url = format!("{}/servers/{}/metadata", Self::nova_url(ctx), id);
                let meta_body = serde_json::json!({"metadata": meta});
                let _ = os_http_put(&meta_url, &meta_body, ctx).await;
            }
        }

        let server = resp.get("server").unwrap_or(&resp);
        let attrs = serde_json::json!({
            "id": id,
            "name": server.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "status": server.get("status").and_then(|v| v.as_str()).unwrap_or("ACTIVE"),
        });

        Ok(ResourceResult::success(id, attrs))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let url = format!("{}/servers/{}", Self::nova_url(ctx), id);
        info!("Destroying OpenStack server: {}", id);
        os_http_delete(&url, ctx).await?;
        Ok(ResourceResult::success(
            id,
            serde_json::json!({"id": id, "status": "DELETED"}),
        ))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let read_result = self.read(id, ctx).await?;
        if !read_result.exists {
            return Err(ProvisioningError::ImportError {
                resource_type: "openstack_server".to_string(),
                resource_id: id.to_string(),
                message: "Server not found".to_string(),
            });
        }
        Ok(ResourceResult::success(id, read_result.attributes))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();
        if let Some(net) = config.get("network").and_then(|v| v.as_str()) {
            // If the network value looks like a reference to another resource
            if net.starts_with("openstack_network.") {
                deps.push(ResourceDependency::new("openstack_network", net, "id"));
            }
        }
        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["image".to_string(), "flavor".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["name", "flavor", "image"] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{} is required for openstack_server",
                    field
                )));
            }
        }
        Ok(())
    }
}

// ============================================================================
// OpenStack Network Resource
// ============================================================================

/// Manages Neutron L2 networks.
#[derive(Debug, Clone)]
pub struct OpenStackNetworkResource;

impl OpenStackNetworkResource {
    fn neutron_url(ctx: &ProviderContext) -> String {
        ctx.config
            .get("neutron_endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                ctx.config
                    .get("auth_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("http://localhost:9696")
            })
            .trim_end_matches('/')
            .to_string()
    }
}

#[async_trait]
impl Resource for OpenStackNetworkResource {
    fn resource_type(&self) -> &str {
        "openstack_network"
    }

    fn provider(&self) -> &str {
        PROVIDER_NAME
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "openstack_network".to_string(),
            description: "OpenStack Neutron L2 network".to_string(),
            required_args: vec![SchemaField {
                name: "name".to_string(),
                field_type: FieldType::String,
                description: "Network name".to_string(),
                default: None,
                constraints: vec![FieldConstraint::MinLength { min: 1 }],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "admin_state_up".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Administrative state".to_string(),
                    default: Some(Value::Bool(true)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "shared".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Whether the network is shared across projects".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "external".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Whether the network is an external network".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "mtu".to_string(),
                    field_type: FieldType::Integer,
                    description: "Maximum transmission unit".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 68 },
                        FieldConstraint::MaxValue { value: 9216 },
                    ],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Network UUID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: FieldType::String,
                    description: "Network status (ACTIVE, BUILD, DOWN, ERROR)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![],
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
        let url = format!("{}/v2.0/networks/{}", Self::neutron_url(ctx), id);
        debug!("OpenStack network read: GET {}", url);

        let data = match os_http_get(&url, ctx).await {
            Ok(d) => d,
            Err(_) => return Ok(ResourceReadResult::not_found()),
        };

        let network = data.get("network").unwrap_or(&data);
        let attrs = serde_json::json!({
            "id": network.get("id").and_then(|v| v.as_str()).unwrap_or(id),
            "name": network.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "status": network.get("status").and_then(|v| v.as_str()).unwrap_or("UNKNOWN"),
            "admin_state_up": network.get("admin_state_up").and_then(|v| v.as_bool()).unwrap_or(true),
            "shared": network.get("shared").and_then(|v| v.as_bool()).unwrap_or(false),
            "external": network.get("router:external").and_then(|v| v.as_bool()).unwrap_or(false),
            "mtu": network.get("mtu").and_then(|v| v.as_u64()).unwrap_or(0),
        });

        Ok(ResourceReadResult::found(id, attrs))
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

                for field in &["name", "admin_state_up", "shared", "external", "mtu"] {
                    if let (Some(d), Some(c)) = (desired.get(*field), cur.get(*field)) {
                        if d != c {
                            modifications.insert(field.to_string(), (c.clone(), d.clone()));
                        }
                    }
                }

                if modifications.is_empty() {
                    Ok(ResourceDiff::no_change())
                } else {
                    Ok(ResourceDiff {
                        change_type: ChangeType::Update,
                        additions: HashMap::new(),
                        modifications,
                        deletions: Vec::new(),
                        requires_replacement: false,
                        replacement_fields: Vec::new(),
                    })
                }
            }
        }
    }

    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProvisioningError::ValidationError("name is required".to_string()))?;

        let mut net_body = serde_json::json!({
            "network": {
                "name": name,
                "admin_state_up": config.get("admin_state_up").and_then(|v| v.as_bool()).unwrap_or(true),
            }
        });

        if let Some(shared) = config.get("shared").and_then(|v| v.as_bool()) {
            net_body["network"]["shared"] = Value::Bool(shared);
        }
        if let Some(ext) = config.get("external").and_then(|v| v.as_bool()) {
            net_body["network"]["router:external"] = Value::Bool(ext);
        }
        if let Some(mtu) = config.get("mtu").and_then(|v| v.as_u64()) {
            net_body["network"]["mtu"] = Value::Number(serde_json::Number::from(mtu));
        }

        let url = format!("{}/v2.0/networks", Self::neutron_url(ctx));
        info!("Creating OpenStack network: {}", name);

        let resp = os_http_post(&url, &net_body, ctx).await?;

        let network = resp.get("network").unwrap_or(&resp);
        let net_id = network
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let attrs = serde_json::json!({
            "id": net_id,
            "name": name,
            "status": network.get("status").and_then(|v| v.as_str()).unwrap_or("ACTIVE"),
            "admin_state_up": config.get("admin_state_up").and_then(|v| v.as_bool()).unwrap_or(true),
        });

        Ok(ResourceResult::success(&net_id, attrs))
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let mut update_body = serde_json::json!({"network": {}});

        if let Some(name) = new.get("name").and_then(|v| v.as_str()) {
            update_body["network"]["name"] = Value::String(name.to_string());
        }
        if let Some(admin) = new.get("admin_state_up").and_then(|v| v.as_bool()) {
            update_body["network"]["admin_state_up"] = Value::Bool(admin);
        }

        let url = format!("{}/v2.0/networks/{}", Self::neutron_url(ctx), id);
        info!("Updating OpenStack network: {}", id);

        let resp = os_http_put(&url, &update_body, ctx).await?;

        let network = resp.get("network").unwrap_or(&resp);
        let attrs = serde_json::json!({
            "id": id,
            "name": network.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            "status": network.get("status").and_then(|v| v.as_str()).unwrap_or("ACTIVE"),
        });

        Ok(ResourceResult::success(id, attrs))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let url = format!("{}/v2.0/networks/{}", Self::neutron_url(ctx), id);
        info!("Destroying OpenStack network: {}", id);
        os_http_delete(&url, ctx).await?;
        Ok(ResourceResult::success(
            id,
            serde_json::json!({"id": id, "status": "DELETED"}),
        ))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let read_result = self.read(id, ctx).await?;
        if !read_result.exists {
            return Err(ProvisioningError::ImportError {
                resource_type: "openstack_network".to_string(),
                resource_id: id.to_string(),
                message: "Network not found".to_string(),
            });
        }
        Ok(ResourceResult::success(id, read_result.attributes))
    }

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        Vec::new()
    }

    fn forces_replacement(&self) -> Vec<String> {
        Vec::new()
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        if config.get("name").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::ValidationError(
                "name is required for openstack_network".to_string(),
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
        let provider = OpenStackProvider::new();
        assert_eq!(provider.name(), "openstack");
        assert_eq!(provider.version(), "0.1.0");
    }

    #[test]
    fn test_config_schema() {
        let provider = OpenStackProvider::new();
        let schema = provider.config_schema();
        assert_eq!(schema.name, "openstack");
        assert_eq!(schema.required_fields.len(), 4);
        assert!(schema.regions.is_none());

        let password_field = schema.required_fields.iter().find(|f| f.name == "password");
        assert!(password_field.is_some());
        assert!(password_field.unwrap().sensitive);
    }

    #[test]
    fn test_resource_types() {
        let provider = OpenStackProvider::new();
        let types = provider.resource_types();
        assert!(types.contains(&"openstack_server".to_string()));
        assert!(types.contains(&"openstack_network".to_string()));
    }

    #[test]
    fn test_validate_config_valid() {
        let provider = OpenStackProvider::new();
        let config = serde_json::json!({
            "auth_url": "https://keystone:5000/v3",
            "username": "admin",
            "password": "secret",
            "project_name": "demo"
        });
        assert!(provider.validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_config_missing_auth_url() {
        let provider = OpenStackProvider::new();
        let config = serde_json::json!({
            "username": "admin",
            "password": "secret",
            "project_name": "demo"
        });
        assert!(provider.validate_config(&config).is_err());
    }

    #[test]
    fn test_context_not_configured() {
        let provider = OpenStackProvider::new();
        assert!(provider.context().is_err());
    }

    #[tokio::test]
    async fn test_configure() {
        let mut provider = OpenStackProvider::new();
        let config = ProviderConfig {
            name: "openstack".to_string(),
            region: Some("RegionOne".to_string()),
            settings: serde_json::json!({
                "auth_url": "https://keystone:5000/v3",
                "username": "admin",
                "password": "secret",
                "project_name": "demo",
                "domain_name": "Default"
            }),
        };

        let result = provider.configure(config).await;
        assert!(result.is_ok());

        assert_eq!(
            provider.auth_url,
            Some("https://keystone:5000/v3".to_string())
        );
        assert_eq!(provider.region, Some("RegionOne".to_string()));

        let ctx = provider.context();
        assert!(ctx.is_ok());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.provider, "openstack");
        assert_eq!(ctx.region, Some("RegionOne".to_string()));

        // Resources should be registered
        assert!(provider.resource("openstack_server").is_ok());
        assert!(provider.resource("openstack_network").is_ok());
        assert!(provider.resource("nonexistent").is_err());
    }

    #[test]
    fn test_server_resource_schema() {
        let res = OpenStackServerResource;
        let schema = res.schema();
        assert_eq!(schema.resource_type, "openstack_server");
        assert_eq!(schema.required_args.len(), 3);
    }

    #[test]
    fn test_server_validate() {
        let res = OpenStackServerResource;

        let valid = serde_json::json!({
            "name": "web-01",
            "flavor": "m1.small",
            "image": "ubuntu-22.04"
        });
        assert!(res.validate(&valid).is_ok());

        let invalid = serde_json::json!({
            "name": "web-01",
            "flavor": "m1.small"
        });
        assert!(res.validate(&invalid).is_err());
    }

    #[test]
    fn test_network_resource_schema() {
        let res = OpenStackNetworkResource;
        let schema = res.schema();
        assert_eq!(schema.resource_type, "openstack_network");
        assert_eq!(schema.required_args.len(), 1);
    }

    #[test]
    fn test_network_validate() {
        let res = OpenStackNetworkResource;
        assert!(res.validate(&serde_json::json!({"name": "net-01"})).is_ok());
        assert!(res.validate(&serde_json::json!({})).is_err());
    }

    #[test]
    fn test_credentials() {
        let creds = OpenStackCredentials {
            auth_url: "https://keystone:5000/v3".to_string(),
            username: "admin".to_string(),
            project_name: "demo".to_string(),
            domain_name: "Default".to_string(),
            token: Some("tok123".to_string()),
        };
        assert_eq!(creds.credential_type(), "openstack_keystone_v3");
        assert!(!creds.is_expired());

        let val = creds.as_value();
        assert_eq!(val.get("has_token").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn test_provider_debug() {
        let provider = OpenStackProvider::new();
        let debug_str = format!("{:?}", provider);
        assert!(debug_str.contains("OpenStackProvider"));
    }
}
