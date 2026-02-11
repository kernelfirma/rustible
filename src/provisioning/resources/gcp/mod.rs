//! GCP Resource Implementations (Experimental Stubs)
//!
//! This module contains stub resource implementations for Google Cloud Platform
//! resources. Each resource implements the `Resource` trait with placeholder
//! operations that return `ProvisioningError::CloudApiError("not yet implemented")`.
//!
//! # Available Resources
//!
//! - `google_compute_instance` - GCE Virtual Machine Instances
//! - `google_compute_network` - VPC Networks
//! - `google_compute_subnetwork` - VPC Subnetworks
//! - `google_compute_firewall` - VPC Firewall Rules

use async_trait::async_trait;
use serde_json::Value;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    FieldType, ProviderContext, Resource, ResourceDependency, ResourceDiff, ResourceReadResult,
    ResourceResult, ResourceSchema, ResourceTimeouts, SchemaField,
};

// ============================================================================
// Google Compute Instance
// ============================================================================

/// Google Compute Engine instance resource (stub)
#[derive(Debug, Clone, Default)]
pub struct GoogleComputeInstance;

impl GoogleComputeInstance {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for GoogleComputeInstance {
    fn resource_type(&self) -> &str {
        "google_compute_instance"
    }

    fn provider(&self) -> &str {
        "gcp"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "google_compute_instance".to_string(),
            description: "Google Compute Engine virtual machine instance".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "machine_type".to_string(),
                    field_type: FieldType::String,
                    description: "Machine type (e.g., e2-medium)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "zone".to_string(),
                    field_type: FieldType::String,
                    description: "The zone (e.g., us-central1-a)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "boot_disk".to_string(),
                    field_type: FieldType::Object(vec![
                        SchemaField {
                            name: "image".to_string(),
                            field_type: FieldType::String,
                            description: "Boot disk image".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "size_gb".to_string(),
                            field_type: FieldType::Integer,
                            description: "Boot disk size in GB".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                    ]),
                    description: "Boot disk configuration".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "network_interface".to_string(),
                    field_type: FieldType::Object(vec![
                        SchemaField {
                            name: "network".to_string(),
                            field_type: FieldType::String,
                            description: "VPC network".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "subnetwork".to_string(),
                            field_type: FieldType::String,
                            description: "Subnetwork".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                    ]),
                    description: "Network interface configuration".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "labels".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Labels to apply".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Network tags".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Instance ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "self_link".to_string(),
                    field_type: FieldType::String,
                    description: "Self link URI".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "instance_id".to_string(),
                    field_type: FieldType::String,
                    description: "Server-assigned unique identifier".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string(), "zone".to_string()],
            timeouts: ResourceTimeouts {
                create: 600,
                read: 60,
                update: 600,
                delete: 600,
            },
        }
    }

    async fn read(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        match current {
            None => Ok(ResourceDiff::create(desired.clone())),
            Some(_) => Ok(ResourceDiff::no_change()),
        }
    }

    async fn create(
        &self,
        _config: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn update(
        &self,
        _id: &str,
        _old: &Value,
        _new: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn destroy(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn import(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        Vec::new()
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string(), "zone".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["name", "machine_type", "zone"] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{} is required for google_compute_instance",
                    field
                )));
            }
        }
        Ok(())
    }
}

// ============================================================================
// Google Compute Network
// ============================================================================

/// Google VPC Network resource (stub)
#[derive(Debug, Clone, Default)]
pub struct GoogleComputeNetwork;

impl GoogleComputeNetwork {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for GoogleComputeNetwork {
    fn resource_type(&self) -> &str {
        "google_compute_network"
    }

    fn provider(&self) -> &str {
        "gcp"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "google_compute_network".to_string(),
            description: "Google VPC Network".to_string(),
            required_args: vec![SchemaField {
                name: "name".to_string(),
                field_type: FieldType::String,
                description: "The name of the network".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "auto_create_subnetworks".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Automatically create subnetworks (default: true)".to_string(),
                    default: Some(Value::Bool(true)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "routing_mode".to_string(),
                    field_type: FieldType::String,
                    description: "Routing mode: REGIONAL or GLOBAL".to_string(),
                    default: Some(Value::String("REGIONAL".to_string())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Network description".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Network ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "self_link".to_string(),
                    field_type: FieldType::String,
                    description: "Self link URI".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "gateway_ipv4".to_string(),
                    field_type: FieldType::String,
                    description: "Gateway IPv4 address".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string()],
            timeouts: ResourceTimeouts::default(),
        }
    }

    async fn read(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        match current {
            None => Ok(ResourceDiff::create(desired.clone())),
            Some(_) => Ok(ResourceDiff::no_change()),
        }
    }

    async fn create(
        &self,
        _config: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn update(
        &self,
        _id: &str,
        _old: &Value,
        _new: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn destroy(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn import(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        Vec::new()
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        if config.get("name").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::ValidationError(
                "name is required for google_compute_network".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Google Compute Subnetwork
// ============================================================================

/// Google VPC Subnetwork resource (stub)
#[derive(Debug, Clone, Default)]
pub struct GoogleComputeSubnetwork;

impl GoogleComputeSubnetwork {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for GoogleComputeSubnetwork {
    fn resource_type(&self) -> &str {
        "google_compute_subnetwork"
    }

    fn provider(&self) -> &str {
        "gcp"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "google_compute_subnetwork".to_string(),
            description: "Google VPC Subnetwork".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the subnetwork".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "ip_cidr_range".to_string(),
                    field_type: FieldType::String,
                    description: "Primary IP CIDR range (e.g., 10.0.0.0/24)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "network".to_string(),
                    field_type: FieldType::String,
                    description: "The VPC network self_link or name".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "region".to_string(),
                    field_type: FieldType::String,
                    description: "The GCP region".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Subnetwork description".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_ip_google_access".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Enable private Google access".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Subnetwork ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "self_link".to_string(),
                    field_type: FieldType::String,
                    description: "Self link URI".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "gateway_address".to_string(),
                    field_type: FieldType::String,
                    description: "Gateway address for the subnetwork".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string(), "network".to_string(), "region".to_string()],
            timeouts: ResourceTimeouts::default(),
        }
    }

    async fn read(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        match current {
            None => Ok(ResourceDiff::create(desired.clone())),
            Some(_) => Ok(ResourceDiff::no_change()),
        }
    }

    async fn create(
        &self,
        _config: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn update(
        &self,
        _id: &str,
        _old: &Value,
        _new: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn destroy(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn import(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        Vec::new()
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string(), "network".to_string(), "region".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["name", "ip_cidr_range", "network", "region"] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{} is required for google_compute_subnetwork",
                    field
                )));
            }
        }
        Ok(())
    }
}

// ============================================================================
// Google Compute Firewall
// ============================================================================

/// Google VPC Firewall rule resource (stub)
#[derive(Debug, Clone, Default)]
pub struct GoogleComputeFirewall;

impl GoogleComputeFirewall {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for GoogleComputeFirewall {
    fn resource_type(&self) -> &str {
        "google_compute_firewall"
    }

    fn provider(&self) -> &str {
        "gcp"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "google_compute_firewall".to_string(),
            description: "Google VPC Firewall Rule".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the firewall rule".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "network".to_string(),
                    field_type: FieldType::String,
                    description: "The VPC network self_link or name".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "description".to_string(),
                    field_type: FieldType::String,
                    description: "Firewall rule description".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "direction".to_string(),
                    field_type: FieldType::String,
                    description: "Direction: INGRESS or EGRESS".to_string(),
                    default: Some(Value::String("INGRESS".to_string())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "priority".to_string(),
                    field_type: FieldType::Integer,
                    description: "Priority (0-65535, lower = higher priority)".to_string(),
                    default: Some(Value::Number(1000.into())),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "source_ranges".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Source CIDR ranges".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "target_tags".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Target network tags".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "allow".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::Object(vec![
                        SchemaField {
                            name: "protocol".to_string(),
                            field_type: FieldType::String,
                            description: "Protocol (tcp, udp, icmp, etc.)".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "ports".to_string(),
                            field_type: FieldType::List(Box::new(FieldType::String)),
                            description: "Port ranges".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                    ]))),
                    description: "Allow rules".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "deny".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::Object(vec![
                        SchemaField {
                            name: "protocol".to_string(),
                            field_type: FieldType::String,
                            description: "Protocol (tcp, udp, icmp, etc.)".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                        SchemaField {
                            name: "ports".to_string(),
                            field_type: FieldType::List(Box::new(FieldType::String)),
                            description: "Port ranges".to_string(),
                            default: None,
                            constraints: vec![],
                            sensitive: false,
                        },
                    ]))),
                    description: "Deny rules".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Firewall rule ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "self_link".to_string(),
                    field_type: FieldType::String,
                    description: "Self link URI".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string()],
            timeouts: ResourceTimeouts::default(),
        }
    }

    async fn read(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        match current {
            None => Ok(ResourceDiff::create(desired.clone())),
            Some(_) => Ok(ResourceDiff::no_change()),
        }
    }

    async fn create(
        &self,
        _config: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn update(
        &self,
        _id: &str,
        _old: &Value,
        _new: &Value,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn destroy(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    async fn import(
        &self,
        _id: &str,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        Err(ProvisioningError::CloudApiError(
            "not yet implemented".into(),
        ))
    }

    fn dependencies(&self, _config: &Value) -> Vec<ResourceDependency> {
        Vec::new()
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec!["name".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["name", "network"] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{} is required for google_compute_firewall",
                    field
                )));
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

    // -- Compute Instance --

    #[test]
    fn test_compute_instance_type() {
        let res = GoogleComputeInstance::new();
        assert_eq!(res.resource_type(), "google_compute_instance");
        assert_eq!(res.provider(), "gcp");
    }

    #[test]
    fn test_compute_instance_schema() {
        let res = GoogleComputeInstance::new();
        let schema = res.schema();
        assert_eq!(schema.resource_type, "google_compute_instance");
        assert_eq!(schema.required_args.len(), 3);
        assert_eq!(schema.timeouts.create, 600);

        let names: Vec<&str> = schema.required_args.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"machine_type"));
        assert!(names.contains(&"zone"));
    }

    #[test]
    fn test_compute_instance_validate() {
        let res = GoogleComputeInstance::new();
        let valid = serde_json::json!({
            "name": "instance-1",
            "machine_type": "e2-medium",
            "zone": "us-central1-a"
        });
        assert!(res.validate(&valid).is_ok());

        assert!(res.validate(&serde_json::json!({"name": "vm1"})).is_err());
        assert!(res.validate(&serde_json::json!({})).is_err());
    }

    #[test]
    fn test_compute_instance_forces_replacement() {
        let res = GoogleComputeInstance::new();
        let force_new = res.forces_replacement();
        assert!(force_new.contains(&"name".to_string()));
        assert!(force_new.contains(&"zone".to_string()));
    }

    // -- Compute Network --

    #[test]
    fn test_compute_network_type() {
        let res = GoogleComputeNetwork::new();
        assert_eq!(res.resource_type(), "google_compute_network");
        assert_eq!(res.provider(), "gcp");
    }

    #[test]
    fn test_compute_network_schema() {
        let res = GoogleComputeNetwork::new();
        let schema = res.schema();
        assert_eq!(schema.required_args.len(), 1);
        assert_eq!(schema.required_args[0].name, "name");

        // Check auto_create_subnetworks default
        let auto_create = schema
            .optional_args
            .iter()
            .find(|f| f.name == "auto_create_subnetworks")
            .unwrap();
        assert_eq!(auto_create.default, Some(Value::Bool(true)));
    }

    #[test]
    fn test_compute_network_validate() {
        let res = GoogleComputeNetwork::new();
        assert!(res.validate(&serde_json::json!({"name": "my-net"})).is_ok());
        assert!(res.validate(&serde_json::json!({})).is_err());
    }

    // -- Compute Subnetwork --

    #[test]
    fn test_compute_subnetwork_type() {
        let res = GoogleComputeSubnetwork::new();
        assert_eq!(res.resource_type(), "google_compute_subnetwork");
        assert_eq!(res.provider(), "gcp");
    }

    #[test]
    fn test_compute_subnetwork_schema() {
        let res = GoogleComputeSubnetwork::new();
        let schema = res.schema();
        assert_eq!(schema.required_args.len(), 4);
    }

    #[test]
    fn test_compute_subnetwork_validate() {
        let res = GoogleComputeSubnetwork::new();
        let valid = serde_json::json!({
            "name": "subnet-1",
            "ip_cidr_range": "10.0.0.0/24",
            "network": "my-network",
            "region": "us-central1"
        });
        assert!(res.validate(&valid).is_ok());

        let missing_cidr = serde_json::json!({
            "name": "subnet-1",
            "network": "my-network",
            "region": "us-central1"
        });
        assert!(res.validate(&missing_cidr).is_err());
    }

    #[test]
    fn test_compute_subnetwork_forces_replacement() {
        let res = GoogleComputeSubnetwork::new();
        let force_new = res.forces_replacement();
        assert!(force_new.contains(&"name".to_string()));
        assert!(force_new.contains(&"network".to_string()));
        assert!(force_new.contains(&"region".to_string()));
    }

    // -- Compute Firewall --

    #[test]
    fn test_compute_firewall_type() {
        let res = GoogleComputeFirewall::new();
        assert_eq!(res.resource_type(), "google_compute_firewall");
        assert_eq!(res.provider(), "gcp");
    }

    #[test]
    fn test_compute_firewall_schema() {
        let res = GoogleComputeFirewall::new();
        let schema = res.schema();
        assert_eq!(schema.required_args.len(), 2);

        let names: Vec<&str> = schema.required_args.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"network"));

        // Check direction default
        let direction = schema
            .optional_args
            .iter()
            .find(|f| f.name == "direction")
            .unwrap();
        assert_eq!(
            direction.default,
            Some(Value::String("INGRESS".to_string()))
        );
    }

    #[test]
    fn test_compute_firewall_validate() {
        let res = GoogleComputeFirewall::new();
        let valid = serde_json::json!({
            "name": "allow-ssh",
            "network": "my-network"
        });
        assert!(res.validate(&valid).is_ok());

        assert!(res.validate(&serde_json::json!({"name": "fw1"})).is_err());
        assert!(res.validate(&serde_json::json!({})).is_err());
    }

    // -- Cross-resource tests --

    #[test]
    fn test_all_dependencies_empty() {
        let instance = GoogleComputeInstance::new();
        let network = GoogleComputeNetwork::new();
        let subnet = GoogleComputeSubnetwork::new();
        let firewall = GoogleComputeFirewall::new();

        let config = serde_json::json!({});
        assert!(instance.dependencies(&config).is_empty());
        assert!(network.dependencies(&config).is_empty());
        assert!(subnet.dependencies(&config).is_empty());
        assert!(firewall.dependencies(&config).is_empty());
    }
}
