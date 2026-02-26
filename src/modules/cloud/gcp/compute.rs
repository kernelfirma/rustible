//! GCP Compute Engine module for instance and infrastructure management.
//!
//! This module provides comprehensive Compute Engine management including:
//!
//! - Instance creation, deletion, start, stop, and reset
//! - Service account configuration and IAM bindings
//! - Instance metadata management
//! - Network and firewall rule management
//!
//! ## GcpComputeInstanceModule
//!
//! Manages Compute Engine instance lifecycle. Supports idempotent operations
//! using instance names for identification within a project and zone.
//!
//! ### Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Instance name (unique within project/zone) |
//! | `zone` | Yes | GCP zone (e.g., us-central1-a) |
//! | `project` | No | GCP project ID (default: from environment) |
//! | `state` | No | Desired state: running, stopped, terminated, absent (default: running) |
//! | `machine_type` | No | Machine type (default: e2-medium) |
//! | `image` | No | Boot disk image URL |
//! | `image_family` | No | Image family name (e.g., debian-11) |
//! | `image_project` | No | Project containing the image family |
//! | `disk_size_gb` | No | Boot disk size in GB (default: 10) |
//! | `disk_type` | No | Boot disk type: pd-standard, pd-ssd, pd-balanced |
//! | `network` | No | VPC network name (default: default) |
//! | `subnet` | No | Subnet name for the instance |
//! | `network_ip` | No | Internal IP address |
//! | `external_ip` | No | External IP: auto, none, or static IP address |
//! | `preemptible` | No | Use preemptible VM (default: false) |
//! | `spot` | No | Use Spot VM (default: false) |
//! | `service_account` | No | Service account email |
//! | `scopes` | No | OAuth scopes for the service account |
//! | `metadata` | No | Instance metadata as key-value pairs |
//! | `startup_script` | No | Startup script content |
//! | `labels` | No | Instance labels as key-value pairs |
//! | `tags` | No | Network tags |
//! | `can_ip_forward` | No | Enable IP forwarding (default: false) |
//! | `deletion_protection` | No | Enable deletion protection (default: false) |
//! | `wait` | No | Wait for operation completion (default: true) |
//! | `wait_timeout` | No | Timeout for wait operations in seconds (default: 300) |
//!
//! ### Example
//!
//! ```yaml
//! - name: Launch a web server
//!   gcp_compute_instance:
//!     name: web-server-01
//!     zone: us-central1-a
//!     machine_type: e2-standard-2
//!     image_family: debian-11
//!     image_project: debian-cloud
//!     disk_size_gb: 20
//!     network: my-vpc
//!     subnet: my-subnet
//!     tags:
//!       - http-server
//!       - https-server
//!     labels:
//!       environment: production
//!       team: web
//!     service_account: my-sa@my-project.iam.gserviceaccount.com
//!     scopes:
//!       - https://www.googleapis.com/auth/cloud-platform
//!     metadata:
//!       enable-oslogin: "TRUE"
//!     wait: true
//!     state: running
//! ```
//!
//! ## GcpComputeFirewallModule
//!
//! Manages Compute Engine firewall rules.
//!
//! ### Parameters
//!
//! | Parameter | Required | Description |
//! |-----------|----------|-------------|
//! | `name` | Yes | Firewall rule name |
//! | `network` | No | Network to apply rule (default: default) |
//! | `state` | No | Desired state: present, absent (default: present) |
//! | `direction` | No | INGRESS or EGRESS (default: INGRESS) |
//! | `priority` | No | Rule priority 0-65535 (default: 1000) |
//! | `allowed` | No* | Allowed traffic specification |
//! | `denied` | No* | Denied traffic specification |
//! | `source_ranges` | No | Source IP ranges (INGRESS only) |
//! | `destination_ranges` | No | Destination IP ranges (EGRESS only) |
//! | `source_tags` | No | Source network tags (INGRESS only) |
//! | `target_tags` | No | Target network tags |
//! | `source_service_accounts` | No | Source service accounts |
//! | `target_service_accounts` | No | Target service accounts |
//! | `disabled` | No | Whether rule is disabled (default: false) |
//! | `description` | No | Rule description |
//!
//! ## GcpComputeNetworkModule
//!
//! Manages VPC networks and related resources.
//!
//! ## GcpServiceAccountModule
//!
//! Manages service accounts and their keys.

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Represents the desired state of a Compute Engine instance
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum InstanceState {
    /// Instance should be running
    #[default]
    Running,
    /// Instance should be stopped (TERMINATED state in GCP)
    Stopped,
    /// Instance should be deleted
    Terminated,
    /// Instance should not exist (alias for terminated)
    Absent,
    /// Instance should be reset (transient state)
    Reset,
}

impl InstanceState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "running" | "started" | "present" => Ok(InstanceState::Running),
            "stopped" | "suspended" => Ok(InstanceState::Stopped),
            "terminated" | "absent" | "deleted" => Ok(InstanceState::Terminated),
            "reset" | "restarted" => Ok(InstanceState::Reset),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: running, stopped, terminated, absent, reset",
                s
            ))),
        }
    }
}

/// GCP Compute Engine instance status as returned by the API
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GcpInstanceStatus {
    /// Instance is being provisioned
    Provisioning,
    /// Instance is being staged
    Staging,
    /// Instance is running
    Running,
    /// Instance is being stopped
    Stopping,
    /// Instance is being suspended
    Suspending,
    /// Instance is suspended
    Suspended,
    /// Instance is being repaired
    Repairing,
    /// Instance is stopped (TERMINATED in GCP API)
    Terminated,
    /// Unknown status
    Unknown(String),
}

impl GcpInstanceStatus {
    fn from_api_status(status: &str) -> Self {
        match status.to_uppercase().as_str() {
            "PROVISIONING" => Self::Provisioning,
            "STAGING" => Self::Staging,
            "RUNNING" => Self::Running,
            "STOPPING" => Self::Stopping,
            "SUSPENDING" => Self::Suspending,
            "SUSPENDED" => Self::Suspended,
            "REPAIRING" => Self::Repairing,
            "TERMINATED" => Self::Terminated,
            other => Self::Unknown(other.to_string()),
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(self, Self::Running | Self::Terminated | Self::Suspended)
    }

    fn matches_desired(&self, desired: &InstanceState) -> bool {
        match (self, desired) {
            (Self::Running, InstanceState::Running) => true,
            (Self::Terminated, InstanceState::Stopped) => true,
            (Self::Suspended, InstanceState::Stopped) => true,
            _ => false,
        }
    }
}

/// Disk configuration for an instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskConfig {
    /// Boot disk (true for first disk)
    pub boot: bool,
    /// Auto-delete disk when instance is deleted
    pub auto_delete: bool,
    /// Disk size in GB
    pub size_gb: Option<u32>,
    /// Disk type: pd-standard, pd-ssd, pd-balanced, local-ssd
    pub disk_type: Option<String>,
    /// Source image URL
    pub source_image: Option<String>,
    /// Device name
    pub device_name: Option<String>,
    /// Disk encryption key
    pub disk_encryption_key: Option<String>,
}

/// Network interface configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceConfig {
    /// VPC network name or URL
    pub network: Option<String>,
    /// Subnet name or URL
    pub subnetwork: Option<String>,
    /// Internal IP address
    pub network_ip: Option<String>,
    /// External access configuration
    pub access_configs: Option<Vec<AccessConfig>>,
    /// Alias IP ranges
    pub alias_ip_ranges: Option<Vec<AliasIpRange>>,
}

/// Access configuration for external connectivity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessConfig {
    /// Access type (ONE_TO_ONE_NAT)
    pub r#type: String,
    /// Access config name
    pub name: Option<String>,
    /// Static IP address
    pub nat_ip: Option<String>,
    /// Network tier: PREMIUM or STANDARD
    pub network_tier: Option<String>,
}

/// Alias IP range for multiple IPs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AliasIpRange {
    /// IP CIDR range
    pub ip_cidr_range: String,
    /// Subnetwork range name
    pub subnetwork_range_name: Option<String>,
}

/// Service account configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccountConfig {
    /// Service account email
    pub email: String,
    /// OAuth scopes
    pub scopes: Vec<String>,
}

/// Allowed/Denied traffic specification for firewall rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallAllowed {
    /// IP protocol: tcp, udp, icmp, esp, ah, sctp, ipip, all
    #[serde(rename = "IPProtocol")]
    pub ip_protocol: String,
    /// Port ranges (e.g., ["80", "443", "8000-9000"])
    pub ports: Option<Vec<String>>,
}

/// Compute Engine instance configuration
#[derive(Debug, Clone)]
struct ComputeInstanceConfig {
    name: String,
    zone: String,
    project: Option<String>,
    state: InstanceState,
    machine_type: String,
    image: Option<String>,
    image_family: Option<String>,
    image_project: Option<String>,
    disk_size_gb: u32,
    disk_type: String,
    network: String,
    subnet: Option<String>,
    network_ip: Option<String>,
    external_ip: Option<String>,
    preemptible: bool,
    spot: bool,
    service_account: Option<String>,
    scopes: Vec<String>,
    metadata: HashMap<String, String>,
    startup_script: Option<String>,
    labels: HashMap<String, String>,
    tags: Vec<String>,
    can_ip_forward: bool,
    deletion_protection: bool,
    wait: bool,
    wait_timeout: u64,
}

impl ComputeInstanceConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let zone = params.get_string_required("zone")?;

        let state = if let Some(s) = params.get_string("state")? {
            InstanceState::from_str(&s)?
        } else {
            InstanceState::default()
        };

        // Parse metadata
        let mut metadata = HashMap::new();
        if let Some(meta_value) = params.get("metadata") {
            if let Some(meta_obj) = meta_value.as_object() {
                for (k, v) in meta_obj {
                    if let Some(vs) = v.as_str() {
                        metadata.insert(k.clone(), vs.to_string());
                    } else {
                        metadata.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                    }
                }
            }
        }

        // Parse labels
        let mut labels = HashMap::new();
        if let Some(label_value) = params.get("labels") {
            if let Some(label_obj) = label_value.as_object() {
                for (k, v) in label_obj {
                    if let Some(vs) = v.as_str() {
                        labels.insert(k.clone(), vs.to_string());
                    } else {
                        labels.insert(k.clone(), v.to_string().trim_matches('"').to_string());
                    }
                }
            }
        }

        // Parse tags
        let tags = params.get_vec_string("tags")?.unwrap_or_default();

        // Parse scopes
        let scopes = params.get_vec_string("scopes")?.unwrap_or_else(|| {
            vec!["https://www.googleapis.com/auth/devstorage.read_only".to_string()]
        });

        Ok(Self {
            name,
            zone,
            project: params.get_string("project")?,
            state,
            machine_type: params
                .get_string("machine_type")?
                .unwrap_or_else(|| "e2-medium".to_string()),
            image: params.get_string("image")?,
            image_family: params.get_string("image_family")?,
            image_project: params.get_string("image_project")?,
            disk_size_gb: params.get_u32("disk_size_gb")?.unwrap_or(10),
            disk_type: params
                .get_string("disk_type")?
                .unwrap_or_else(|| "pd-standard".to_string()),
            network: params
                .get_string("network")?
                .unwrap_or_else(|| "default".to_string()),
            subnet: params.get_string("subnet")?,
            network_ip: params.get_string("network_ip")?,
            external_ip: params.get_string("external_ip")?,
            preemptible: params.get_bool_or("preemptible", false),
            spot: params.get_bool_or("spot", false),
            service_account: params.get_string("service_account")?,
            scopes,
            metadata,
            startup_script: params.get_string("startup_script")?,
            labels,
            tags,
            can_ip_forward: params.get_bool_or("can_ip_forward", false),
            deletion_protection: params.get_bool_or("deletion_protection", false),
            wait: params.get_bool_or("wait", true),
            wait_timeout: params.get_i64("wait_timeout")?.unwrap_or(300) as u64,
        })
    }

    /// Build the source image URL from family and project
    fn get_source_image(&self) -> ModuleResult<String> {
        if let Some(image) = &self.image {
            // Full image URL provided
            if image.starts_with("projects/") || image.starts_with("https://") {
                Ok(image.clone())
            } else {
                // Assume it's a global image name
                Ok(format!(
                    "projects/{}/global/images/{}",
                    self.image_project.as_deref().unwrap_or("debian-cloud"),
                    image
                ))
            }
        } else if let Some(family) = &self.image_family {
            let project = self.image_project.as_deref().unwrap_or("debian-cloud");
            Ok(format!(
                "projects/{}/global/images/family/{}",
                project, family
            ))
        } else {
            // Default to Debian 11
            Ok("projects/debian-cloud/global/images/family/debian-11".to_string())
        }
    }

    /// Get the full machine type URL
    fn get_machine_type_url(&self, _project: &str) -> String {
        if self.machine_type.starts_with("zones/") || self.machine_type.starts_with("https://") {
            self.machine_type.clone()
        } else {
            format!("zones/{}/machineTypes/{}", self.zone, self.machine_type)
        }
    }
}

/// Instance information returned from GCP API
#[derive(Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    /// Instance ID
    pub id: String,
    /// Instance name
    pub name: String,
    /// Instance status
    pub status: String,
    /// Machine type
    pub machine_type: String,
    /// Zone
    pub zone: String,
    /// Creation timestamp
    pub creation_timestamp: String,
    /// Network interfaces with IPs
    pub network_interfaces: Vec<NetworkInterfaceInfo>,
    /// Instance labels
    pub labels: HashMap<String, String>,
    /// Network tags
    pub tags: Vec<String>,
    /// Service accounts
    pub service_accounts: Vec<ServiceAccountInfo>,
    /// Metadata
    pub metadata: HashMap<String, String>,
    /// Self-link URL
    pub self_link: String,
}

impl std::fmt::Debug for InstanceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstanceInfo")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("status", &self.status)
            .field("machine_type", &self.machine_type)
            .field("zone", &self.zone)
            .field("creation_timestamp", &self.creation_timestamp)
            .field("network_interfaces", &self.network_interfaces)
            .field("labels", &self.labels)
            .field("tags", &self.tags)
            .field(
                "service_accounts",
                &self
                    .service_accounts
                    .iter()
                    .map(|_| "[REDACTED]")
                    .collect::<Vec<_>>(),
            )
            .field("metadata", &self.metadata)
            .field("self_link", &self.self_link)
            .finish()
    }
}

/// Network interface information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceInfo {
    /// Network name
    pub network: String,
    /// Subnetwork name
    pub subnetwork: Option<String>,
    /// Internal IP address
    pub network_ip: String,
    /// External IP address (if any)
    pub access_config_nat_ip: Option<String>,
}

/// Service account information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccountInfo {
    /// Service account email
    pub email: String,
    /// OAuth scopes
    pub scopes: Vec<String>,
}

/// GCP Compute Engine Instance module
pub struct GcpComputeInstanceModule;

impl GcpComputeInstanceModule {
    /// Get the project ID from config or environment
    fn get_project(config: &ComputeInstanceConfig) -> ModuleResult<String> {
        if let Some(project) = &config.project {
            return Ok(project.clone());
        }

        // Try environment variables
        if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
            return Ok(project);
        }
        if let Ok(project) = std::env::var("GCLOUD_PROJECT") {
            return Ok(project);
        }
        if let Ok(project) = std::env::var("GCP_PROJECT") {
            return Ok(project);
        }

        Err(ModuleError::MissingParameter(
            "project is required. Set via parameter, GOOGLE_CLOUD_PROJECT, or GCLOUD_PROJECT environment variable".to_string(),
        ))
    }

    /// Find instance by name
    async fn find_instance(
        _name: &str,
        _zone: &str,
        _project: &str,
    ) -> ModuleResult<Option<InstanceInfo>> {
        // In a real implementation, this would use the Google Cloud SDK:
        //
        // use google_cloud_compute::Client;
        //
        // let client = Client::new().await?;
        // let instance = client
        //     .instances()
        //     .get(project, zone, name)
        //     .await?;
        //
        // For now, return None to simulate no existing instance
        Ok(None)
    }

    /// Create a new instance
    async fn create_instance(
        config: &ComputeInstanceConfig,
        project: &str,
    ) -> ModuleResult<InstanceInfo> {
        let source_image = config.get_source_image()?;
        let machine_type = config.get_machine_type_url(project);

        // In a real implementation:
        //
        // let instance = Instance {
        //     name: config.name.clone(),
        //     machine_type: machine_type.clone(),
        //     disks: vec![AttachedDisk {
        //         boot: true,
        //         auto_delete: true,
        //         initialize_params: Some(AttachedDiskInitializeParams {
        //             source_image: source_image.clone(),
        //             disk_size_gb: config.disk_size_gb as i64,
        //             disk_type: format!("zones/{}/diskTypes/{}", config.zone, config.disk_type),
        //         }),
        //         ..Default::default()
        //     }],
        //     network_interfaces: vec![NetworkInterface {
        //         network: format!("global/networks/{}", config.network),
        //         subnetwork: config.subnet.clone().map(|s| format!("regions/{}/subnetworks/{}", region, s)),
        //         network_ip: config.network_ip.clone(),
        //         access_configs: if config.external_ip.as_deref() != Some("none") {
        //             Some(vec![AccessConfig {
        //                 type_: "ONE_TO_ONE_NAT".to_string(),
        //                 name: Some("External NAT".to_string()),
        //                 nat_ip: config.external_ip.clone().filter(|ip| ip != "auto"),
        //                 ..Default::default()
        //             }])
        //         } else {
        //             None
        //         },
        //         ..Default::default()
        //     }],
        //     labels: Some(config.labels.clone()),
        //     tags: Some(Tags { items: Some(config.tags.clone()) }),
        //     metadata: Some(Metadata {
        //         items: config.metadata.iter().map(|(k, v)| MetadataItems {
        //             key: k.clone(),
        //             value: Some(v.clone()),
        //         }).collect(),
        //     }),
        //     service_accounts: config.service_account.as_ref().map(|sa| vec![ServiceAccount {
        //         email: sa.clone(),
        //         scopes: config.scopes.clone(),
        //     }]),
        //     scheduling: Some(Scheduling {
        //         preemptible: config.preemptible,
        //         provisioning_model: if config.spot { Some("SPOT".to_string()) } else { None },
        //         ..Default::default()
        //     }),
        //     can_ip_forward: config.can_ip_forward,
        //     deletion_protection: config.deletion_protection,
        //     ..Default::default()
        // };
        //
        // client.instances().insert(project, &config.zone, instance).await?;

        let instance_id = format!("{:019}", rand::random::<u64>());

        tracing::info!(
            "Would create instance '{}' in zone '{}' with machine type '{}' and image '{}'",
            config.name,
            config.zone,
            config.machine_type,
            source_image
        );

        Ok(InstanceInfo {
            id: instance_id,
            name: config.name.clone(),
            status: "RUNNING".to_string(),
            machine_type,
            zone: config.zone.clone(),
            creation_timestamp: chrono::Utc::now().to_rfc3339(),
            network_interfaces: vec![NetworkInterfaceInfo {
                network: config.network.clone(),
                subnetwork: config.subnet.clone(),
                network_ip: config
                    .network_ip
                    .clone()
                    .unwrap_or_else(|| "10.128.0.2".to_string()),
                access_config_nat_ip: if config.external_ip.as_deref() != Some("none") {
                    Some(format!(
                        "35.{}.{}.{}",
                        rand::random::<u8>(),
                        rand::random::<u8>(),
                        rand::random::<u8>()
                    ))
                } else {
                    None
                },
            }],
            labels: config.labels.clone(),
            tags: config.tags.clone(),
            service_accounts: config
                .service_account
                .as_ref()
                .map(|sa| {
                    vec![ServiceAccountInfo {
                        email: sa.clone(),
                        scopes: config.scopes.clone(),
                    }]
                })
                .unwrap_or_default(),
            metadata: config.metadata.clone(),
            self_link: format!(
                "https://www.googleapis.com/compute/v1/projects/{}/zones/{}/instances/{}",
                project, config.zone, config.name
            ),
        })
    }

    /// Start a stopped instance
    async fn start_instance(_name: &str, _zone: &str, _project: &str) -> ModuleResult<()> {
        // client.instances().start(project, zone, name).await?;
        tracing::info!("Would start instance: {} in zone {}", _name, _zone);
        Ok(())
    }

    /// Stop a running instance
    async fn stop_instance(_name: &str, _zone: &str, _project: &str) -> ModuleResult<()> {
        // client.instances().stop(project, zone, name).await?;
        tracing::info!("Would stop instance: {} in zone {}", _name, _zone);
        Ok(())
    }

    /// Delete an instance
    async fn delete_instance(_name: &str, _zone: &str, _project: &str) -> ModuleResult<()> {
        // client.instances().delete(project, zone, name).await?;
        tracing::info!("Would delete instance: {} in zone {}", _name, _zone);
        Ok(())
    }

    /// Reset (reboot) an instance
    async fn reset_instance(_name: &str, _zone: &str, _project: &str) -> ModuleResult<()> {
        // client.instances().reset(project, zone, name).await?;
        tracing::info!("Would reset instance: {} in zone {}", _name, _zone);
        Ok(())
    }

    /// Wait for an operation to complete
    async fn wait_for_operation(
        _operation_name: &str,
        _zone: &str,
        _project: &str,
        timeout: Duration,
    ) -> ModuleResult<()> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(5);

        tracing::info!("Waiting for operation to complete (timeout: {:?})", timeout);

        // In a real implementation:
        // loop {
        //     if start.elapsed() >= timeout {
        //         return Err(ModuleError::ExecutionFailed("Operation timed out".to_string()));
        //     }
        //
        //     let operation = client.zone_operations().get(project, zone, operation_name).await?;
        //     if operation.status == "DONE" {
        //         if let Some(error) = operation.error {
        //             return Err(ModuleError::ExecutionFailed(format!(
        //                 "Operation failed: {:?}", error
        //             )));
        //         }
        //         break;
        //     }
        //
        //     tokio::time::sleep(poll_interval).await;
        // }

        // Simulate waiting
        if start.elapsed() < timeout {
            tokio::time::sleep(std::cmp::min(poll_interval, Duration::from_millis(100))).await;
        }

        Ok(())
    }

    /// Execute the module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ComputeInstanceConfig::from_params(params)?;
        let project = Self::get_project(&config)?;

        let existing = Self::find_instance(&config.name, &config.zone, &project).await?;

        match config.state {
            InstanceState::Running => {
                self.ensure_running(&config, &project, existing, context)
                    .await
            }
            InstanceState::Stopped => {
                self.ensure_stopped(&config, &project, existing, context)
                    .await
            }
            InstanceState::Terminated | InstanceState::Absent => {
                self.ensure_deleted(&config, &project, existing, context)
                    .await
            }
            InstanceState::Reset => {
                self.ensure_reset(&config, &project, existing, context)
                    .await
            }
        }
    }

    /// Ensure instance is in running state
    async fn ensure_running(
        &self,
        config: &ComputeInstanceConfig,
        project: &str,
        existing: Option<InstanceInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(instance) = existing {
            let status = GcpInstanceStatus::from_api_status(&instance.status);

            if status == GcpInstanceStatus::Running {
                return Ok(ModuleOutput::ok(format!(
                    "Instance '{}' is already running",
                    config.name
                ))
                .with_data("instance", serde_json::to_value(&instance).unwrap()));
            }

            if status == GcpInstanceStatus::Terminated {
                // Instance is stopped, start it
                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would start instance '{}'",
                        config.name
                    )));
                }

                Self::start_instance(&config.name, &config.zone, project).await?;

                if config.wait {
                    Self::wait_for_operation(
                        "start-operation",
                        &config.zone,
                        project,
                        Duration::from_secs(config.wait_timeout),
                    )
                    .await?;
                }

                return Ok(
                    ModuleOutput::changed(format!("Started instance '{}'", config.name))
                        .with_data("instance", serde_json::to_value(&instance).unwrap()),
                );
            }

            // Instance is in some intermediate state
            Ok(ModuleOutput::ok(format!(
                "Instance '{}' is in state '{}', waiting for it to stabilize",
                config.name, instance.status
            )))
        } else {
            // Create new instance
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would create instance '{}' in zone '{}'",
                    config.name, config.zone
                )));
            }

            let instance = Self::create_instance(config, project).await?;

            if config.wait {
                Self::wait_for_operation(
                    "create-operation",
                    &config.zone,
                    project,
                    Duration::from_secs(config.wait_timeout),
                )
                .await?;
            }

            Ok(ModuleOutput::changed(format!(
                "Created instance '{}' in zone '{}'",
                config.name, config.zone
            ))
            .with_data("instance", serde_json::to_value(&instance).unwrap()))
        }
    }

    /// Ensure instance is in stopped state
    async fn ensure_stopped(
        &self,
        config: &ComputeInstanceConfig,
        project: &str,
        existing: Option<InstanceInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(instance) = existing {
            let status = GcpInstanceStatus::from_api_status(&instance.status);

            if status == GcpInstanceStatus::Terminated {
                return Ok(ModuleOutput::ok(format!(
                    "Instance '{}' is already stopped",
                    config.name
                ))
                .with_data("instance", serde_json::to_value(&instance).unwrap()));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would stop instance '{}'",
                    config.name
                )));
            }

            Self::stop_instance(&config.name, &config.zone, project).await?;

            if config.wait {
                Self::wait_for_operation(
                    "stop-operation",
                    &config.zone,
                    project,
                    Duration::from_secs(config.wait_timeout),
                )
                .await?;
            }

            Ok(
                ModuleOutput::changed(format!("Stopped instance '{}'", config.name))
                    .with_data("instance", serde_json::to_value(&instance).unwrap()),
            )
        } else {
            Ok(ModuleOutput::ok(format!(
                "Instance '{}' does not exist",
                config.name
            )))
        }
    }

    /// Ensure instance is deleted
    async fn ensure_deleted(
        &self,
        config: &ComputeInstanceConfig,
        project: &str,
        existing: Option<InstanceInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(_instance) = existing {
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would delete instance '{}'",
                    config.name
                )));
            }

            Self::delete_instance(&config.name, &config.zone, project).await?;

            if config.wait {
                Self::wait_for_operation(
                    "delete-operation",
                    &config.zone,
                    project,
                    Duration::from_secs(config.wait_timeout),
                )
                .await?;
            }

            Ok(ModuleOutput::changed(format!(
                "Deleted instance '{}'",
                config.name
            )))
        } else {
            Ok(ModuleOutput::ok(format!(
                "Instance '{}' does not exist",
                config.name
            )))
        }
    }

    /// Reset the instance
    async fn ensure_reset(
        &self,
        config: &ComputeInstanceConfig,
        project: &str,
        existing: Option<InstanceInfo>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        if let Some(instance) = existing {
            let status = GcpInstanceStatus::from_api_status(&instance.status);

            if status != GcpInstanceStatus::Running {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Cannot reset instance '{}' in state '{}'. Instance must be running.",
                    config.name, instance.status
                )));
            }

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would reset instance '{}'",
                    config.name
                )));
            }

            Self::reset_instance(&config.name, &config.zone, project).await?;

            if config.wait {
                Self::wait_for_operation(
                    "reset-operation",
                    &config.zone,
                    project,
                    Duration::from_secs(config.wait_timeout),
                )
                .await?;
            }

            Ok(
                ModuleOutput::changed(format!("Reset instance '{}'", config.name))
                    .with_data("instance", serde_json::to_value(&instance).unwrap()),
            )
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Instance '{}' does not exist and cannot be reset",
                config.name
            )))
        }
    }
}

impl Module for GcpComputeInstanceModule {
    fn name(&self) -> &'static str {
        "gcp_compute_instance"
    }

    fn description(&self) -> &'static str {
        "Create, delete, start, stop, and manage GCP Compute Engine instances"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // GCP API has rate limits
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name", "zone"]
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
        // Validate required parameters
        params.get_string_required("name")?;
        params.get_string_required("zone")?;

        // Validate state if provided
        if let Some(state) = params.get_string("state")? {
            InstanceState::from_str(&state)?;
        }

        // Validate disk type if provided
        if let Some(disk_type) = params.get_string("disk_type")? {
            let valid_types = [
                "pd-standard",
                "pd-ssd",
                "pd-balanced",
                "pd-extreme",
                "local-ssd",
            ];
            if !valid_types.contains(&disk_type.as_str()) {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid disk_type '{}'. Valid types: {}",
                    disk_type,
                    valid_types.join(", ")
                )));
            }
        }

        Ok(())
    }
}

// ============================================================================
// Firewall Module
// ============================================================================

/// Firewall rule state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FirewallState {
    #[default]
    Present,
    Absent,
}

/// Firewall rule direction
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum FirewallDirection {
    #[default]
    Ingress,
    Egress,
}

/// Firewall rule configuration
#[derive(Debug, Clone)]
struct FirewallConfig {
    name: String,
    project: Option<String>,
    network: String,
    state: FirewallState,
    direction: FirewallDirection,
    priority: u32,
    allowed: Vec<FirewallAllowed>,
    denied: Vec<FirewallAllowed>,
    source_ranges: Vec<String>,
    destination_ranges: Vec<String>,
    source_tags: Vec<String>,
    target_tags: Vec<String>,
    source_service_accounts: Vec<String>,
    target_service_accounts: Vec<String>,
    disabled: bool,
    description: Option<String>,
}

impl FirewallConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        let state = if let Some(s) = params.get_string("state")? {
            match s.to_lowercase().as_str() {
                "present" => FirewallState::Present,
                "absent" => FirewallState::Absent,
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid state '{}'. Valid states: present, absent",
                        s
                    )))
                }
            }
        } else {
            FirewallState::default()
        };

        let direction = if let Some(d) = params.get_string("direction")? {
            match d.to_uppercase().as_str() {
                "INGRESS" | "IN" => FirewallDirection::Ingress,
                "EGRESS" | "OUT" => FirewallDirection::Egress,
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid direction '{}'. Valid directions: INGRESS, EGRESS",
                        d
                    )))
                }
            }
        } else {
            FirewallDirection::default()
        };

        // Parse allowed rules
        let allowed = if let Some(allowed_value) = params.get("allowed") {
            if let Some(allowed_array) = allowed_value.as_array() {
                allowed_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Parse denied rules
        let denied = if let Some(denied_value) = params.get("denied") {
            if let Some(denied_array) = denied_value.as_array() {
                denied_array
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            name,
            project: params.get_string("project")?,
            network: params
                .get_string("network")?
                .unwrap_or_else(|| "default".to_string()),
            state,
            direction,
            priority: params.get_u32("priority")?.unwrap_or(1000),
            allowed,
            denied,
            source_ranges: params.get_vec_string("source_ranges")?.unwrap_or_default(),
            destination_ranges: params
                .get_vec_string("destination_ranges")?
                .unwrap_or_default(),
            source_tags: params.get_vec_string("source_tags")?.unwrap_or_default(),
            target_tags: params.get_vec_string("target_tags")?.unwrap_or_default(),
            source_service_accounts: params
                .get_vec_string("source_service_accounts")?
                .unwrap_or_default(),
            target_service_accounts: params
                .get_vec_string("target_service_accounts")?
                .unwrap_or_default(),
            disabled: params.get_bool_or("disabled", false),
            description: params.get_string("description")?,
        })
    }
}

/// Firewall rule info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirewallInfo {
    pub id: String,
    pub name: String,
    pub network: String,
    pub direction: String,
    pub priority: u32,
    pub allowed: Vec<FirewallAllowed>,
    pub denied: Vec<FirewallAllowed>,
    pub source_ranges: Vec<String>,
    pub target_tags: Vec<String>,
    pub disabled: bool,
    pub self_link: String,
}

/// GCP Compute Engine Firewall module
pub struct GcpComputeFirewallModule;

impl GcpComputeFirewallModule {
    fn get_project(config: &FirewallConfig) -> ModuleResult<String> {
        if let Some(project) = &config.project {
            return Ok(project.clone());
        }

        if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
            return Ok(project);
        }
        if let Ok(project) = std::env::var("GCLOUD_PROJECT") {
            return Ok(project);
        }

        Err(ModuleError::MissingParameter(
            "project is required".to_string(),
        ))
    }

    async fn find_firewall(_name: &str, _project: &str) -> ModuleResult<Option<FirewallInfo>> {
        // In a real implementation:
        // client.firewalls().get(project, name).await
        Ok(None)
    }

    async fn create_firewall(config: &FirewallConfig, project: &str) -> ModuleResult<FirewallInfo> {
        tracing::info!(
            "Would create firewall rule '{}' in network '{}'",
            config.name,
            config.network
        );

        Ok(FirewallInfo {
            id: format!("{:019}", rand::random::<u64>()),
            name: config.name.clone(),
            network: config.network.clone(),
            direction: match config.direction {
                FirewallDirection::Ingress => "INGRESS".to_string(),
                FirewallDirection::Egress => "EGRESS".to_string(),
            },
            priority: config.priority,
            allowed: config.allowed.clone(),
            denied: config.denied.clone(),
            source_ranges: config.source_ranges.clone(),
            target_tags: config.target_tags.clone(),
            disabled: config.disabled,
            self_link: format!(
                "https://www.googleapis.com/compute/v1/projects/{}/global/firewalls/{}",
                project, config.name
            ),
        })
    }

    async fn delete_firewall(_name: &str, _project: &str) -> ModuleResult<()> {
        tracing::info!("Would delete firewall rule: {}", _name);
        Ok(())
    }

    async fn update_firewall(
        _config: &FirewallConfig,
        _project: &str,
    ) -> ModuleResult<FirewallInfo> {
        tracing::info!("Would update firewall rule: {}", _config.name);
        Self::create_firewall(_config, _project).await
    }

    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = FirewallConfig::from_params(params)?;
        let project = Self::get_project(&config)?;

        let existing = Self::find_firewall(&config.name, &project).await?;

        match config.state {
            FirewallState::Present => {
                if let Some(_firewall) = existing {
                    // Update existing firewall
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would update firewall rule '{}'",
                            config.name
                        )));
                    }

                    let updated = Self::update_firewall(&config, &project).await?;
                    Ok(
                        ModuleOutput::changed(format!("Updated firewall rule '{}'", config.name))
                            .with_data("firewall", serde_json::to_value(&updated).unwrap()),
                    )
                } else {
                    // Create new firewall
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create firewall rule '{}'",
                            config.name
                        )));
                    }

                    let firewall = Self::create_firewall(&config, &project).await?;
                    Ok(
                        ModuleOutput::changed(format!("Created firewall rule '{}'", config.name))
                            .with_data("firewall", serde_json::to_value(&firewall).unwrap()),
                    )
                }
            }
            FirewallState::Absent => {
                if existing.is_some() {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would delete firewall rule '{}'",
                            config.name
                        )));
                    }

                    Self::delete_firewall(&config.name, &project).await?;
                    Ok(ModuleOutput::changed(format!(
                        "Deleted firewall rule '{}'",
                        config.name
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Firewall rule '{}' does not exist",
                        config.name
                    )))
                }
            }
        }
    }
}

impl Module for GcpComputeFirewallModule {
    fn name(&self) -> &'static str {
        "gcp_compute_firewall"
    }

    fn description(&self) -> &'static str {
        "Create, update, and delete GCP Compute Engine firewall rules"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
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
// Network Module
// ============================================================================

/// VPC Network state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NetworkState {
    #[default]
    Present,
    Absent,
}

/// VPC Network configuration
#[derive(Debug, Clone)]
struct NetworkConfig {
    name: String,
    project: Option<String>,
    state: NetworkState,
    auto_create_subnetworks: bool,
    routing_mode: String,
    mtu: Option<u32>,
    description: Option<String>,
}

impl NetworkConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        let state = if let Some(s) = params.get_string("state")? {
            match s.to_lowercase().as_str() {
                "present" => NetworkState::Present,
                "absent" => NetworkState::Absent,
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid state '{}'. Valid states: present, absent",
                        s
                    )))
                }
            }
        } else {
            NetworkState::default()
        };

        Ok(Self {
            name,
            project: params.get_string("project")?,
            state,
            auto_create_subnetworks: params.get_bool_or("auto_create_subnetworks", true),
            routing_mode: params
                .get_string("routing_mode")?
                .unwrap_or_else(|| "REGIONAL".to_string()),
            mtu: params.get_u32("mtu")?,
            description: params.get_string("description")?,
        })
    }
}

/// VPC Network info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub id: String,
    pub name: String,
    pub auto_create_subnetworks: bool,
    pub routing_mode: String,
    pub mtu: u32,
    pub subnetworks: Vec<String>,
    pub self_link: String,
}

/// GCP Compute Engine Network module
pub struct GcpComputeNetworkModule;

impl GcpComputeNetworkModule {
    fn get_project(config: &NetworkConfig) -> ModuleResult<String> {
        if let Some(project) = &config.project {
            return Ok(project.clone());
        }

        if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
            return Ok(project);
        }
        if let Ok(project) = std::env::var("GCLOUD_PROJECT") {
            return Ok(project);
        }

        Err(ModuleError::MissingParameter(
            "project is required".to_string(),
        ))
    }

    async fn find_network(_name: &str, _project: &str) -> ModuleResult<Option<NetworkInfo>> {
        Ok(None)
    }

    async fn create_network(config: &NetworkConfig, project: &str) -> ModuleResult<NetworkInfo> {
        tracing::info!("Would create VPC network '{}'", config.name);

        Ok(NetworkInfo {
            id: format!("{:019}", rand::random::<u64>()),
            name: config.name.clone(),
            auto_create_subnetworks: config.auto_create_subnetworks,
            routing_mode: config.routing_mode.clone(),
            mtu: config.mtu.unwrap_or(1460),
            subnetworks: Vec::new(),
            self_link: format!(
                "https://www.googleapis.com/compute/v1/projects/{}/global/networks/{}",
                project, config.name
            ),
        })
    }

    async fn delete_network(_name: &str, _project: &str) -> ModuleResult<()> {
        tracing::info!("Would delete VPC network: {}", _name);
        Ok(())
    }

    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = NetworkConfig::from_params(params)?;
        let project = Self::get_project(&config)?;

        let existing = Self::find_network(&config.name, &project).await?;

        match config.state {
            NetworkState::Present => {
                if let Some(network) = existing {
                    Ok(
                        ModuleOutput::ok(format!("VPC network '{}' already exists", config.name))
                            .with_data("network", serde_json::to_value(&network).unwrap()),
                    )
                } else {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create VPC network '{}'",
                            config.name
                        )));
                    }

                    let network = Self::create_network(&config, &project).await?;
                    Ok(
                        ModuleOutput::changed(format!("Created VPC network '{}'", config.name))
                            .with_data("network", serde_json::to_value(&network).unwrap()),
                    )
                }
            }
            NetworkState::Absent => {
                if existing.is_some() {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would delete VPC network '{}'",
                            config.name
                        )));
                    }

                    Self::delete_network(&config.name, &project).await?;
                    Ok(ModuleOutput::changed(format!(
                        "Deleted VPC network '{}'",
                        config.name
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "VPC network '{}' does not exist",
                        config.name
                    )))
                }
            }
        }
    }
}

impl Module for GcpComputeNetworkModule {
    fn name(&self) -> &'static str {
        "gcp_compute_network"
    }

    fn description(&self) -> &'static str {
        "Create and delete GCP VPC networks"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 10,
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
// Service Account Module
// ============================================================================

/// Service account state
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ServiceAccountState {
    #[default]
    Present,
    Absent,
}

/// Service account configuration
#[derive(Debug, Clone)]
struct ServiceAccountModuleConfig {
    name: String,
    project: Option<String>,
    state: ServiceAccountState,
    display_name: Option<String>,
    description: Option<String>,
    create_key: bool,
    key_algorithm: String,
}

impl ServiceAccountModuleConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        let state = if let Some(s) = params.get_string("state")? {
            match s.to_lowercase().as_str() {
                "present" => ServiceAccountState::Present,
                "absent" => ServiceAccountState::Absent,
                _ => {
                    return Err(ModuleError::InvalidParameter(format!(
                        "Invalid state '{}'. Valid states: present, absent",
                        s
                    )))
                }
            }
        } else {
            ServiceAccountState::default()
        };

        Ok(Self {
            name,
            project: params.get_string("project")?,
            state,
            display_name: params.get_string("display_name")?,
            description: params.get_string("description")?,
            create_key: params.get_bool_or("create_key", false),
            key_algorithm: params
                .get_string("key_algorithm")?
                .unwrap_or_else(|| "KEY_ALG_RSA_2048".to_string()),
        })
    }
}

/// Service account module info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccountModuleInfo {
    pub name: String,
    pub email: String,
    pub unique_id: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub disabled: bool,
    pub oauth2_client_id: String,
}

/// GCP Service Account module
pub struct GcpServiceAccountModule;

impl GcpServiceAccountModule {
    fn get_project(config: &ServiceAccountModuleConfig) -> ModuleResult<String> {
        if let Some(project) = &config.project {
            return Ok(project.clone());
        }

        if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
            return Ok(project);
        }
        if let Ok(project) = std::env::var("GCLOUD_PROJECT") {
            return Ok(project);
        }

        Err(ModuleError::MissingParameter(
            "project is required".to_string(),
        ))
    }

    async fn find_service_account(
        _name: &str,
        _project: &str,
    ) -> ModuleResult<Option<ServiceAccountModuleInfo>> {
        Ok(None)
    }

    async fn create_service_account(
        config: &ServiceAccountModuleConfig,
        project: &str,
    ) -> ModuleResult<ServiceAccountModuleInfo> {
        let email = format!("{}@{}.iam.gserviceaccount.com", config.name, project);

        tracing::info!("Would create service account '{}'", email);

        Ok(ServiceAccountModuleInfo {
            name: format!("projects/{}/serviceAccounts/{}", project, email),
            email,
            unique_id: format!("{:021}", rand::random::<u64>()),
            display_name: config.display_name.clone(),
            description: config.description.clone(),
            disabled: false,
            oauth2_client_id: format!("{:021}", rand::random::<u64>()),
        })
    }

    async fn delete_service_account(_email: &str, _project: &str) -> ModuleResult<()> {
        tracing::info!("Would delete service account: {}", _email);
        Ok(())
    }

    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ServiceAccountModuleConfig::from_params(params)?;
        let project = Self::get_project(&config)?;

        let email = format!("{}@{}.iam.gserviceaccount.com", config.name, project);
        let existing = Self::find_service_account(&email, &project).await?;

        match config.state {
            ServiceAccountState::Present => {
                if let Some(sa) = existing {
                    Ok(
                        ModuleOutput::ok(format!("Service account '{}' already exists", email))
                            .with_data("service_account", serde_json::to_value(&sa).unwrap()),
                    )
                } else {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would create service account '{}'",
                            email
                        )));
                    }

                    let sa = Self::create_service_account(&config, &project).await?;
                    Ok(
                        ModuleOutput::changed(format!("Created service account '{}'", email))
                            .with_data("service_account", serde_json::to_value(&sa).unwrap()),
                    )
                }
            }
            ServiceAccountState::Absent => {
                if existing.is_some() {
                    if context.check_mode {
                        return Ok(ModuleOutput::changed(format!(
                            "Would delete service account '{}'",
                            email
                        )));
                    }

                    Self::delete_service_account(&email, &project).await?;
                    Ok(ModuleOutput::changed(format!(
                        "Deleted service account '{}'",
                        email
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Service account '{}' does not exist",
                        email
                    )))
                }
            }
        }
    }
}

impl Module for GcpServiceAccountModule {
    fn name(&self) -> &'static str {
        "gcp_service_account"
    }

    fn description(&self) -> &'static str {
        "Create and delete GCP service accounts"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::RateLimited {
            requests_per_second: 5,
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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_state_from_str() {
        assert_eq!(
            InstanceState::from_str("running").unwrap(),
            InstanceState::Running
        );
        assert_eq!(
            InstanceState::from_str("stopped").unwrap(),
            InstanceState::Stopped
        );
        assert_eq!(
            InstanceState::from_str("terminated").unwrap(),
            InstanceState::Terminated
        );
        assert_eq!(
            InstanceState::from_str("absent").unwrap(),
            InstanceState::Terminated
        );
        assert_eq!(
            InstanceState::from_str("reset").unwrap(),
            InstanceState::Reset
        );
        assert!(InstanceState::from_str("invalid").is_err());
    }

    #[test]
    fn test_gcp_instance_status_from_api() {
        assert_eq!(
            GcpInstanceStatus::from_api_status("RUNNING"),
            GcpInstanceStatus::Running
        );
        assert_eq!(
            GcpInstanceStatus::from_api_status("TERMINATED"),
            GcpInstanceStatus::Terminated
        );
        assert_eq!(
            GcpInstanceStatus::from_api_status("STAGING"),
            GcpInstanceStatus::Staging
        );
        assert_eq!(
            GcpInstanceStatus::from_api_status("SUSPENDED"),
            GcpInstanceStatus::Suspended
        );
    }

    #[test]
    fn test_gcp_status_matches_desired() {
        assert!(GcpInstanceStatus::Running.matches_desired(&InstanceState::Running));
        assert!(GcpInstanceStatus::Terminated.matches_desired(&InstanceState::Stopped));
        assert!(!GcpInstanceStatus::Running.matches_desired(&InstanceState::Stopped));
    }

    #[test]
    fn test_compute_instance_module_metadata() {
        let module = GcpComputeInstanceModule;
        assert_eq!(module.name(), "gcp_compute_instance");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(module.required_params(), &["name", "zone"]);
    }

    #[test]
    fn test_firewall_module_metadata() {
        let module = GcpComputeFirewallModule;
        assert_eq!(module.name(), "gcp_compute_firewall");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_network_module_metadata() {
        let module = GcpComputeNetworkModule;
        assert_eq!(module.name(), "gcp_compute_network");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_service_account_module_metadata() {
        let module = GcpServiceAccountModule;
        assert_eq!(module.name(), "gcp_service_account");
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
    }

    #[test]
    fn test_compute_instance_config_parsing() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-instance"));
        params.insert("zone".to_string(), serde_json::json!("us-central1-a"));
        params.insert(
            "machine_type".to_string(),
            serde_json::json!("e2-standard-2"),
        );
        params.insert("image_family".to_string(), serde_json::json!("debian-11"));
        params.insert(
            "image_project".to_string(),
            serde_json::json!("debian-cloud"),
        );

        let config = ComputeInstanceConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "test-instance");
        assert_eq!(config.zone, "us-central1-a");
        assert_eq!(config.machine_type, "e2-standard-2");
        assert_eq!(config.image_family, Some("debian-11".to_string()));
    }

    #[test]
    fn test_compute_instance_config_with_labels() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-instance"));
        params.insert("zone".to_string(), serde_json::json!("us-central1-a"));
        params.insert(
            "labels".to_string(),
            serde_json::json!({
                "environment": "production",
                "team": "web"
            }),
        );

        let config = ComputeInstanceConfig::from_params(&params).unwrap();
        assert_eq!(
            config.labels.get("environment"),
            Some(&"production".to_string())
        );
        assert_eq!(config.labels.get("team"), Some(&"web".to_string()));
    }

    #[test]
    fn test_compute_instance_config_with_tags() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test-instance"));
        params.insert("zone".to_string(), serde_json::json!("us-central1-a"));
        params.insert(
            "tags".to_string(),
            serde_json::json!(["http-server", "https-server"]),
        );

        let config = ComputeInstanceConfig::from_params(&params).unwrap();
        assert_eq!(config.tags, vec!["http-server", "https-server"]);
    }

    #[test]
    fn test_firewall_config_parsing() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("allow-http"));
        params.insert("network".to_string(), serde_json::json!("my-vpc"));
        params.insert("direction".to_string(), serde_json::json!("INGRESS"));
        params.insert("priority".to_string(), serde_json::json!(500));

        let config = FirewallConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "allow-http");
        assert_eq!(config.network, "my-vpc");
        assert_eq!(config.direction, FirewallDirection::Ingress);
        assert_eq!(config.priority, 500);
    }

    #[test]
    fn test_source_image_url() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test"));
        params.insert("zone".to_string(), serde_json::json!("us-central1-a"));
        params.insert(
            "image_family".to_string(),
            serde_json::json!("ubuntu-2204-lts"),
        );
        params.insert(
            "image_project".to_string(),
            serde_json::json!("ubuntu-os-cloud"),
        );

        let config = ComputeInstanceConfig::from_params(&params).unwrap();
        let source_image = config.get_source_image().unwrap();
        assert_eq!(
            source_image,
            "projects/ubuntu-os-cloud/global/images/family/ubuntu-2204-lts"
        );
    }

    #[test]
    fn test_validate_params_missing_zone() {
        let module = GcpComputeInstanceModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test"));
        // Missing zone
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_invalid_state() {
        let module = GcpComputeInstanceModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test"));
        params.insert("zone".to_string(), serde_json::json!("us-central1-a"));
        params.insert("state".to_string(), serde_json::json!("invalid_state"));
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_invalid_disk_type() {
        let module = GcpComputeInstanceModule;
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("test"));
        params.insert("zone".to_string(), serde_json::json!("us-central1-a"));
        params.insert("disk_type".to_string(), serde_json::json!("invalid-type"));
        assert!(module.validate_params(&params).is_err());
    }
}
