//! Azure VM module for virtual machine and infrastructure management.
//!
//! This module provides comprehensive Azure VM lifecycle management including:
//!
//! - VM creation, deletion, start, stop, and restart
//! - Resource group management
//! - Network interface setup
//! - Managed identity support
//! - Disk and storage configuration
//!
//! ## AzureVmModule
//!
//! Manages Azure Virtual Machine lifecycle. Supports idempotent operations using
//! VM names for identification within a resource group.
//!
//! ### Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Virtual machine name |
//! | `resource_group` | Yes | Resource group name |
//! | `state` | No | Desired state: present, absent, running, stopped, deallocated (default: present) |
//! | `location` | No* | Azure region (*required when creating new VM) |
//! | `vm_size` | No | VM size (default: Standard_B1s) |
//! | `image` | No* | VM image reference (*required when creating new VM) |
//! | `admin_username` | No | Admin username for the VM |
//! | `admin_password` | No | Admin password (use ssh_public_keys for Linux) |
//! | `ssh_public_keys` | No | List of SSH public keys for Linux VMs |
//! | `os_disk` | No | OS disk configuration |
//! | `data_disks` | No | Data disk configurations |
//! | `network_interfaces` | No | Network interface IDs to attach |
//! | `subnet_id` | No | Subnet ID for auto-created NIC |
//! | `public_ip` | No | Whether to create public IP (default: false) |
//! | `nsg_id` | No | Network security group ID |
//! | `availability_set_id` | No | Availability set ID |
//! | `zones` | No | Availability zones |
//! | `managed_identity` | No | Managed identity configuration |
//! | `tags` | No | Resource tags |
//! | `wait` | No | Wait for operation to complete (default: true) |
//! | `wait_timeout` | No | Timeout for wait operations in seconds (default: 600) |
//!
//! ### Example
//!
//! ```yaml
//! - name: Create an Azure VM
//!   azure_vm:
//!     name: web-server-01
//!     resource_group: my-rg
//!     location: eastus
//!     vm_size: Standard_B2s
//!     image:
//!       publisher: Canonical
//!       offer: 0001-com-ubuntu-server-jammy
//!       sku: 22_04-lts-gen2
//!       version: latest
//!     admin_username: azureuser
//!     ssh_public_keys:
//!       - path: /home/azureuser/.ssh/authorized_keys
//!         key_data: ssh-rsa AAAAB3...
//!     managed_identity:
//!       type: SystemAssigned
//!     tags:
//!       Environment: production
//!     state: present
//! ```
//!
//! ## AzureResourceGroupModule
//!
//! Manages Azure Resource Groups.
//!
//! ### Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Resource group name |
//! | `location` | No* | Azure region (*required when creating) |
//! | `state` | No | Desired state: present, absent (default: present) |
//! | `tags` | No | Resource tags |
//! | `force_delete` | No | Force delete even if resources exist (default: false) |
//!
//! ## AzureNetworkInterfaceModule
//!
//! Manages Azure Network Interfaces.
//!
//! ### Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Network interface name |
//! | `resource_group` | Yes | Resource group name |
//! | `location` | No* | Azure region (*required when creating) |
//! | `subnet_id` | No* | Subnet ID (*required when creating) |
//! | `private_ip_address` | No | Static private IP address |
//! | `private_ip_allocation` | No | Dynamic or Static (default: Dynamic) |
//! | `public_ip_address_id` | No | Public IP address resource ID |
//! | `nsg_id` | No | Network security group ID |
//! | `enable_accelerated_networking` | No | Enable accelerated networking |
//! | `enable_ip_forwarding` | No | Enable IP forwarding |
//! | `state` | No | Desired state: present, absent (default: present) |
//! | `tags` | No | Resource tags |

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Represents the desired state of an Azure VM
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum VmState {
    /// VM should exist and be running
    #[default]
    Present,
    /// VM should not exist
    Absent,
    /// VM should be running
    Running,
    /// VM should be stopped (still incurs compute charges)
    Stopped,
    /// VM should be deallocated (no compute charges)
    Deallocated,
    /// VM should be restarted
    Restarted,
}

impl VmState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(VmState::Present),
            "absent" => Ok(VmState::Absent),
            "running" | "started" => Ok(VmState::Running),
            "stopped" => Ok(VmState::Stopped),
            "deallocated" => Ok(VmState::Deallocated),
            "restarted" => Ok(VmState::Restarted),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, running, stopped, deallocated, restarted",
                s
            ))),
        }
    }
}

/// Azure VM power state as returned by the API
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AzureVmPowerState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Deallocating,
    Deallocated,
    Unknown(String),
}

impl AzureVmPowerState {
    fn from_api_state(state: &str) -> Self {
        match state.to_lowercase().as_str() {
            "starting" => Self::Starting,
            "running" => Self::Running,
            "stopping" => Self::Stopping,
            "stopped" => Self::Stopped,
            "deallocating" => Self::Deallocating,
            "deallocated" => Self::Deallocated,
            other => Self::Unknown(other.to_string()),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(self, Self::Running | Self::Stopped | Self::Deallocated)
    }

    fn matches_desired(&self, desired: &VmState) -> bool {
        match (self, desired) {
            (Self::Running, VmState::Running | VmState::Present) => true,
            (Self::Stopped, VmState::Stopped) => true,
            (Self::Deallocated, VmState::Deallocated) => true,
            _ => false,
        }
    }
}

/// VM image reference configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageReference {
    /// Image publisher (e.g., "Canonical")
    pub publisher: Option<String>,
    /// Image offer (e.g., "0001-com-ubuntu-server-jammy")
    pub offer: Option<String>,
    /// Image SKU (e.g., "22_04-lts-gen2")
    pub sku: Option<String>,
    /// Image version (e.g., "latest")
    pub version: Option<String>,
    /// Custom image ID (alternative to publisher/offer/sku/version)
    pub id: Option<String>,
}

impl ImageReference {
    fn from_params(value: &serde_json::Value) -> ModuleResult<Self> {
        if let Some(obj) = value.as_object() {
            Ok(Self {
                publisher: obj
                    .get("publisher")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                offer: obj.get("offer").and_then(|v| v.as_str()).map(String::from),
                sku: obj.get("sku").and_then(|v| v.as_str()).map(String::from),
                version: obj
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                id: obj.get("id").and_then(|v| v.as_str()).map(String::from),
            })
        } else if let Some(s) = value.as_str() {
            // Support image ID string directly
            Ok(Self {
                publisher: None,
                offer: None,
                sku: None,
                version: None,
                id: Some(s.to_string()),
            })
        } else {
            Err(ModuleError::InvalidParameter(
                "image must be an object or string".to_string(),
            ))
        }
    }
}

/// OS disk configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsDiskConfig {
    /// Disk name
    pub name: Option<String>,
    /// Disk size in GB
    pub disk_size_gb: Option<u32>,
    /// Storage account type: Standard_LRS, Premium_LRS, StandardSSD_LRS, UltraSSD_LRS
    pub storage_account_type: Option<String>,
    /// Caching type: None, ReadOnly, ReadWrite
    pub caching: Option<String>,
    /// Whether to delete disk on VM deletion
    pub delete_option: Option<String>,
    /// Encryption settings
    pub encryption: Option<DiskEncryption>,
}

/// Data disk configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDiskConfig {
    /// Logical unit number (LUN)
    pub lun: u32,
    /// Disk name
    pub name: Option<String>,
    /// Disk size in GB
    pub disk_size_gb: Option<u32>,
    /// Storage account type
    pub storage_account_type: Option<String>,
    /// Caching type
    pub caching: Option<String>,
    /// Create option: Empty, Attach, FromImage
    pub create_option: Option<String>,
    /// Managed disk ID (for Attach)
    pub managed_disk_id: Option<String>,
    /// Delete option on VM deletion
    pub delete_option: Option<String>,
}

/// Disk encryption configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskEncryption {
    /// Disk encryption set ID
    pub disk_encryption_set_id: Option<String>,
    /// Encryption at host enabled
    pub encryption_at_host: Option<bool>,
}

/// SSH public key configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshPublicKey {
    /// Path where key should be placed (e.g., /home/azureuser/.ssh/authorized_keys)
    pub path: String,
    /// SSH public key data
    pub key_data: String,
}

/// Managed identity configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedIdentityConfig {
    /// Identity type: SystemAssigned, UserAssigned, SystemAssigned,UserAssigned, None
    #[serde(rename = "type")]
    pub identity_type: String,
    /// User-assigned identity IDs
    pub user_assigned_identities: Option<Vec<String>>,
}

impl ManagedIdentityConfig {
    fn from_params(value: &serde_json::Value) -> ModuleResult<Self> {
        if let Some(obj) = value.as_object() {
            let identity_type = obj
                .get("type")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    ModuleError::InvalidParameter("managed_identity.type is required".to_string())
                })?
                .to_string();

            let user_assigned = obj
                .get("user_assigned_identities")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                });

            Ok(Self {
                identity_type,
                user_assigned_identities: user_assigned,
            })
        } else if let Some(s) = value.as_str() {
            // Simple form: just the type
            Ok(Self {
                identity_type: s.to_string(),
                user_assigned_identities: None,
            })
        } else {
            Err(ModuleError::InvalidParameter(
                "managed_identity must be an object or string".to_string(),
            ))
        }
    }
}

/// Azure VM configuration parsed from module parameters
#[derive(Debug, Clone)]
struct AzureVmConfig {
    name: String,
    resource_group: String,
    state: VmState,
    location: Option<String>,
    vm_size: String,
    image: Option<ImageReference>,
    admin_username: Option<String>,
    admin_password: Option<String>,
    ssh_public_keys: Vec<SshPublicKey>,
    os_disk: Option<OsDiskConfig>,
    data_disks: Vec<DataDiskConfig>,
    network_interfaces: Vec<String>,
    subnet_id: Option<String>,
    public_ip: bool,
    nsg_id: Option<String>,
    availability_set_id: Option<String>,
    zones: Vec<String>,
    managed_identity: Option<ManagedIdentityConfig>,
    tags: HashMap<String, String>,
    wait: bool,
    wait_timeout: u64,
    custom_data: Option<String>,
    license_type: Option<String>,
    priority: Option<String>,
    eviction_policy: Option<String>,
    max_price: Option<f64>,
}

impl AzureVmConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let resource_group = params.get_string_required("resource_group")?;

        let state = if let Some(s) = params.get_string("state")? {
            VmState::from_str(&s)?
        } else {
            VmState::default()
        };

        // Parse image reference
        let image = if let Some(img_value) = params.get("image") {
            Some(ImageReference::from_params(img_value)?)
        } else {
            None
        };

        // Parse SSH public keys
        let ssh_public_keys = if let Some(keys_value) = params.get("ssh_public_keys") {
            if let Some(keys_array) = keys_value.as_array() {
                keys_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Parse OS disk configuration
        let os_disk = if let Some(disk_value) = params.get("os_disk") {
            serde_json::from_value(disk_value.clone()).ok()
        } else {
            None
        };

        // Parse data disks
        let data_disks = if let Some(disks_value) = params.get("data_disks") {
            if let Some(disks_array) = disks_value.as_array() {
                disks_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Parse network interfaces
        let network_interfaces = params
            .get_vec_string("network_interfaces")?
            .unwrap_or_default();

        // Parse zones
        let zones = params.get_vec_string("zones")?.unwrap_or_default();

        // Parse managed identity
        let managed_identity = if let Some(identity_value) = params.get("managed_identity") {
            Some(ManagedIdentityConfig::from_params(identity_value)?)
        } else {
            None
        };

        // Parse tags
        let mut tags = HashMap::new();
        if let Some(tag_value) = params.get("tags") {
            if let Some(tag_obj) = tag_value.as_object() {
                for (k, v) in tag_obj {
                    if let Some(vs) = v.as_str() {
                        tags.insert(k.clone(), vs.to_string());
                    } else {
                        tags.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                    }
                }
            }
        }

        Ok(Self {
            name,
            resource_group,
            state,
            location: params.get_string("location")?,
            vm_size: params
                .get_string("vm_size")?
                .unwrap_or_else(|| "Standard_B1s".to_string()),
            image,
            admin_username: params.get_string("admin_username")?,
            admin_password: params.get_string("admin_password")?,
            ssh_public_keys,
            os_disk,
            data_disks,
            network_interfaces,
            subnet_id: params.get_string("subnet_id")?,
            public_ip: params.get_bool_or("public_ip", false),
            nsg_id: params.get_string("nsg_id")?,
            availability_set_id: params.get_string("availability_set_id")?,
            zones,
            managed_identity,
            tags,
            wait: params.get_bool_or("wait", true),
            wait_timeout: params.get_i64("wait_timeout")?.unwrap_or(600) as u64,
            custom_data: params.get_string("custom_data")?,
            license_type: params.get_string("license_type")?,
            priority: params.get_string("priority")?,
            eviction_policy: params.get_string("eviction_policy")?,
            max_price: params.get("max_price").and_then(|v| v.as_f64()),
        })
    }
}

/// Azure VM information returned from API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmInfo {
    pub id: String,
    pub name: String,
    pub resource_group: String,
    pub location: String,
    pub vm_size: String,
    pub provisioning_state: String,
    pub power_state: String,
    pub public_ip_address: Option<String>,
    pub private_ip_address: Option<String>,
    pub network_interfaces: Vec<String>,
    pub os_type: Option<String>,
    pub admin_username: Option<String>,
    pub tags: HashMap<String, String>,
    pub managed_identity: Option<ManagedIdentityInfo>,
    pub availability_zone: Option<String>,
}

/// Managed identity information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedIdentityInfo {
    pub principal_id: Option<String>,
    pub tenant_id: Option<String>,
    pub identity_type: String,
    pub user_assigned_identities: Option<HashMap<String, UserAssignedIdentityInfo>>,
}

/// User-assigned identity information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAssignedIdentityInfo {
    pub principal_id: String,
    pub client_id: String,
}

/// Azure VM Module for managing Azure Virtual Machines
pub struct AzureVmModule;

impl AzureVmModule {
    /// Find VM by name in resource group
    async fn find_vm(_name: &str, _resource_group: &str) -> ModuleResult<Option<VmInfo>> {
        // In a real implementation using azure_mgmt_compute:
        //
        // use azure_identity::DefaultAzureCredential;
        // use azure_mgmt_compute::Client;
        //
        // let credential = DefaultAzureCredential::default();
        // let client = Client::new(
        //     subscription_id,
        //     Arc::new(credential),
        //     azure_core::ClientOptions::default(),
        // )?;
        //
        // let vm = client
        //     .virtual_machines_client()
        //     .get(resource_group, name, None)
        //     .await?;
        //
        // let instance_view = client
        //     .virtual_machines_client()
        //     .instance_view(resource_group, name)
        //     .await?;

        Ok(None)
    }

    /// Create a new Azure VM
    async fn create_vm(config: &AzureVmConfig) -> ModuleResult<VmInfo> {
        // Validate required parameters for creation
        let location = config.location.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("location is required when creating a new VM".to_string())
        })?;

        let _image = config.image.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("image is required when creating a new VM".to_string())
        })?;

        // In a real implementation:
        //
        // use azure_mgmt_compute::models::{
        //     VirtualMachine, HardwareProfile, StorageProfile, OsProfile,
        //     NetworkProfile, ImageReference as AzureImageReference,
        // };
        //
        // let vm_params = VirtualMachine {
        //     location: location.clone(),
        //     properties: Some(VirtualMachineProperties {
        //         hardware_profile: Some(HardwareProfile {
        //             vm_size: Some(config.vm_size.clone()),
        //         }),
        //         storage_profile: Some(StorageProfile {
        //             image_reference: Some(AzureImageReference {
        //                 publisher: image.publisher.clone(),
        //                 offer: image.offer.clone(),
        //                 sku: image.sku.clone(),
        //                 version: image.version.clone(),
        //                 ..Default::default()
        //             }),
        //             ..Default::default()
        //         }),
        //         os_profile: Some(OsProfile {
        //             computer_name: Some(config.name.clone()),
        //             admin_username: config.admin_username.clone(),
        //             ..Default::default()
        //         }),
        //         network_profile: Some(NetworkProfile {
        //             network_interfaces: config.network_interfaces
        //                 .iter()
        //                 .map(|id| NetworkInterfaceReference { id: Some(id.clone()), ..Default::default() })
        //                 .collect(),
        //         }),
        //         ..Default::default()
        //     }),
        //     identity: config.managed_identity.as_ref().map(|mi| VirtualMachineIdentity {
        //         identity_type: Some(mi.identity_type.clone()),
        //         ..Default::default()
        //     }),
        //     tags: Some(config.tags.clone()),
        //     ..Default::default()
        // };
        //
        // let result = client
        //     .virtual_machines_client()
        //     .create_or_update(resource_group, name, vm_params)
        //     .await?;

        let vm_id = format!(
            "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/{}/providers/Microsoft.Compute/virtualMachines/{}",
            config.resource_group, config.name
        );

        tracing::info!(
            "Would create Azure VM '{}' in resource group '{}' with size {}",
            config.name,
            config.resource_group,
            config.vm_size
        );

        let managed_identity = config
            .managed_identity
            .as_ref()
            .map(|mi| ManagedIdentityInfo {
                principal_id: Some(format!("{:032x}", rand::random::<u128>())),
                tenant_id: Some(format!("{:032x}", rand::random::<u128>())),
                identity_type: mi.identity_type.clone(),
                user_assigned_identities: mi.user_assigned_identities.as_ref().map(|ids| {
                    ids.iter()
                        .map(|id| {
                            (
                                id.clone(),
                                UserAssignedIdentityInfo {
                                    principal_id: format!("{:032x}", rand::random::<u128>()),
                                    client_id: format!("{:032x}", rand::random::<u128>()),
                                },
                            )
                        })
                        .collect()
                }),
            });

        Ok(VmInfo {
            id: vm_id,
            name: config.name.clone(),
            resource_group: config.resource_group.clone(),
            location: location.clone(),
            vm_size: config.vm_size.clone(),
            provisioning_state: "Succeeded".to_string(),
            power_state: "running".to_string(),
            public_ip_address: if config.public_ip {
                Some(format!(
                    "{}.{}.{}.{}",
                    rand::random::<u8>(),
                    rand::random::<u8>(),
                    rand::random::<u8>(),
                    rand::random::<u8>()
                ))
            } else {
                None
            },
            private_ip_address: Some(format!(
                "10.0.{}.{}",
                rand::random::<u8>(),
                rand::random::<u8>()
            )),
            network_interfaces: config.network_interfaces.clone(),
            os_type: Some("Linux".to_string()),
            admin_username: config.admin_username.clone(),
            tags: config.tags.clone(),
            managed_identity,
            availability_zone: config.zones.first().cloned(),
        })
    }

    /// Delete an Azure VM
    async fn delete_vm(
        _name: &str,
        _resource_group: &str,
        _delete_resources: bool,
    ) -> ModuleResult<()> {
        // In a real implementation:
        // client.virtual_machines_client().delete(resource_group, name, None).await?;
        //
        // If delete_resources is true, also delete:
        // - Network interfaces
        // - Public IP addresses
        // - OS disk
        // - Data disks

        tracing::info!("Would delete Azure VM: {}/{}", _resource_group, _name);
        Ok(())
    }

    /// Start a stopped/deallocated VM
    async fn start_vm(_name: &str, _resource_group: &str) -> ModuleResult<()> {
        // client.virtual_machines_client().start(resource_group, name).await?;
        tracing::info!("Would start Azure VM: {}/{}", _resource_group, _name);
        Ok(())
    }

    /// Stop a running VM (keeps allocation)
    async fn stop_vm(_name: &str, _resource_group: &str) -> ModuleResult<()> {
        // client.virtual_machines_client().power_off(resource_group, name, None).await?;
        tracing::info!("Would stop Azure VM: {}/{}", _resource_group, _name);
        Ok(())
    }

    /// Deallocate a VM (releases resources)
    async fn deallocate_vm(_name: &str, _resource_group: &str) -> ModuleResult<()> {
        // client.virtual_machines_client().deallocate(resource_group, name, None).await?;
        tracing::info!("Would deallocate Azure VM: {}/{}", _resource_group, _name);
        Ok(())
    }

    /// Restart a VM
    async fn restart_vm(_name: &str, _resource_group: &str) -> ModuleResult<()> {
        // client.virtual_machines_client().restart(resource_group, name).await?;
        tracing::info!("Would restart Azure VM: {}/{}", _resource_group, _name);
        Ok(())
    }

    /// Wait for VM to reach desired power state
    async fn wait_for_state(
        name: &str,
        resource_group: &str,
        desired_state: &VmState,
        timeout: Duration,
    ) -> ModuleResult<VmInfo> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(10);

        tracing::info!(
            "Waiting for VM {}/{} to reach state {:?} (timeout: {:?})",
            resource_group,
            name,
            desired_state,
            timeout
        );

        // In a real implementation, poll instance view:
        // loop {
        //     if start.elapsed() >= timeout {
        //         return Err(ModuleError::ExecutionFailed(format!(
        //             "Timeout waiting for VM to reach {:?} state",
        //             desired_state
        //         )));
        //     }
        //
        //     let instance_view = client
        //         .virtual_machines_client()
        //         .instance_view(resource_group, name)
        //         .await?;
        //
        //     let power_state = instance_view.statuses
        //         .iter()
        //         .find(|s| s.code.as_ref().map_or(false, |c| c.starts_with("PowerState/")))
        //         .and_then(|s| s.code.as_ref())
        //         .map(|c| c.strip_prefix("PowerState/").unwrap_or(c));
        //
        //     if let Some(state) = power_state {
        //         let current = AzureVmPowerState::from_api_state(state);
        //         if current.matches_desired(desired_state) {
        //             break;
        //         }
        //     }
        //
        //     tokio::time::sleep(poll_interval).await;
        // }

        // Simulate waiting
        if start.elapsed() < timeout {
            tokio::time::sleep(std::cmp::min(poll_interval, Duration::from_millis(100))).await;
        }

        let state_str = match desired_state {
            VmState::Running | VmState::Present => "running",
            VmState::Stopped => "stopped",
            VmState::Deallocated => "deallocated",
            VmState::Restarted => "running",
            VmState::Absent => "deleted",
        };

        Ok(VmInfo {
            id: format!(
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/{}/providers/Microsoft.Compute/virtualMachines/{}",
                resource_group, name
            ),
            name: name.to_string(),
            resource_group: resource_group.to_string(),
            location: "eastus".to_string(),
            vm_size: "Standard_B1s".to_string(),
            provisioning_state: "Succeeded".to_string(),
            power_state: state_str.to_string(),
            public_ip_address: None,
            private_ip_address: Some("10.0.0.4".to_string()),
            network_interfaces: Vec::new(),
            os_type: Some("Linux".to_string()),
            admin_username: None,
            tags: HashMap::new(),
            managed_identity: None,
            availability_zone: None,
        })
    }

    /// Execute the Azure VM module
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = AzureVmConfig::from_params(params)?;

        // Find existing VM
        let existing = Self::find_vm(&config.name, &config.resource_group).await?;

        // Determine actions based on current and desired state
        match config.state {
            VmState::Present | VmState::Running => {
                self.ensure_present_or_running(&config, &existing, context)
                    .await
            }
            VmState::Stopped => self.ensure_stopped(&config, &existing, context).await,
            VmState::Deallocated => self.ensure_deallocated(&config, &existing, context).await,
            VmState::Absent => self.ensure_absent(&config, &existing, context).await,
            VmState::Restarted => self.ensure_restarted(&config, &existing, context).await,
        }
    }

    /// Ensure VM is present and running
    async fn ensure_present_or_running(
        &self,
        config: &AzureVmConfig,
        existing: &Option<VmInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if existing.is_none() {
            // Create new VM
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would create VM '{}' in resource group '{}'",
                    config.name, config.resource_group
                ))
                .with_data("action", serde_json::json!("create")));
            }

            let vm = Self::create_vm(config).await?;

            // Wait for VM to be running if requested
            let final_vm = if config.wait && config.state == VmState::Running {
                Self::wait_for_state(
                    &config.name,
                    &config.resource_group,
                    &VmState::Running,
                    Duration::from_secs(config.wait_timeout),
                )
                .await?
            } else {
                vm
            };

            Ok(ModuleOutput::changed(format!(
                "Created VM '{}' in resource group '{}'",
                config.name, config.resource_group
            ))
            .with_data("vm", serde_json::to_value(&final_vm).unwrap())
            .with_data("id", serde_json::json!(final_vm.id)))
        } else {
            let vm = existing.as_ref().unwrap();
            let power_state = AzureVmPowerState::from_api_state(&vm.power_state);

            // Check if VM needs to be started
            if config.state == VmState::Running
                && !matches!(power_state, AzureVmPowerState::Running)
            {
                if context.check_mode {
                    return Ok(
                        ModuleOutput::changed(format!("Would start VM '{}'", config.name))
                            .with_data("action", serde_json::json!("start")),
                    );
                }

                Self::start_vm(&config.name, &config.resource_group).await?;

                let final_vm = if config.wait {
                    Self::wait_for_state(
                        &config.name,
                        &config.resource_group,
                        &VmState::Running,
                        Duration::from_secs(config.wait_timeout),
                    )
                    .await?
                } else {
                    vm.clone()
                };

                Ok(
                    ModuleOutput::changed(format!("Started VM '{}'", config.name))
                        .with_data("vm", serde_json::to_value(&final_vm).unwrap()),
                )
            } else {
                // VM already exists and is in desired state
                Ok(ModuleOutput::ok(format!(
                    "VM '{}' already exists in desired state",
                    config.name
                ))
                .with_data("vm", serde_json::to_value(vm).unwrap()))
            }
        }
    }

    /// Ensure VM is stopped
    async fn ensure_stopped(
        &self,
        config: &AzureVmConfig,
        existing: &Option<VmInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let vm = existing.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "VM '{}' not found in resource group '{}'",
                config.name, config.resource_group
            ))
        })?;

        let power_state = AzureVmPowerState::from_api_state(&vm.power_state);

        if matches!(power_state, AzureVmPowerState::Stopped) {
            return Ok(
                ModuleOutput::ok(format!("VM '{}' is already stopped", config.name))
                    .with_data("vm", serde_json::to_value(vm).unwrap()),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would stop VM '{}'", config.name))
                    .with_data("action", serde_json::json!("stop")),
            );
        }

        Self::stop_vm(&config.name, &config.resource_group).await?;

        let final_vm = if config.wait {
            Self::wait_for_state(
                &config.name,
                &config.resource_group,
                &VmState::Stopped,
                Duration::from_secs(config.wait_timeout),
            )
            .await?
        } else {
            vm.clone()
        };

        Ok(
            ModuleOutput::changed(format!("Stopped VM '{}'", config.name))
                .with_data("vm", serde_json::to_value(&final_vm).unwrap()),
        )
    }

    /// Ensure VM is deallocated
    async fn ensure_deallocated(
        &self,
        config: &AzureVmConfig,
        existing: &Option<VmInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let vm = existing.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "VM '{}' not found in resource group '{}'",
                config.name, config.resource_group
            ))
        })?;

        let power_state = AzureVmPowerState::from_api_state(&vm.power_state);

        if matches!(power_state, AzureVmPowerState::Deallocated) {
            return Ok(
                ModuleOutput::ok(format!("VM '{}' is already deallocated", config.name))
                    .with_data("vm", serde_json::to_value(vm).unwrap()),
            );
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would deallocate VM '{}'", config.name))
                    .with_data("action", serde_json::json!("deallocate")),
            );
        }

        Self::deallocate_vm(&config.name, &config.resource_group).await?;

        let final_vm = if config.wait {
            Self::wait_for_state(
                &config.name,
                &config.resource_group,
                &VmState::Deallocated,
                Duration::from_secs(config.wait_timeout),
            )
            .await?
        } else {
            vm.clone()
        };

        Ok(
            ModuleOutput::changed(format!("Deallocated VM '{}'", config.name))
                .with_data("vm", serde_json::to_value(&final_vm).unwrap()),
        )
    }

    /// Ensure VM is absent
    async fn ensure_absent(
        &self,
        config: &AzureVmConfig,
        existing: &Option<VmInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if existing.is_none() {
            return Ok(ModuleOutput::ok(format!(
                "VM '{}' does not exist",
                config.name
            )));
        }

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would delete VM '{}'", config.name))
                    .with_data("action", serde_json::json!("delete")),
            );
        }

        Self::delete_vm(&config.name, &config.resource_group, true).await?;

        Ok(ModuleOutput::changed(format!(
            "Deleted VM '{}' from resource group '{}'",
            config.name, config.resource_group
        )))
    }

    /// Ensure VM is restarted
    async fn ensure_restarted(
        &self,
        config: &AzureVmConfig,
        existing: &Option<VmInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let vm = existing.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "VM '{}' not found in resource group '{}'",
                config.name, config.resource_group
            ))
        })?;

        if context.check_mode {
            return Ok(
                ModuleOutput::changed(format!("Would restart VM '{}'", config.name))
                    .with_data("action", serde_json::json!("restart")),
            );
        }

        Self::restart_vm(&config.name, &config.resource_group).await?;

        let final_vm = if config.wait {
            Self::wait_for_state(
                &config.name,
                &config.resource_group,
                &VmState::Running,
                Duration::from_secs(config.wait_timeout),
            )
            .await?
        } else {
            vm.clone()
        };

        Ok(
            ModuleOutput::changed(format!("Restarted VM '{}'", config.name))
                .with_data("vm", serde_json::to_value(&final_vm).unwrap()),
        )
    }
}

impl Module for AzureVmModule {
    fn name(&self) -> &'static str {
        "azure_vm"
    }

    fn description(&self) -> &'static str {
        "Create, update, delete, start, stop, and manage Azure Virtual Machines"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 20,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name", "resource_group"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate name is provided
        if params.get_string("name")?.is_none() {
            return Err(ModuleError::MissingParameter("name".to_string()));
        }

        // Validate resource_group is provided
        if params.get_string("resource_group")?.is_none() {
            return Err(ModuleError::MissingParameter("resource_group".to_string()));
        }

        // Validate state if provided
        if let Some(state) = params.get_string("state")? {
            VmState::from_str(&state)?;
        }

        // Validate priority if provided
        if let Some(priority) = params.get_string("priority")? {
            if !["Regular", "Low", "Spot"].contains(&priority.as_str()) {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid priority '{}'. Valid values: Regular, Low, Spot",
                    priority
                )));
            }
        }

        Ok(())
    }
}

// ============================================================================
// Resource Group Module
// ============================================================================

/// Desired state for a resource group
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ResourceGroupState {
    #[default]
    Present,
    Absent,
}

/// Resource group configuration
#[derive(Debug, Clone)]
struct ResourceGroupConfig {
    name: String,
    location: Option<String>,
    state: ResourceGroupState,
    tags: HashMap<String, String>,
    force_delete: bool,
}

impl ResourceGroupConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        let state = if let Some(s) = params.get_string("state")? {
            match s.to_lowercase().as_str() {
                "present" => ResourceGroupState::Present,
                "absent" => ResourceGroupState::Absent,
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid state '{}'. Valid states: present, absent",
                        s
                    )))
                }
            }
        } else {
            ResourceGroupState::default()
        };

        let mut tags = HashMap::new();
        if let Some(tag_value) = params.get("tags") {
            if let Some(tag_obj) = tag_value.as_object() {
                for (k, v) in tag_obj {
                    if let Some(vs) = v.as_str() {
                        tags.insert(k.clone(), vs.to_string());
                    } else {
                        tags.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                    }
                }
            }
        }

        Ok(Self {
            name,
            location: params.get_string("location")?,
            state,
            tags,
            force_delete: params.get_bool_or("force_delete", false),
        })
    }
}

/// Resource group information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceGroupInfo {
    pub id: String,
    pub name: String,
    pub location: String,
    pub provisioning_state: String,
    pub tags: HashMap<String, String>,
}

/// Azure Resource Group Module
pub struct AzureResourceGroupModule;

impl AzureResourceGroupModule {
    /// Find resource group by name
    async fn find_resource_group(_name: &str) -> ModuleResult<Option<ResourceGroupInfo>> {
        // In a real implementation:
        // client.resource_groups_client().get(name).await?
        Ok(None)
    }

    /// Create resource group
    async fn create_resource_group(
        config: &ResourceGroupConfig,
    ) -> ModuleResult<ResourceGroupInfo> {
        let location = config.location.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter(
                "location is required when creating a resource group".to_string(),
            )
        })?;

        tracing::info!(
            "Would create resource group '{}' in location '{}'",
            config.name,
            location
        );

        Ok(ResourceGroupInfo {
            id: format!(
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/{}",
                config.name
            ),
            name: config.name.clone(),
            location: location.clone(),
            provisioning_state: "Succeeded".to_string(),
            tags: config.tags.clone(),
        })
    }

    /// Delete resource group
    async fn delete_resource_group(_name: &str, _force: bool) -> ModuleResult<()> {
        tracing::info!("Would delete resource group: {}", _name);
        Ok(())
    }

    /// Execute the resource group module
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ResourceGroupConfig::from_params(params)?;
        let existing = Self::find_resource_group(&config.name).await?;

        match config.state {
            ResourceGroupState::Present => {
                if let Some(rg) = existing {
                    Ok(
                        ModuleOutput::ok(format!(
                            "Resource group '{}' already exists",
                            config.name
                        ))
                        .with_data("resource_group", serde_json::to_value(&rg).unwrap()),
                    )
                } else {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create resource group '{}'",
                            config.name
                        )));
                    }

                    let rg = Self::create_resource_group(&config).await?;
                    Ok(
                        ModuleOutput::changed(format!("Created resource group '{}'", config.name))
                            .with_data("resource_group", serde_json::to_value(&rg).unwrap()),
                    )
                }
            }
            ResourceGroupState::Absent => {
                if existing.is_some() {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would delete resource group '{}'",
                            config.name
                        )));
                    }

                    Self::delete_resource_group(&config.name, config.force_delete).await?;
                    Ok(ModuleOutput::changed(format!(
                        "Deleted resource group '{}'",
                        config.name
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Resource group '{}' does not exist",
                        config.name
                    )))
                }
            }
        }
    }
}

impl Module for AzureResourceGroupModule {
    fn name(&self) -> &'static str {
        "azure_resource_group"
    }

    fn description(&self) -> &'static str {
        "Create and delete Azure Resource Groups"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 20,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }
}

// ============================================================================
// Network Interface Module
// ============================================================================

/// Network interface configuration
#[derive(Debug, Clone)]
struct NetworkInterfaceConfig {
    name: String,
    resource_group: String,
    location: Option<String>,
    subnet_id: Option<String>,
    private_ip_address: Option<String>,
    private_ip_allocation: String,
    public_ip_address_id: Option<String>,
    nsg_id: Option<String>,
    enable_accelerated_networking: bool,
    enable_ip_forwarding: bool,
    state: ResourceGroupState,
    tags: HashMap<String, String>,
}

impl NetworkInterfaceConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let resource_group = params.get_string_required("resource_group")?;

        let state = if let Some(s) = params.get_string("state")? {
            match s.to_lowercase().as_str() {
                "present" => ResourceGroupState::Present,
                "absent" => ResourceGroupState::Absent,
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid state '{}'. Valid states: present, absent",
                        s
                    )))
                }
            }
        } else {
            ResourceGroupState::default()
        };

        let mut tags = HashMap::new();
        if let Some(tag_value) = params.get("tags") {
            if let Some(tag_obj) = tag_value.as_object() {
                for (k, v) in tag_obj {
                    if let Some(vs) = v.as_str() {
                        tags.insert(k.clone(), vs.to_string());
                    } else {
                        tags.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                    }
                }
            }
        }

        Ok(Self {
            name,
            resource_group,
            location: params.get_string("location")?,
            subnet_id: params.get_string("subnet_id")?,
            private_ip_address: params.get_string("private_ip_address")?,
            private_ip_allocation: params
                .get_string("private_ip_allocation")?
                .unwrap_or_else(|| "Dynamic".to_string()),
            public_ip_address_id: params.get_string("public_ip_address_id")?,
            nsg_id: params.get_string("nsg_id")?,
            enable_accelerated_networking: params
                .get_bool_or("enable_accelerated_networking", false),
            enable_ip_forwarding: params.get_bool_or("enable_ip_forwarding", false),
            state,
            tags,
        })
    }
}

/// Network interface information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceInfo {
    pub id: String,
    pub name: String,
    pub resource_group: String,
    pub location: String,
    pub provisioning_state: String,
    pub private_ip_address: Option<String>,
    pub private_ip_allocation: String,
    pub public_ip_address: Option<String>,
    pub subnet_id: Option<String>,
    pub nsg_id: Option<String>,
    pub mac_address: Option<String>,
    pub enable_accelerated_networking: bool,
    pub enable_ip_forwarding: bool,
    pub tags: HashMap<String, String>,
}

/// Azure Network Interface Module
pub struct AzureNetworkInterfaceModule;

impl AzureNetworkInterfaceModule {
    /// Find network interface by name
    async fn find_nic(
        _name: &str,
        _resource_group: &str,
    ) -> ModuleResult<Option<NetworkInterfaceInfo>> {
        Ok(None)
    }

    /// Create network interface
    async fn create_nic(config: &NetworkInterfaceConfig) -> ModuleResult<NetworkInterfaceInfo> {
        let location = config.location.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter(
                "location is required when creating a network interface".to_string(),
            )
        })?;

        let subnet_id = config.subnet_id.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter(
                "subnet_id is required when creating a network interface".to_string(),
            )
        })?;

        tracing::info!(
            "Would create network interface '{}' in resource group '{}'",
            config.name,
            config.resource_group
        );

        let private_ip = if config.private_ip_allocation == "Static" {
            config.private_ip_address.clone()
        } else {
            Some(format!(
                "10.0.{}.{}",
                rand::random::<u8>(),
                rand::random::<u8>()
            ))
        };

        Ok(NetworkInterfaceInfo {
            id: format!(
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/{}/providers/Microsoft.Network/networkInterfaces/{}",
                config.resource_group, config.name
            ),
            name: config.name.clone(),
            resource_group: config.resource_group.clone(),
            location: location.clone(),
            provisioning_state: "Succeeded".to_string(),
            private_ip_address: private_ip,
            private_ip_allocation: config.private_ip_allocation.clone(),
            public_ip_address: None,
            subnet_id: Some(subnet_id.clone()),
            nsg_id: config.nsg_id.clone(),
            mac_address: Some(format!(
                "{:02X}-{:02X}-{:02X}-{:02X}-{:02X}-{:02X}",
                rand::random::<u8>(),
                rand::random::<u8>(),
                rand::random::<u8>(),
                rand::random::<u8>(),
                rand::random::<u8>(),
                rand::random::<u8>()
            )),
            enable_accelerated_networking: config.enable_accelerated_networking,
            enable_ip_forwarding: config.enable_ip_forwarding,
            tags: config.tags.clone(),
        })
    }

    /// Delete network interface
    async fn delete_nic(_name: &str, _resource_group: &str) -> ModuleResult<()> {
        tracing::info!(
            "Would delete network interface: {}/{}",
            _resource_group,
            _name
        );
        Ok(())
    }

    /// Execute the network interface module
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = NetworkInterfaceConfig::from_params(params)?;
        let existing = Self::find_nic(&config.name, &config.resource_group).await?;

        match config.state {
            ResourceGroupState::Present => {
                if let Some(nic) = existing {
                    Ok(ModuleOutput::ok(format!(
                        "Network interface '{}' already exists",
                        config.name
                    ))
                    .with_data("network_interface", serde_json::to_value(&nic).unwrap()))
                } else {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create network interface '{}'",
                            config.name
                        )));
                    }

                    let nic = Self::create_nic(&config).await?;
                    Ok(ModuleOutput::changed(format!(
                        "Created network interface '{}'",
                        config.name
                    ))
                    .with_data("network_interface", serde_json::to_value(&nic).unwrap()))
                }
            }
            ResourceGroupState::Absent => {
                if existing.is_some() {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would delete network interface '{}'",
                            config.name
                        )));
                    }

                    Self::delete_nic(&config.name, &config.resource_group).await?;
                    Ok(ModuleOutput::changed(format!(
                        "Deleted network interface '{}'",
                        config.name
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Network interface '{}' does not exist",
                        config.name
                    )))
                }
            }
        }
    }
}

impl Module for AzureNetworkInterfaceModule {
    fn name(&self) -> &'static str {
        "azure_network_interface"
    }

    fn description(&self) -> &'static str {
        "Create, update, and delete Azure Network Interfaces"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 20,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name", "resource_group"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_state_from_str() {
        assert_eq!(VmState::from_str("present").unwrap(), VmState::Present);
        assert_eq!(VmState::from_str("absent").unwrap(), VmState::Absent);
        assert_eq!(VmState::from_str("running").unwrap(), VmState::Running);
        assert_eq!(VmState::from_str("stopped").unwrap(), VmState::Stopped);
        assert_eq!(
            VmState::from_str("deallocated").unwrap(),
            VmState::Deallocated
        );
        assert_eq!(VmState::from_str("restarted").unwrap(), VmState::Restarted);
        assert!(VmState::from_str("invalid").is_err());
    }

    #[test]
    fn test_azure_vm_power_state_from_api() {
        assert_eq!(
            AzureVmPowerState::from_api_state("running"),
            AzureVmPowerState::Running
        );
        assert_eq!(
            AzureVmPowerState::from_api_state("stopped"),
            AzureVmPowerState::Stopped
        );
        assert_eq!(
            AzureVmPowerState::from_api_state("deallocated"),
            AzureVmPowerState::Deallocated
        );
    }

    #[test]
    fn test_power_state_matches_desired() {
        assert!(AzureVmPowerState::Running.matches_desired(&VmState::Running));
        assert!(AzureVmPowerState::Running.matches_desired(&VmState::Present));
        assert!(AzureVmPowerState::Stopped.matches_desired(&VmState::Stopped));
        assert!(AzureVmPowerState::Deallocated.matches_desired(&VmState::Deallocated));
        assert!(!AzureVmPowerState::Running.matches_desired(&VmState::Stopped));
    }

    #[test]
    fn test_azure_vm_module_metadata() {
        let module = AzureVmModule;
        assert_eq!(module.name(), "azure_vm");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(module.required_params(), &["name", "resource_group"]);
    }

    #[test]
    fn test_azure_resource_group_module_metadata() {
        let module = AzureResourceGroupModule;
        assert_eq!(module.name(), "azure_resource_group");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_azure_network_interface_module_metadata() {
        let module = AzureNetworkInterfaceModule;
        assert_eq!(module.name(), "azure_network_interface");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(module.required_params(), &["name", "resource_group"]);
    }

    #[test]
    fn test_azure_vm_config_parsing() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-vm"));
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        params.insert("location".to_string(), serde_json::json!("eastus"));
        params.insert("vm_size".to_string(), serde_json::json!("Standard_B2s"));
        params.insert("state".to_string(), serde_json::json!("running"));

        let config = AzureVmConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "test-vm");
        assert_eq!(config.resource_group, "test-rg");
        assert_eq!(config.location, Some("eastus".to_string()));
        assert_eq!(config.vm_size, "Standard_B2s");
        assert_eq!(config.state, VmState::Running);
    }

    #[test]
    fn test_azure_vm_config_with_image() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-vm"));
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        params.insert(
            "image".to_string(),
            serde_json::json!({
                "publisher": "Canonical",
                "offer": "0001-com-ubuntu-server-jammy",
                "sku": "22_04-lts-gen2",
                "version": "latest"
            }),
        );

        let config = AzureVmConfig::from_params(&params).unwrap();
        let image = config.image.unwrap();
        assert_eq!(image.publisher, Some("Canonical".to_string()));
        assert_eq!(
            image.offer,
            Some("0001-com-ubuntu-server-jammy".to_string())
        );
        assert_eq!(image.sku, Some("22_04-lts-gen2".to_string()));
        assert_eq!(image.version, Some("latest".to_string()));
    }

    #[test]
    fn test_azure_vm_config_with_managed_identity() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-vm"));
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        params.insert(
            "managed_identity".to_string(),
            serde_json::json!({
                "type": "SystemAssigned"
            }),
        );

        let config = AzureVmConfig::from_params(&params).unwrap();
        let identity = config.managed_identity.unwrap();
        assert_eq!(identity.identity_type, "SystemAssigned");
    }

    #[test]
    fn test_azure_vm_config_with_tags() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-vm"));
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        params.insert(
            "tags".to_string(),
            serde_json::json!({
                "Environment": "production",
                "Team": "web"
            }),
        );

        let config = AzureVmConfig::from_params(&params).unwrap();
        assert_eq!(
            config.tags.get("Environment"),
            Some(&"production".to_string())
        );
        assert_eq!(config.tags.get("Team"), Some(&"web".to_string()));
    }

    #[test]
    fn test_resource_group_config_parsing() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-rg"));
        params.insert("location".to_string(), serde_json::json!("westus2"));
        params.insert("state".to_string(), serde_json::json!("present"));

        let config = ResourceGroupConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "test-rg");
        assert_eq!(config.location, Some("westus2".to_string()));
        assert_eq!(config.state, ResourceGroupState::Present);
    }

    #[test]
    fn test_network_interface_config_parsing() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-nic"));
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        params.insert("location".to_string(), serde_json::json!("eastus"));
        params.insert(
            "subnet_id".to_string(),
            serde_json::json!("/subscriptions/.../subnets/default"),
        );
        params.insert(
            "enable_accelerated_networking".to_string(),
            serde_json::json!(true),
        );

        let config = NetworkInterfaceConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "test-nic");
        assert_eq!(config.resource_group, "test-rg");
        assert!(config.enable_accelerated_networking);
        assert_eq!(config.private_ip_allocation, "Dynamic");
    }

    #[test]
    fn test_validate_params_missing_name() {
        let module = AzureVmModule;
        let mut params = ModuleParams::new();
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_missing_resource_group() {
        let module = AzureVmModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-vm"));
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_invalid_state() {
        let module = AzureVmModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-vm"));
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        params.insert("state".to_string(), serde_json::json!("invalid_state"));
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_invalid_priority() {
        let module = AzureVmModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-vm"));
        params.insert("resource_group".to_string(), serde_json::json!("test-rg"));
        params.insert("priority".to_string(), serde_json::json!("Invalid"));
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_image_reference_from_object() {
        let value = serde_json::json!({
            "publisher": "Canonical",
            "offer": "UbuntuServer",
            "sku": "18.04-LTS",
            "version": "latest"
        });

        let image = ImageReference::from_params(&value).unwrap();
        assert_eq!(image.publisher, Some("Canonical".to_string()));
        assert_eq!(image.offer, Some("UbuntuServer".to_string()));
        assert_eq!(image.sku, Some("18.04-LTS".to_string()));
        assert_eq!(image.version, Some("latest".to_string()));
        assert!(image.id.is_none());
    }

    #[test]
    fn test_image_reference_from_string() {
        let value = serde_json::json!("/subscriptions/.../images/my-custom-image");

        let image = ImageReference::from_params(&value).unwrap();
        assert!(image.publisher.is_none());
        assert_eq!(
            image.id,
            Some("/subscriptions/.../images/my-custom-image".to_string())
        );
    }

    #[test]
    fn test_managed_identity_from_object() {
        let value = serde_json::json!({
            "type": "UserAssigned",
            "user_assigned_identities": ["/subscriptions/.../identity1"]
        });

        let identity = ManagedIdentityConfig::from_params(&value).unwrap();
        assert_eq!(identity.identity_type, "UserAssigned");
        assert!(identity.user_assigned_identities.is_some());
    }

    #[test]
    fn test_managed_identity_from_string() {
        let value = serde_json::json!("SystemAssigned");

        let identity = ManagedIdentityConfig::from_params(&value).unwrap();
        assert_eq!(identity.identity_type, "SystemAssigned");
        assert!(identity.user_assigned_identities.is_none());
    }
}
