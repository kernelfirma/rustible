//! AWS NAT Gateway Resource for infrastructure provisioning
//!
//! This module implements the `Resource` trait for AWS NAT Gateways, enabling declarative
//! NAT Gateway management through the provisioning system.
//!
//! ## Example
//!
//! ```yaml
//! resources:
//!   aws_nat_gateway:
//!     main:
//!       subnet_id: "{{ resources.aws_subnet.public.id }}"
//!       allocation_id: "{{ resources.aws_eip.nat.id }}"
//!       connectivity_type: public
//!       tags:
//!         Name: main-nat-gateway
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
use aws_sdk_ec2::types::{ConnectivityType, ResourceType, Tag, TagSpecification};
#[cfg(feature = "aws")]
use aws_sdk_ec2::Client;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// NAT Gateway Resource Configuration
// ============================================================================

/// NAT Gateway resource attributes (computed from cloud)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatGatewayAttributes {
    /// NAT Gateway ID (e.g., nat-12345678)
    pub id: String,
    /// Subnet ID where NAT Gateway is located
    pub subnet_id: String,
    /// VPC ID
    pub vpc_id: String,
    /// Elastic IP allocation ID (for public NAT Gateway)
    pub allocation_id: Option<String>,
    /// Network interface ID
    pub network_interface_id: Option<String>,
    /// Private IP address
    pub private_ip: Option<String>,
    /// Public IP address (for public NAT Gateway)
    pub public_ip: Option<String>,
    /// Connectivity type (public or private)
    pub connectivity_type: String,
    /// State (pending, available, deleting, deleted, failed)
    pub state: String,
    /// Failure code if state is failed
    pub failure_code: Option<String>,
    /// Failure message if state is failed
    pub failure_message: Option<String>,
    /// Tags
    pub tags: HashMap<String, String>,
}

/// NAT Gateway configuration
#[derive(Debug, Clone)]
pub struct NatGatewayConfig {
    pub subnet_id: String,
    pub allocation_id: Option<String>,
    pub connectivity_type: String,
    pub private_ip_address: Option<String>,
    pub secondary_allocation_ids: Vec<String>,
    pub secondary_private_ip_addresses: Vec<String>,
    pub secondary_private_ip_address_count: Option<i32>,
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS NAT Gateway Resource Implementation
// ============================================================================

/// AWS NAT Gateway resource for infrastructure provisioning
///
/// This resource manages AWS NAT Gateways, which enable instances in private
/// subnets to connect to the internet while preventing the internet from
/// initiating connections to those instances.
#[derive(Debug, Clone, Default)]
pub struct AwsNatGatewayResource;

impl AwsNatGatewayResource {
    /// Create a new NAT Gateway resource
    pub fn new() -> Self {
        Self
    }

    /// Build the resource schema
    fn build_schema() -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_nat_gateway".to_string(),
            description: "AWS NAT Gateway for VPC internet access".to_string(),
            required_args: vec![SchemaField {
                name: "subnet_id".to_string(),
                field_type: FieldType::String,
                description: "The subnet ID to create the NAT Gateway in".to_string(),
                default: None,
                constraints: vec![FieldConstraint::Pattern {
                    regex: r"^subnet-[a-f0-9]+$".to_string(),
                }],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "allocation_id".to_string(),
                    field_type: FieldType::String,
                    description: "Elastic IP allocation ID (required for public NAT Gateway)"
                        .to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "connectivity_type".to_string(),
                    field_type: FieldType::String,
                    description: "Connectivity type: public or private (default: public)"
                        .to_string(),
                    default: Some(Value::String("public".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec!["public".to_string(), "private".to_string()],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_ip_address".to_string(),
                    field_type: FieldType::String,
                    description: "Primary private IP address to assign".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "secondary_allocation_ids".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Secondary EIP allocation IDs".to_string(),
                    default: Some(Value::Array(vec![])),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "secondary_private_ip_addresses".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Secondary private IP addresses".to_string(),
                    default: Some(Value::Array(vec![])),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "secondary_private_ip_address_count".to_string(),
                    field_type: FieldType::Integer,
                    description: "Number of secondary private IPs to assign".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Tags to apply to the NAT Gateway".to_string(),
                    default: Some(Value::Object(Default::default())),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "NAT Gateway ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "vpc_id".to_string(),
                    field_type: FieldType::String,
                    description: "VPC ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "network_interface_id".to_string(),
                    field_type: FieldType::String,
                    description: "Network interface ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_ip".to_string(),
                    field_type: FieldType::String,
                    description: "Private IP address".to_string(),
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
                    name: "state".to_string(),
                    field_type: FieldType::String,
                    description: "NAT Gateway state".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "subnet_id".to_string(),
                "connectivity_type".to_string(),
                "allocation_id".to_string(),
                "private_ip_address".to_string(),
            ],
            timeouts: ResourceTimeouts {
                create: 600, // NAT Gateways can take a while to create
                read: 60,
                update: 300,
                delete: 600, // Deletion can also be slow
            },
        }
    }

    /// Extract configuration values from JSON
    fn extract_config(config: &Value) -> ProvisioningResult<NatGatewayConfig> {
        let subnet_id = config
            .get("subnet_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProvisioningError::ValidationError("subnet_id is required".to_string()))?
            .to_string();

        let allocation_id = config
            .get("allocation_id")
            .and_then(|v| v.as_str())
            .map(String::from);

        let connectivity_type = config
            .get("connectivity_type")
            .and_then(|v| v.as_str())
            .unwrap_or("public")
            .to_string();

        let private_ip_address = config
            .get("private_ip_address")
            .and_then(|v| v.as_str())
            .map(String::from);

        let secondary_allocation_ids = config
            .get("secondary_allocation_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let secondary_private_ip_addresses = config
            .get("secondary_private_ip_addresses")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let secondary_private_ip_address_count = config
            .get("secondary_private_ip_address_count")
            .and_then(|v| v.as_i64())
            .map(|n| n as i32);

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

        Ok(NatGatewayConfig {
            subnet_id,
            allocation_id,
            connectivity_type,
            private_ip_address,
            secondary_allocation_ids,
            secondary_private_ip_addresses,
            secondary_private_ip_address_count,
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

    /// Read NAT Gateway by ID from AWS
    #[cfg(feature = "aws")]
    async fn read_nat_gateway_by_id(
        client: &Client,
        nat_gateway_id: &str,
    ) -> ProvisioningResult<Option<NatGatewayAttributes>> {
        let resp = client
            .describe_nat_gateways()
            .nat_gateway_ids(nat_gateway_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to describe NAT Gateway: {}", e))
            })?;

        if let Some(nat) = resp.nat_gateways().first() {
            // Skip if deleted
            if nat.state() == Some(&aws_sdk_ec2::types::NatGatewayState::Deleted) {
                return Ok(None);
            }

            let nat_id = nat.nat_gateway_id().unwrap_or_default().to_string();
            let subnet_id = nat.subnet_id().unwrap_or_default().to_string();
            let vpc_id = nat.vpc_id().unwrap_or_default().to_string();

            // Get addresses from the first address
            let (allocation_id, network_interface_id, private_ip, public_ip) =
                if let Some(addr) = nat.nat_gateway_addresses().first() {
                    (
                        addr.allocation_id().map(String::from),
                        addr.network_interface_id().map(String::from),
                        addr.private_ip().map(String::from),
                        addr.public_ip().map(String::from),
                    )
                } else {
                    (None, None, None, None)
                };

            // Extract tags
            let mut tags = HashMap::new();
            for tag in nat.tags() {
                if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                    tags.insert(key.to_string(), value.to_string());
                }
            }

            Ok(Some(NatGatewayAttributes {
                id: nat_id,
                subnet_id,
                vpc_id,
                allocation_id,
                network_interface_id,
                private_ip,
                public_ip,
                connectivity_type: nat
                    .connectivity_type()
                    .map(|c| c.as_str().to_string())
                    .unwrap_or_else(|| "public".to_string()),
                state: nat
                    .state()
                    .map(|s| s.as_str().to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                failure_code: nat.failure_code().map(String::from),
                failure_message: nat.failure_message().map(String::from),
                tags,
            }))
        } else {
            Ok(None)
        }
    }

    /// Create NAT Gateway in AWS
    #[cfg(feature = "aws")]
    async fn create_nat_gateway(
        client: &Client,
        config: &NatGatewayConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<NatGatewayAttributes> {
        // Build tags including default tags from context
        let mut all_tags: Vec<Tag> = ctx
            .default_tags
            .iter()
            .map(|(k, v)| Tag::builder().key(k).value(v).build())
            .collect();

        for (k, v) in &config.tags {
            all_tags.push(Tag::builder().key(k).value(v).build());
        }

        let connectivity_type = if config.connectivity_type == "private" {
            ConnectivityType::Private
        } else {
            ConnectivityType::Public
        };

        let mut req = client
            .create_nat_gateway()
            .subnet_id(&config.subnet_id)
            .connectivity_type(connectivity_type)
            .tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::Natgateway)
                    .set_tags(Some(all_tags))
                    .build(),
            );

        // allocation_id is required for public NAT Gateway
        if let Some(ref alloc_id) = config.allocation_id {
            req = req.allocation_id(alloc_id);
        }

        if let Some(ref private_ip) = config.private_ip_address {
            req = req.private_ip_address(private_ip);
        }

        for alloc_id in &config.secondary_allocation_ids {
            req = req.secondary_allocation_ids(alloc_id);
        }

        for private_ip in &config.secondary_private_ip_addresses {
            req = req.secondary_private_ip_addresses(private_ip);
        }

        if let Some(count) = config.secondary_private_ip_address_count {
            req = req.secondary_private_ip_address_count(count);
        }

        let resp = req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create NAT Gateway: {}", e))
        })?;

        let nat = resp.nat_gateway().ok_or_else(|| {
            ProvisioningError::CloudApiError("No NAT Gateway returned from create".to_string())
        })?;

        let nat_id = nat.nat_gateway_id().unwrap_or_default().to_string();

        // Wait for NAT Gateway to be available
        Self::wait_for_nat_gateway_available(client, &nat_id).await?;

        // Read the full NAT Gateway attributes
        Self::read_nat_gateway_by_id(client, &nat_id)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read created NAT Gateway".to_string())
            })
    }

    /// Wait for NAT Gateway to be available
    #[cfg(feature = "aws")]
    async fn wait_for_nat_gateway_available(
        client: &Client,
        nat_gateway_id: &str,
    ) -> ProvisioningResult<()> {
        use std::time::Duration;

        let max_attempts = 60; // Up to 10 minutes
        let delay = Duration::from_secs(10);

        for _ in 0..max_attempts {
            let resp = client
                .describe_nat_gateways()
                .nat_gateway_ids(nat_gateway_id)
                .send()
                .await
                .map_err(|e| {
                    ProvisioningError::CloudApiError(format!(
                        "Failed to describe NAT Gateway: {}",
                        e
                    ))
                })?;

            if let Some(nat) = resp.nat_gateways().first() {
                match nat.state() {
                    Some(state) if state.as_str() == "available" => {
                        return Ok(());
                    }
                    Some(state) if state.as_str() == "failed" => {
                        let failure_msg = nat
                            .failure_message()
                            .map(String::from)
                            .unwrap_or_else(|| "Unknown error".to_string());
                        return Err(ProvisioningError::CloudApiError(format!(
                            "NAT Gateway creation failed: {}",
                            failure_msg
                        )));
                    }
                    _ => {}
                }
            }

            tokio::time::sleep(delay).await;
        }

        Err(ProvisioningError::Timeout {
            operation: format!("Waiting for NAT Gateway {} to be available", nat_gateway_id),
            seconds: (max_attempts * 10) as u64,
        })
    }

    /// Update NAT Gateway in AWS (tags only - most fields force replacement)
    #[cfg(feature = "aws")]
    async fn update_nat_gateway(
        client: &Client,
        nat_id: &str,
        old_config: &NatGatewayConfig,
        new_config: &NatGatewayConfig,
    ) -> ProvisioningResult<NatGatewayAttributes> {
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
                    .resources(nat_id)
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
                    .resources(nat_id)
                    .set_tags(Some(tags_to_create))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to create tags: {}", e))
                    })?;
            }
        }

        // Read updated NAT Gateway
        Self::read_nat_gateway_by_id(client, nat_id)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read updated NAT Gateway".to_string())
            })
    }

    /// Delete NAT Gateway in AWS
    #[cfg(feature = "aws")]
    async fn delete_nat_gateway(client: &Client, nat_id: &str) -> ProvisioningResult<()> {
        client
            .delete_nat_gateway()
            .nat_gateway_id(nat_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete NAT Gateway: {}", e))
            })?;

        // Wait for deletion
        Self::wait_for_nat_gateway_deleted(client, nat_id).await
    }

    /// Wait for NAT Gateway to be deleted
    #[cfg(feature = "aws")]
    async fn wait_for_nat_gateway_deleted(
        client: &Client,
        nat_gateway_id: &str,
    ) -> ProvisioningResult<()> {
        use std::time::Duration;

        let max_attempts = 60;
        let delay = Duration::from_secs(10);

        for _ in 0..max_attempts {
            let resp = client
                .describe_nat_gateways()
                .nat_gateway_ids(nat_gateway_id)
                .send()
                .await;

            match resp {
                Ok(response) => {
                    if let Some(nat) = response.nat_gateways().first() {
                        if nat.state() == Some(&aws_sdk_ec2::types::NatGatewayState::Deleted) {
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    }
                }
                Err(_) => {
                    // NAT Gateway not found, assume deleted
                    return Ok(());
                }
            }

            tokio::time::sleep(delay).await;
        }

        Err(ProvisioningError::Timeout {
            operation: format!("Waiting for NAT Gateway {} to be deleted", nat_gateway_id),
            seconds: (max_attempts * 10) as u64,
        })
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
            "vpc_id",
            "network_interface_id",
            "private_ip",
            "public_ip",
            "state",
            "failure_code",
            "failure_message",
        ];

        if let (Some(desired_obj), Some(current_obj)) = (desired.as_object(), current.as_object()) {
            for (key, desired_val) in desired_obj {
                if computed_fields.contains(&key.as_str()) {
                    continue;
                }

                if let Some(current_val) = current_obj.get(key) {
                    if desired_val != current_val {
                        modifications
                            .insert(key.clone(), (current_val.clone(), desired_val.clone()));

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
        let has_changes =
            !additions.is_empty() || !modifications.is_empty() || !deletions.is_empty();

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

    /// Extract dependencies from config
    fn extract_dependencies(config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        // Subnet dependency
        if let Some(subnet_id) = config.get("subnet_id").and_then(|v| v.as_str()) {
            if subnet_id.contains("resources.aws_subnet.") {
                if let Some(name) = subnet_id
                    .strip_prefix("{{ resources.aws_subnet.")
                    .and_then(|s| s.strip_suffix(".id }}"))
                {
                    deps.push(ResourceDependency {
                        resource_type: "aws_subnet".to_string(),
                        resource_name: name.to_string(),
                        attribute: "id".to_string(),
                        hard: true,
                    });
                }
            }
        }

        // Elastic IP dependency
        if let Some(alloc_id) = config.get("allocation_id").and_then(|v| v.as_str()) {
            if alloc_id.contains("resources.aws_eip.") {
                if let Some(name) = alloc_id
                    .strip_prefix("{{ resources.aws_eip.")
                    .and_then(|s| s.strip_suffix(".id }}"))
                {
                    deps.push(ResourceDependency {
                        resource_type: "aws_eip".to_string(),
                        resource_name: name.to_string(),
                        attribute: "id".to_string(),
                        hard: true,
                    });
                }
            }
        }

        deps
    }
}

#[async_trait]
impl Resource for AwsNatGatewayResource {
    fn resource_type(&self) -> &str {
        "aws_nat_gateway"
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

        match Self::read_nat_gateway_by_id(&client, id).await? {
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
        let nat_config = Self::extract_config(config)?;
        let client = Self::create_client(ctx).await?;

        match Self::create_nat_gateway(&client, &nat_config, ctx).await {
            Ok(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;

                let mut result = ResourceResult::success(&attrs.id, attributes);
                result
                    .outputs
                    .insert("id".to_string(), Value::String(attrs.id.clone()));
                if let Some(ref public_ip) = attrs.public_ip {
                    result
                        .outputs
                        .insert("public_ip".to_string(), Value::String(public_ip.clone()));
                }
                if let Some(ref private_ip) = attrs.private_ip {
                    result
                        .outputs
                        .insert("private_ip".to_string(), Value::String(private_ip.clone()));
                }
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

        match Self::update_nat_gateway(&client, id, &old_config, &new_config).await {
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

        match Self::delete_nat_gateway(&client, id).await {
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

        match Self::read_nat_gateway_by_id(&client, id).await? {
            Some(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
                Ok(ResourceResult::success(id, attributes))
            }
            None => Err(ProvisioningError::ImportError {
                resource_type: "aws_nat_gateway".to_string(),
                resource_id: id.to_string(),
                message: "NAT Gateway not found".to_string(),
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
        Self::extract_dependencies(config)
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec![
            "subnet_id".to_string(),
            "connectivity_type".to_string(),
            "allocation_id".to_string(),
            "private_ip_address".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate required subnet_id
        config
            .get("subnet_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ProvisioningError::ValidationError("subnet_id is required".to_string())
            })?;

        // Validate connectivity_type if provided
        if let Some(conn_type) = config.get("connectivity_type").and_then(|v| v.as_str()) {
            if conn_type != "public" && conn_type != "private" {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid connectivity_type '{}'. Valid values: public, private",
                    conn_type
                )));
            }

            // Public NAT Gateway requires allocation_id
            if conn_type == "public" && config.get("allocation_id").is_none() {
                return Err(ProvisioningError::ValidationError(
                    "allocation_id is required for public NAT Gateway".to_string(),
                ));
            }
        } else {
            // Default is public, which requires allocation_id
            if config.get("allocation_id").is_none() {
                return Err(ProvisioningError::ValidationError(
                    "allocation_id is required for public NAT Gateway".to_string(),
                ));
            }
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
        let resource = AwsNatGatewayResource::new();
        assert_eq!(resource.resource_type(), "aws_nat_gateway");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsNatGatewayResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_nat_gateway");
        assert_eq!(schema.required_args.len(), 1);
        assert_eq!(schema.required_args[0].name, "subnet_id");
        assert!(schema.force_new.contains(&"subnet_id".to_string()));
        assert!(schema.force_new.contains(&"connectivity_type".to_string()));
    }

    #[test]
    fn test_validate_public_nat() {
        let resource = AwsNatGatewayResource::new();

        // Valid public NAT Gateway
        let config = serde_json::json!({
            "subnet_id": "subnet-12345",
            "allocation_id": "eipalloc-12345"
        });
        assert!(resource.validate(&config).is_ok());

        // Missing allocation_id for public NAT
        let config = serde_json::json!({
            "subnet_id": "subnet-12345"
        });
        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_validate_private_nat() {
        let resource = AwsNatGatewayResource::new();

        // Valid private NAT Gateway (no allocation_id needed)
        let config = serde_json::json!({
            "subnet_id": "subnet-12345",
            "connectivity_type": "private"
        });
        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_dependencies_extraction() {
        let config = serde_json::json!({
            "subnet_id": "{{ resources.aws_subnet.public.id }}",
            "allocation_id": "{{ resources.aws_eip.nat.id }}"
        });

        let deps = AwsNatGatewayResource::extract_dependencies(&config);
        assert_eq!(deps.len(), 2);
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_subnet" && d.resource_name == "public"));
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_eip" && d.resource_name == "nat"));
    }

    #[test]
    fn test_extract_config() {
        let config = serde_json::json!({
            "subnet_id": "subnet-12345",
            "allocation_id": "eipalloc-12345",
            "connectivity_type": "public",
            "tags": {
                "Name": "test-nat"
            }
        });

        let nat_config = AwsNatGatewayResource::extract_config(&config).unwrap();
        assert_eq!(nat_config.subnet_id, "subnet-12345");
        assert_eq!(nat_config.allocation_id, Some("eipalloc-12345".to_string()));
        assert_eq!(nat_config.connectivity_type, "public");
        assert_eq!(nat_config.tags.get("Name"), Some(&"test-nat".to_string()));
    }

    #[test]
    fn test_compute_diff_create() {
        let desired = serde_json::json!({
            "subnet_id": "subnet-12345",
            "allocation_id": "eipalloc-12345"
        });

        let diff = AwsNatGatewayResource::compute_diff(&desired, None, &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::Create);
    }

    #[test]
    fn test_compute_diff_no_change() {
        let desired = serde_json::json!({
            "subnet_id": "subnet-12345",
            "allocation_id": "eipalloc-12345"
        });

        let current = serde_json::json!({
            "subnet_id": "subnet-12345",
            "allocation_id": "eipalloc-12345",
            "id": "nat-12345",
            "vpc_id": "vpc-12345"
        });

        let diff = AwsNatGatewayResource::compute_diff(&desired, Some(&current), &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[test]
    fn test_compute_diff_replace() {
        let desired = serde_json::json!({
            "subnet_id": "subnet-67890",
            "allocation_id": "eipalloc-12345"
        });

        let current = serde_json::json!({
            "subnet_id": "subnet-12345",
            "allocation_id": "eipalloc-12345",
            "id": "nat-12345"
        });

        let force_new = vec!["subnet_id".to_string()];
        let diff =
            AwsNatGatewayResource::compute_diff(&desired, Some(&current), &force_new).unwrap();
        assert_eq!(diff.change_type, ChangeType::Replace);
        assert!(diff.requires_replacement);
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsNatGatewayResource::new();
        let force_new = resource.forces_replacement();

        assert!(force_new.contains(&"subnet_id".to_string()));
        assert!(force_new.contains(&"connectivity_type".to_string()));
        assert!(force_new.contains(&"allocation_id".to_string()));
    }
}
