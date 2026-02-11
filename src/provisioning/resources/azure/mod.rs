//! Azure Resource Implementations (Experimental Stubs)
//!
//! This module contains stub resource implementations for Azure resources.
//! Each resource implements the `Resource` trait with placeholder operations
//! that return `ProvisioningError::CloudApiError("not yet implemented")`.
//!
//! # Available Resources
//!
//! - `azurerm_resource_group` - Azure Resource Groups
//! - `azurerm_virtual_network` - Azure Virtual Networks
//! - `azurerm_subnet` - Azure Subnets
//! - `azurerm_network_interface` - Azure Network Interfaces
//! - `azurerm_linux_virtual_machine` - Azure Linux Virtual Machines

use async_trait::async_trait;
use serde_json::Value;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    FieldType, ProviderContext, Resource, ResourceDependency, ResourceDiff, ResourceReadResult,
    ResourceResult, ResourceSchema, ResourceTimeouts, SchemaField,
};

// ============================================================================
// Azure Resource Group
// ============================================================================

/// Azure Resource Group resource (stub)
#[derive(Debug, Clone, Default)]
pub struct AzurermResourceGroup;

impl AzurermResourceGroup {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for AzurermResourceGroup {
    fn resource_type(&self) -> &str {
        "azurerm_resource_group"
    }

    fn provider(&self) -> &str {
        "azure"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "azurerm_resource_group".to_string(),
            description: "Azure Resource Group".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the resource group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "location".to_string(),
                    field_type: FieldType::String,
                    description: "The Azure region (e.g., eastus)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![SchemaField {
                name: "tags".to_string(),
                field_type: FieldType::Map(Box::new(FieldType::String)),
                description: "Tags to apply to the resource group".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            computed_attrs: vec![SchemaField {
                name: "id".to_string(),
                field_type: FieldType::String,
                description: "Resource group ID".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            force_new: vec!["name".to_string(), "location".to_string()],
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
        vec!["name".to_string(), "location".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        if config.get("name").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::ValidationError(
                "name is required for azurerm_resource_group".to_string(),
            ));
        }
        if config.get("location").and_then(|v| v.as_str()).is_none() {
            return Err(ProvisioningError::ValidationError(
                "location is required for azurerm_resource_group".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Azure Virtual Network
// ============================================================================

/// Azure Virtual Network resource (stub)
#[derive(Debug, Clone, Default)]
pub struct AzurermVirtualNetwork;

impl AzurermVirtualNetwork {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for AzurermVirtualNetwork {
    fn resource_type(&self) -> &str {
        "azurerm_virtual_network"
    }

    fn provider(&self) -> &str {
        "azure"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "azurerm_virtual_network".to_string(),
            description: "Azure Virtual Network".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the virtual network".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "resource_group_name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the resource group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "location".to_string(),
                    field_type: FieldType::String,
                    description: "The Azure region".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "address_space".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Address space CIDR blocks".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![SchemaField {
                name: "tags".to_string(),
                field_type: FieldType::Map(Box::new(FieldType::String)),
                description: "Tags to apply".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            computed_attrs: vec![SchemaField {
                name: "id".to_string(),
                field_type: FieldType::String,
                description: "Virtual network ID".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            force_new: vec!["name".to_string(), "resource_group_name".to_string()],
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
        vec!["name".to_string(), "resource_group_name".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["name", "resource_group_name", "location"] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{} is required for azurerm_virtual_network",
                    field
                )));
            }
        }
        if config.get("address_space").and_then(|v| v.as_array()).is_none() {
            return Err(ProvisioningError::ValidationError(
                "address_space is required for azurerm_virtual_network".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Azure Subnet
// ============================================================================

/// Azure Subnet resource (stub)
#[derive(Debug, Clone, Default)]
pub struct AzurermSubnet;

impl AzurermSubnet {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for AzurermSubnet {
    fn resource_type(&self) -> &str {
        "azurerm_subnet"
    }

    fn provider(&self) -> &str {
        "azure"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "azurerm_subnet".to_string(),
            description: "Azure Subnet".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "resource_group_name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the resource group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "virtual_network_name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the virtual network".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "address_prefixes".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Address prefixes for the subnet".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![],
            computed_attrs: vec![SchemaField {
                name: "id".to_string(),
                field_type: FieldType::String,
                description: "Subnet ID".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            force_new: vec![
                "name".to_string(),
                "resource_group_name".to_string(),
                "virtual_network_name".to_string(),
            ],
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
        vec![
            "name".to_string(),
            "resource_group_name".to_string(),
            "virtual_network_name".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["name", "resource_group_name", "virtual_network_name"] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{} is required for azurerm_subnet",
                    field
                )));
            }
        }
        if config
            .get("address_prefixes")
            .and_then(|v| v.as_array())
            .is_none()
        {
            return Err(ProvisioningError::ValidationError(
                "address_prefixes is required for azurerm_subnet".to_string(),
            ));
        }
        Ok(())
    }
}

// ============================================================================
// Azure Network Interface
// ============================================================================

/// Azure Network Interface resource (stub)
#[derive(Debug, Clone, Default)]
pub struct AzurermNetworkInterface;

impl AzurermNetworkInterface {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for AzurermNetworkInterface {
    fn resource_type(&self) -> &str {
        "azurerm_network_interface"
    }

    fn provider(&self) -> &str {
        "azure"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "azurerm_network_interface".to_string(),
            description: "Azure Network Interface".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the network interface".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "resource_group_name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the resource group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "location".to_string(),
                    field_type: FieldType::String,
                    description: "The Azure region".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![SchemaField {
                name: "tags".to_string(),
                field_type: FieldType::Map(Box::new(FieldType::String)),
                description: "Tags to apply".to_string(),
                default: None,
                constraints: vec![],
                sensitive: false,
            }],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Network interface ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_ip_address".to_string(),
                    field_type: FieldType::String,
                    description: "Private IP address".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string(), "resource_group_name".to_string()],
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
        vec!["name".to_string(), "resource_group_name".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &["name", "resource_group_name", "location"] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{} is required for azurerm_network_interface",
                    field
                )));
            }
        }
        Ok(())
    }
}

// ============================================================================
// Azure Linux Virtual Machine
// ============================================================================

/// Azure Linux Virtual Machine resource (stub)
#[derive(Debug, Clone, Default)]
pub struct AzurermLinuxVirtualMachine;

impl AzurermLinuxVirtualMachine {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Resource for AzurermLinuxVirtualMachine {
    fn resource_type(&self) -> &str {
        "azurerm_linux_virtual_machine"
    }

    fn provider(&self) -> &str {
        "azure"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "azurerm_linux_virtual_machine".to_string(),
            description: "Azure Linux Virtual Machine".to_string(),
            required_args: vec![
                SchemaField {
                    name: "name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the virtual machine".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "resource_group_name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the resource group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "location".to_string(),
                    field_type: FieldType::String,
                    description: "The Azure region".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "size".to_string(),
                    field_type: FieldType::String,
                    description: "The VM size (e.g., Standard_DS1_v2)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "admin_username".to_string(),
                    field_type: FieldType::String,
                    description: "Admin username for the VM".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "admin_password".to_string(),
                    field_type: FieldType::String,
                    description: "Admin password (if not using SSH key)".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: true,
                },
                SchemaField {
                    name: "network_interface_ids".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "Network interface IDs to attach".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Tags to apply".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Virtual machine ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "public_ip_address".to_string(),
                    field_type: FieldType::String,
                    description: "Public IP address".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "private_ip_address".to_string(),
                    field_type: FieldType::String,
                    description: "Private IP address".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["name".to_string(), "resource_group_name".to_string()],
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
        vec!["name".to_string(), "resource_group_name".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        for field in &[
            "name",
            "resource_group_name",
            "location",
            "size",
            "admin_username",
        ] {
            if config.get(*field).and_then(|v| v.as_str()).is_none() {
                return Err(ProvisioningError::ValidationError(format!(
                    "{} is required for azurerm_linux_virtual_machine",
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

    #[test]
    fn test_resource_group_type() {
        let res = AzurermResourceGroup::new();
        assert_eq!(res.resource_type(), "azurerm_resource_group");
        assert_eq!(res.provider(), "azure");
    }

    #[test]
    fn test_resource_group_schema() {
        let res = AzurermResourceGroup::new();
        let schema = res.schema();
        assert_eq!(schema.resource_type, "azurerm_resource_group");
        assert_eq!(schema.required_args.len(), 2);

        let names: Vec<&str> = schema.required_args.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"location"));
    }

    #[test]
    fn test_resource_group_validate() {
        let res = AzurermResourceGroup::new();
        assert!(res
            .validate(&serde_json::json!({"name": "rg1", "location": "eastus"}))
            .is_ok());
        assert!(res.validate(&serde_json::json!({"name": "rg1"})).is_err());
        assert!(res.validate(&serde_json::json!({})).is_err());
    }

    #[test]
    fn test_virtual_network_type() {
        let res = AzurermVirtualNetwork::new();
        assert_eq!(res.resource_type(), "azurerm_virtual_network");
        assert_eq!(res.provider(), "azure");
    }

    #[test]
    fn test_virtual_network_schema() {
        let res = AzurermVirtualNetwork::new();
        let schema = res.schema();
        assert_eq!(schema.required_args.len(), 4);
    }

    #[test]
    fn test_virtual_network_validate() {
        let res = AzurermVirtualNetwork::new();
        let valid = serde_json::json!({
            "name": "vnet1",
            "resource_group_name": "rg1",
            "location": "eastus",
            "address_space": ["10.0.0.0/16"]
        });
        assert!(res.validate(&valid).is_ok());

        let missing_addr = serde_json::json!({
            "name": "vnet1",
            "resource_group_name": "rg1",
            "location": "eastus"
        });
        assert!(res.validate(&missing_addr).is_err());
    }

    #[test]
    fn test_subnet_type() {
        let res = AzurermSubnet::new();
        assert_eq!(res.resource_type(), "azurerm_subnet");
        assert_eq!(res.provider(), "azure");
    }

    #[test]
    fn test_subnet_validate() {
        let res = AzurermSubnet::new();
        let valid = serde_json::json!({
            "name": "subnet1",
            "resource_group_name": "rg1",
            "virtual_network_name": "vnet1",
            "address_prefixes": ["10.0.1.0/24"]
        });
        assert!(res.validate(&valid).is_ok());
        assert!(res.validate(&serde_json::json!({})).is_err());
    }

    #[test]
    fn test_network_interface_type() {
        let res = AzurermNetworkInterface::new();
        assert_eq!(res.resource_type(), "azurerm_network_interface");
        assert_eq!(res.provider(), "azure");
    }

    #[test]
    fn test_network_interface_validate() {
        let res = AzurermNetworkInterface::new();
        let valid = serde_json::json!({
            "name": "nic1",
            "resource_group_name": "rg1",
            "location": "eastus"
        });
        assert!(res.validate(&valid).is_ok());
        assert!(res.validate(&serde_json::json!({})).is_err());
    }

    #[test]
    fn test_linux_vm_type() {
        let res = AzurermLinuxVirtualMachine::new();
        assert_eq!(res.resource_type(), "azurerm_linux_virtual_machine");
        assert_eq!(res.provider(), "azure");
    }

    #[test]
    fn test_linux_vm_schema() {
        let res = AzurermLinuxVirtualMachine::new();
        let schema = res.schema();
        assert_eq!(schema.required_args.len(), 5);
        assert_eq!(schema.timeouts.create, 600);

        // Check that admin_password is marked sensitive
        let pw = schema
            .optional_args
            .iter()
            .find(|f| f.name == "admin_password")
            .unwrap();
        assert!(pw.sensitive);
    }

    #[test]
    fn test_linux_vm_validate() {
        let res = AzurermLinuxVirtualMachine::new();
        let valid = serde_json::json!({
            "name": "vm1",
            "resource_group_name": "rg1",
            "location": "eastus",
            "size": "Standard_DS1_v2",
            "admin_username": "azureuser"
        });
        assert!(res.validate(&valid).is_ok());
        assert!(res.validate(&serde_json::json!({"name": "vm1"})).is_err());
    }

    #[test]
    fn test_forces_replacement() {
        let rg = AzurermResourceGroup::new();
        assert!(rg.forces_replacement().contains(&"name".to_string()));

        let vm = AzurermLinuxVirtualMachine::new();
        assert!(vm.forces_replacement().contains(&"name".to_string()));
        assert!(vm
            .forces_replacement()
            .contains(&"resource_group_name".to_string()));
    }

    #[test]
    fn test_dependencies_empty() {
        let rg = AzurermResourceGroup::new();
        assert!(rg.dependencies(&serde_json::json!({})).is_empty());

        let vm = AzurermLinuxVirtualMachine::new();
        assert!(vm.dependencies(&serde_json::json!({})).is_empty());
    }
}
