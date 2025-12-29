//! AWS Elastic IP Resource for infrastructure provisioning
//!
//! This module implements the `Resource` trait for AWS Elastic IPs, enabling declarative
//! Elastic IP management through the provisioning system.
//!
//! ## Example
//!
//! ```yaml
//! resources:
//!   aws_eip:
//!     nat:
//!       domain: vpc
//!       tags:
//!         Name: nat-gateway-eip
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "aws")]
use aws_config::BehaviorVersion;
#[cfg(feature = "aws")]
use aws_sdk_ec2::types::{DomainType, ResourceType, Tag, TagSpecification};
#[cfg(feature = "aws")]
use aws_sdk_ec2::Client;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// Elastic IP Resource Configuration
// ============================================================================

/// Elastic IP resource attributes (computed from cloud)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElasticIpAttributes {
    /// Allocation ID (e.g., eipalloc-12345678)
    pub id: String,
    /// Public IP address
    pub public_ip: String,
    /// Private IP address (if associated with ENI)
    pub private_ip: Option<String>,
    /// Association ID (if associated)
    pub association_id: Option<String>,
    /// Domain (vpc or standard)
    pub domain: String,
    /// Instance ID (if associated with instance)
    pub instance_id: Option<String>,
    /// Network interface ID (if associated with ENI)
    pub network_interface_id: Option<String>,
    /// Network interface owner ID
    pub network_interface_owner_id: Option<String>,
    /// Public IPv4 pool
    pub public_ipv4_pool: Option<String>,
    /// Customer owned IPv4 pool
    pub customer_owned_ipv4_pool: Option<String>,
    /// Customer owned IP
    pub customer_owned_ip: Option<String>,
    /// Carrier IP (for Wavelength zones)
    pub carrier_ip: Option<String>,
    /// Tags
    pub tags: HashMap<String, String>,
}

/// Elastic IP configuration
#[derive(Debug, Clone)]
pub struct ElasticIpConfig {
    pub domain: String,
    pub instance: Option<String>,
    pub network_interface: Option<String>,
    pub address: Option<String>,
    pub public_ipv4_pool: Option<String>,
    pub customer_owned_ipv4_pool: Option<String>,
    pub network_border_group: Option<String>,
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS Elastic IP Resource Implementation
// ============================================================================

/// AWS Elastic IP resource for infrastructure provisioning
///
/// This resource manages AWS Elastic IPs, which are static IPv4 addresses
/// designed for dynamic cloud computing.
#[derive(Debug, Clone, Default)]
pub struct AwsElasticIpResource;

impl AwsElasticIpResource {
    /// Create a new Elastic IP resource
    pub fn new() -> Self {
        Self
    }

    /// Build the resource schema
    fn build_schema() -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_eip".to_string(),
            description: "AWS Elastic IP address".to_string(),
            required_args: vec![],
            optional_args: vec![
                SchemaField {
                    name: "domain".to_string(),
                    field_type: FieldType::String,
                    description: "Domain for the EIP: vpc or standard (default: vpc)".to_string(),
                    default: Some(Value::String("vpc".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec!["vpc".to_string(), "standard".to_string()],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "instance".to_string(),
                    field_type: FieldType::String,
                    description: "Instance ID to associate the EIP with".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "network_interface".to_string(),
                    field_type: FieldType::String,
                    description: "Network interface ID to associate the EIP with".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "address".to_string(),
                    field_type: FieldType::String,
                    description: "Specific IP address to allocate".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "public_ipv4_pool".to_string(),
                    field_type: FieldType::String,
                    description: "Public IPv4 pool to allocate from".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "customer_owned_ipv4_pool".to_string(),
                    field_type: FieldType::String,
                    description: "Customer owned IPv4 pool ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "network_border_group".to_string(),
                    field_type: FieldType::String,
                    description: "Network border group for Local Zone/Wavelength".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Tags to apply to the EIP".to_string(),
                    default: Some(Value::Object(Default::default())),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Allocation ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "public_ip".to_string(),
                    field_type: FieldType::String,
                    description: "Public IP address".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_ip".to_string(),
                    field_type: FieldType::String,
                    description: "Private IP address (if associated)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "association_id".to_string(),
                    field_type: FieldType::String,
                    description: "Association ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "domain".to_string(),
                "address".to_string(),
                "public_ipv4_pool".to_string(),
                "customer_owned_ipv4_pool".to_string(),
                "network_border_group".to_string(),
            ],
            timeouts: ResourceTimeouts {
                create: 120,
                read: 60,
                update: 120,
                delete: 120,
            },
        }
    }

    /// Extract configuration values from JSON
    fn extract_config(config: &Value) -> ProvisioningResult<ElasticIpConfig> {
        let domain = config
            .get("domain")
            .and_then(|v| v.as_str())
            .unwrap_or("vpc")
            .to_string();

        let instance = config
            .get("instance")
            .and_then(|v| v.as_str())
            .map(String::from);

        let network_interface = config
            .get("network_interface")
            .and_then(|v| v.as_str())
            .map(String::from);

        let address = config
            .get("address")
            .and_then(|v| v.as_str())
            .map(String::from);

        let public_ipv4_pool = config
            .get("public_ipv4_pool")
            .and_then(|v| v.as_str())
            .map(String::from);

        let customer_owned_ipv4_pool = config
            .get("customer_owned_ipv4_pool")
            .and_then(|v| v.as_str())
            .map(String::from);

        let network_border_group = config
            .get("network_border_group")
            .and_then(|v| v.as_str())
            .map(String::from);

        let tags = if let Some(tags_value) = config.get("tags") {
            if let Some(obj) = tags_value.as_object() {
                obj.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            } else {
                HashMap::new()
            }
        } else {
            HashMap::new()
        };

        Ok(ElasticIpConfig {
            domain,
            instance,
            network_interface,
            address,
            public_ipv4_pool,
            customer_owned_ipv4_pool,
            network_border_group,
            tags,
        })
    }

    /// Create AWS EC2 client
    #[cfg(feature = "aws")]
    async fn create_client(ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let config = if let Some(ref region) = ctx.region {
            aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_ec2::config::Region::new(region.clone()))
                .load()
                .await
        } else {
            aws_config::defaults(BehaviorVersion::latest()).load().await
        };

        Ok(Client::new(&config))
    }

    /// Read Elastic IP by allocation ID from AWS
    #[cfg(feature = "aws")]
    async fn read_eip_by_id(
        client: &Client,
        allocation_id: &str,
    ) -> ProvisioningResult<Option<ElasticIpAttributes>> {
        let resp = client
            .describe_addresses()
            .allocation_ids(allocation_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to describe EIP: {}", e))
            })?;

        if let Some(addr) = resp.addresses().first() {
            let mut tags = HashMap::new();
            for tag in addr.tags() {
                if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                    tags.insert(key.to_string(), value.to_string());
                }
            }

            Ok(Some(ElasticIpAttributes {
                id: addr.allocation_id().unwrap_or_default().to_string(),
                public_ip: addr.public_ip().unwrap_or_default().to_string(),
                private_ip: addr.private_ip_address().map(String::from),
                association_id: addr.association_id().map(String::from),
                domain: addr
                    .domain()
                    .map(|d| d.as_str().to_string())
                    .unwrap_or_else(|| "vpc".to_string()),
                instance_id: addr.instance_id().map(String::from),
                network_interface_id: addr.network_interface_id().map(String::from),
                network_interface_owner_id: addr.network_interface_owner_id().map(String::from),
                public_ipv4_pool: addr.public_ipv4_pool().map(String::from),
                customer_owned_ipv4_pool: addr.customer_owned_ipv4_pool().map(String::from),
                customer_owned_ip: addr.customer_owned_ip().map(String::from),
                carrier_ip: addr.carrier_ip().map(String::from),
                tags,
            }))
        } else {
            Ok(None)
        }
    }

    /// Allocate Elastic IP in AWS
    #[cfg(feature = "aws")]
    async fn allocate_eip(
        client: &Client,
        config: &ElasticIpConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ElasticIpAttributes> {
        // Build tags including default tags from context
        let mut all_tags: Vec<Tag> = ctx
            .default_tags
            .iter()
            .map(|(k, v)| Tag::builder().key(k).value(v).build())
            .collect();

        for (k, v) in &config.tags {
            all_tags.push(Tag::builder().key(k).value(v).build());
        }

        let domain = if config.domain == "vpc" {
            DomainType::Vpc
        } else {
            DomainType::Standard
        };

        let mut req = client
            .allocate_address()
            .domain(domain)
            .tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::ElasticIp)
                    .set_tags(Some(all_tags))
                    .build(),
            );

        if let Some(ref addr) = config.address {
            req = req.address(addr);
        }
        if let Some(ref pool) = config.public_ipv4_pool {
            req = req.public_ipv4_pool(pool);
        }
        if let Some(ref pool) = config.customer_owned_ipv4_pool {
            req = req.customer_owned_ipv4_pool(pool);
        }
        if let Some(ref nbg) = config.network_border_group {
            req = req.network_border_group(nbg);
        }

        let resp = req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to allocate EIP: {}", e))
        })?;

        let allocation_id = resp.allocation_id().ok_or_else(|| {
            ProvisioningError::CloudApiError("No allocation ID returned".to_string())
        })?;

        // Associate with instance or ENI if specified
        if let Some(ref instance_id) = config.instance {
            client
                .associate_address()
                .allocation_id(allocation_id)
                .instance_id(instance_id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to associate EIP with instance: {}",
                        e
                    ))
                })?;
        } else if let Some(ref eni_id) = config.network_interface {
            client
                .associate_address()
                .allocation_id(allocation_id)
                .network_interface_id(eni_id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to associate EIP with ENI: {}",
                        e
                    ))
                })?;
        }

        // Read the full EIP attributes
        Self::read_eip_by_id(client, allocation_id)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read allocated EIP".to_string())
            })
    }

    /// Update Elastic IP in AWS (association and tags only)
    #[cfg(feature = "aws")]
    async fn update_eip(
        client: &Client,
        allocation_id: &str,
        old_config: &ElasticIpConfig,
        new_config: &ElasticIpConfig,
    ) -> ProvisioningResult<ElasticIpAttributes> {
        // Handle association changes
        let old_target = old_config
            .instance
            .as_ref()
            .or(old_config.network_interface.as_ref());
        let new_target = new_config
            .instance
            .as_ref()
            .or(new_config.network_interface.as_ref());

        if old_target != new_target {
            // Disassociate if currently associated
            if old_target.is_some() {
                if let Some(attrs) = Self::read_eip_by_id(client, allocation_id).await? {
                    if let Some(ref assoc_id) = attrs.association_id {
                        client
                            .disassociate_address()
                            .association_id(assoc_id)
                            .send()
                            .await
                            .map_err(|e| {
                                ProvisioningError::CloudApiError(format!(
                                    "Failed to disassociate EIP: {}",
                                    e
                                ))
                            })?;
                    }
                }
            }

            // Associate with new target
            if let Some(ref instance_id) = new_config.instance {
                client
                    .associate_address()
                    .allocation_id(allocation_id)
                    .instance_id(instance_id)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to associate EIP with instance: {}",
                            e
                        ))
                    })?;
            } else if let Some(ref eni_id) = new_config.network_interface {
                client
                    .associate_address()
                    .allocation_id(allocation_id)
                    .network_interface_id(eni_id)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to associate EIP with ENI: {}",
                            e
                        ))
                    })?;
            }
        }

        // Update tags if changed
        if old_config.tags != new_config.tags {
            let tags_to_delete: Vec<_> = old_config
                .tags
                .keys()
                .filter(|k| !new_config.tags.contains_key(*k))
                .map(|k| Tag::builder().key(k).build())
                .collect();

            if !tags_to_delete.is_empty() {
                client
                    .delete_tags()
                    .resources(allocation_id)
                    .set_tags(Some(tags_to_delete))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to delete tags: {}", e))
                    })?;
            }

            let tags_to_create: Vec<_> = new_config
                .tags
                .iter()
                .map(|(k, v)| Tag::builder().key(k).value(v).build())
                .collect();

            if !tags_to_create.is_empty() {
                client
                    .create_tags()
                    .resources(allocation_id)
                    .set_tags(Some(tags_to_create))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to create tags: {}", e))
                    })?;
            }
        }

        // Read updated EIP
        Self::read_eip_by_id(client, allocation_id)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read updated EIP".to_string())
            })
    }

    /// Release Elastic IP in AWS
    #[cfg(feature = "aws")]
    async fn release_eip(client: &Client, allocation_id: &str) -> ProvisioningResult<()> {
        // First disassociate if associated
        if let Some(attrs) = Self::read_eip_by_id(client, allocation_id).await? {
            if let Some(ref assoc_id) = attrs.association_id {
                client
                    .disassociate_address()
                    .association_id(assoc_id)
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!(
                            "Failed to disassociate EIP: {}",
                            e
                        ))
                    })?;
            }
        }

        // Release the EIP
        client
            .release_address()
            .allocation_id(allocation_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to release EIP: {}", e))
            })?;

        Ok(())
    }

    /// Compute diff between desired and current state
    fn compute_diff(
        desired: &Value,
        current: Option<&Value>,
        force_new_fields: &[String],
    ) -> ProvisioningResult<ResourceDiff> {
        if current.is_none() {
            return Ok(ResourceDiff::create(desired.clone()));
        }

        let current = current.unwrap();

        let mut modifications = HashMap::new();
        let mut additions = HashMap::new();
        let mut deletions = Vec::new();
        let mut replacement_fields = Vec::new();

        let computed_fields = [
            "id",
            "public_ip",
            "private_ip",
            "association_id",
            "instance_id",
            "network_interface_id",
            "network_interface_owner_id",
            "customer_owned_ip",
            "carrier_ip",
        ];

        if let (Some(desired_obj), Some(current_obj)) = (desired.as_object(), current.as_object()) {
            for (key, desired_val) in desired_obj {
                if computed_fields.contains(&key.as_str()) {
                    continue;
                }

                if let Some(current_val) = current_obj.get(key) {
                    if desired_val != current_val {
                        modifications.insert(key.clone(), (current_val.clone(), desired_val.clone()));

                        if force_new_fields.contains(key) {
                            replacement_fields.push(key.clone());
                        }
                    }
                } else {
                    additions.insert(key.clone(), desired_val.clone());
                }
            }

            for key in current_obj.keys() {
                if computed_fields.contains(&key.as_str()) {
                    continue;
                }

                if !desired_obj.contains_key(key) {
                    deletions.push(key.clone());
                }
            }
        }

        let requires_replacement = !replacement_fields.is_empty();
        let has_changes = !additions.is_empty() || !modifications.is_empty() || !deletions.is_empty();

        let change_type = if requires_replacement {
            ChangeType::Replace
        } else if has_changes {
            ChangeType::Update
        } else {
            ChangeType::NoOp
        };

        Ok(ResourceDiff {
            change_type,
            additions,
            modifications,
            deletions,
            requires_replacement,
            replacement_fields,
        })
    }
}

#[async_trait]
impl Resource for AwsElasticIpResource {
    fn resource_type(&self) -> &str {
        "aws_eip"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        Self::build_schema()
    }

    #[cfg(feature = "aws")]
    async fn read(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        let client = Self::create_client(ctx).await?;

        match Self::read_eip_by_id(&client, id).await? {
            Some(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
                Ok(ResourceReadResult::found(id, attributes))
            }
            None => Ok(ResourceReadResult::not_found()),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn read(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        let force_new = self.forces_replacement();
        Self::compute_diff(desired, current, &force_new)
    }

    #[cfg(feature = "aws")]
    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let eip_config = Self::extract_config(config)?;
        let client = Self::create_client(ctx).await?;

        match Self::allocate_eip(&client, &eip_config, ctx).await {
            Ok(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;

                let mut result = ResourceResult::success(&attrs.id, attributes);
                result.outputs.insert("id".to_string(), Value::String(attrs.id.clone()));
                result.outputs.insert("public_ip".to_string(), Value::String(attrs.public_ip));
                Ok(result)
            }
            Err(e) => Ok(ResourceResult::failure(e.to_string())),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn create(
        &self,
        _config: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    #[cfg(feature = "aws")]
    async fn update(
        &self,
        id: &str,
        old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let old_config = Self::extract_config(old)?;
        let new_config = Self::extract_config(new)?;
        let client = Self::create_client(ctx).await?;

        match Self::update_eip(&client, id, &old_config, &new_config).await {
            Ok(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
                Ok(ResourceResult::success(id, attributes))
            }
            Err(e) => Ok(ResourceResult::failure(e.to_string())),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn update(
        &self,
        _id: &str,
        _old: &Value,
        _new: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    #[cfg(feature = "aws")]
    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = Self::create_client(ctx).await?;

        match Self::release_eip(&client, id).await {
            Ok(()) => Ok(ResourceResult::success(id, Value::Null)),
            Err(e) => Ok(ResourceResult::failure(e.to_string())),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn destroy(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    #[cfg(feature = "aws")]
    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = Self::create_client(ctx).await?;

        match Self::read_eip_by_id(&client, id).await? {
            Some(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
                Ok(ResourceResult::success(id, attributes))
            }
            None => Err(ProvisioningError::ImportError {
                resource_type: "aws_eip".to_string(),
                resource_id: id.to_string(),
                message: "Elastic IP not found".to_string(),
            }),
        }
    }

    #[cfg(not(feature = "aws"))]
    async fn import(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::ProviderConfigError {
            provider: "aws".to_string(),
            message: "AWS feature not enabled. Build with --features aws".to_string(),
        })
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // Instance dependency
        if let Some(instance) = config.get("instance").and_then(|v| v.as_str()) {
            if instance.contains("resources.aws_instance.") {
                if let Some(name) = instance
                    .strip_prefix("{{ resources.aws_instance.")
                    .and_then(|s| s.strip_suffix(".id }}"))
                {
                    deps.push(ResourceDependency {
                        resource_type: "aws_instance".to_string(),
                        resource_name: name.to_string(),
                        attribute: "id".to_string(),
                        hard: true,
                    });
                }
            }
        }

        // Network interface dependency
        if let Some(eni) = config.get("network_interface").and_then(|v| v.as_str()) {
            if eni.contains("resources.aws_network_interface.") {
                if let Some(name) = eni
                    .strip_prefix("{{ resources.aws_network_interface.")
                    .and_then(|s| s.strip_suffix(".id }}"))
                {
                    deps.push(ResourceDependency {
                        resource_type: "aws_network_interface".to_string(),
                        resource_name: name.to_string(),
                        attribute: "id".to_string(),
                        hard: true,
                    });
                }
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec![
            "domain".to_string(),
            "address".to_string(),
            "public_ipv4_pool".to_string(),
            "customer_owned_ipv4_pool".to_string(),
            "network_border_group".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate domain if provided
        if let Some(domain) = config.get("domain").and_then(|v| v.as_str()) {
            if domain != "vpc" && domain != "standard" {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid domain '{}'. Valid values: vpc, standard",
                    domain
                )));
            }
        }

        // Cannot specify both instance and network_interface
        if config.get("instance").is_some() && config.get("network_interface").is_some() {
            return Err(ProvisioningError::ValidationError(
                "Cannot specify both instance and network_interface".to_string(),
            ));
        }

        // Validate tags is an object if provided
        if let Some(tags) = config.get("tags") {
            if !tags.is_object() && !tags.is_null() {
                return Err(ProvisioningError::ValidationError(
                    "tags must be an object".to_string(),
                ));
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
    fn test_resource_type() {
        let resource = AwsElasticIpResource::new();
        assert_eq!(resource.resource_type(), "aws_eip");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsElasticIpResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_eip");
        assert_eq!(schema.required_args.len(), 0);
        assert_eq!(schema.optional_args.len(), 8);
        assert!(schema.force_new.contains(&"domain".to_string()));
    }

    #[test]
    fn test_validate_config() {
        let resource = AwsElasticIpResource::new();

        // Valid minimal config
        let config = serde_json::json!({});
        assert!(resource.validate(&config).is_ok());

        // Valid with domain
        let config = serde_json::json!({
            "domain": "vpc"
        });
        assert!(resource.validate(&config).is_ok());

        // Invalid domain
        let config = serde_json::json!({
            "domain": "invalid"
        });
        assert!(resource.validate(&config).is_err());

        // Cannot have both instance and network_interface
        let config = serde_json::json!({
            "instance": "i-12345",
            "network_interface": "eni-12345"
        });
        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_extract_config() {
        let config = serde_json::json!({
            "domain": "vpc",
            "instance": "i-12345",
            "tags": {
                "Name": "test-eip"
            }
        });

        let eip_config = AwsElasticIpResource::extract_config(&config).unwrap();
        assert_eq!(eip_config.domain, "vpc");
        assert_eq!(eip_config.instance, Some("i-12345".to_string()));
        assert_eq!(eip_config.tags.get("Name"), Some(&"test-eip".to_string()));
    }

    #[test]
    fn test_compute_diff_create() {
        let desired = serde_json::json!({
            "domain": "vpc"
        });

        let diff = AwsElasticIpResource::compute_diff(&desired, None, &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::Create);
    }

    #[test]
    fn test_compute_diff_no_change() {
        let desired = serde_json::json!({
            "domain": "vpc"
        });

        let current = serde_json::json!({
            "domain": "vpc",
            "id": "eipalloc-12345",
            "public_ip": "1.2.3.4"
        });

        let diff = AwsElasticIpResource::compute_diff(&desired, Some(&current), &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsElasticIpResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"domain".to_string()));
        assert!(force_new.contains(&"address".to_string()));
        assert!(force_new.contains(&"public_ipv4_pool".to_string()));
    }

    #[test]
    fn test_dependencies() {
        let resource = AwsElasticIpResource::new();

        let config = serde_json::json!({
            "instance": "{{ resources.aws_instance.web.id }}"
        });

        let deps = resource.dependencies(&config);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].resource_type, "aws_instance");
        assert_eq!(deps[0].resource_name, "web");
    }
}
