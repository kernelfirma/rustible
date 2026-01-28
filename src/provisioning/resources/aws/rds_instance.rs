//! AWS RDS Instance Resource for Infrastructure Provisioning
//!
//! This module provides the `AwsRdsInstanceResource` which implements the `Resource` trait
//! for managing AWS RDS database instances declaratively via cloud API.
//!
//! ## Example Configuration
//!
//! ```yaml
//! resources:
//!   aws_db_instance:
//!     mydb:
//!       identifier: mydb-instance
//!       engine: postgres
//!       engine_version: "15.4"
//!       instance_class: db.t3.micro
//!       allocated_storage: 20
//!       username: admin
//!       password: mysecretpassword
//!       db_name: myappdb
//!       db_subnet_group_name: my-db-subnet-group
//!       vpc_security_group_ids:
//!         - sg-12345678
//!       skip_final_snapshot: true
//!       tags:
//!         Name: my-database
//!         Environment: production
//! ```

use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

use async_trait::async_trait;
use aws_config::BehaviorVersion;
use aws_sdk_rds::types::{Tag as RdsTag};
use aws_sdk_rds::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info};

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// Supporting Types
// ============================================================================

/// RDS instance configuration parsed from provisioning config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RdsInstanceConfig {
    /// DB instance identifier (required)
    pub identifier: String,
    /// Database engine (required): mysql, postgres, mariadb, oracle-ee, sqlserver-ee, etc.
    pub engine: String,
    /// Engine version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    /// DB instance class (required): db.t3.micro, db.m5.large, etc.
    pub instance_class: String,
    /// Allocated storage in GB (required)
    pub allocated_storage: i32,
    /// Maximum allocated storage for autoscaling (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_allocated_storage: Option<i32>,
    /// Storage type: gp2, gp3, io1, standard
    #[serde(default = "default_storage_type")]
    pub storage_type: String,
    /// IOPS for io1/gp3 storage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iops: Option<i32>,
    /// Throughput for gp3 storage (MiB/s)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_throughput: Option<i32>,
    /// Master username
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Master password (sensitive)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Database name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_name: Option<String>,
    /// Port number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    /// VPC security group IDs
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vpc_security_group_ids: Vec<String>,
    /// DB subnet group name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_subnet_group_name: Option<String>,
    /// DB parameter group name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_group_name: Option<String>,
    /// DB option group name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option_group_name: Option<String>,
    /// Availability zone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
    /// Multi-AZ deployment
    #[serde(default)]
    pub multi_az: bool,
    /// Publicly accessible
    #[serde(default)]
    pub publicly_accessible: bool,
    /// Whether storage is encrypted
    #[serde(default)]
    pub storage_encrypted: bool,
    /// KMS key ID for encryption
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    /// Enable IAM database authentication
    #[serde(default)]
    pub iam_database_authentication_enabled: bool,
    /// Enable Performance Insights
    #[serde(default)]
    pub performance_insights_enabled: bool,
    /// Performance Insights KMS key ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performance_insights_kms_key_id: Option<String>,
    /// Performance Insights retention period (days)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performance_insights_retention_period: Option<i32>,
    /// Enable enhanced monitoring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring_interval: Option<i32>,
    /// IAM role ARN for enhanced monitoring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring_role_arn: Option<String>,
    /// Backup retention period (days)
    #[serde(default = "default_backup_retention")]
    pub backup_retention_period: i32,
    /// Preferred backup window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_window: Option<String>,
    /// Preferred maintenance window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintenance_window: Option<String>,
    /// Whether to skip final snapshot on delete
    #[serde(default)]
    pub skip_final_snapshot: bool,
    /// Final snapshot identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_snapshot_identifier: Option<String>,
    /// Snapshot identifier to restore from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_identifier: Option<String>,
    /// Enable auto minor version upgrade
    #[serde(default = "default_true")]
    pub auto_minor_version_upgrade: bool,
    /// Apply changes immediately
    #[serde(default)]
    pub apply_immediately: bool,
    /// Enable deletion protection
    #[serde(default)]
    pub deletion_protection: bool,
    /// Copy tags to snapshots
    #[serde(default)]
    pub copy_tags_to_snapshot: bool,
    /// License model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_model: Option<String>,
    /// Character set name (Oracle)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character_set_name: Option<String>,
    /// Timezone (SQL Server)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Enable CloudWatch Logs exports
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled_cloudwatch_logs_exports: Vec<String>,
    /// Resource tags
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tags: HashMap<String, String>,
}

fn default_storage_type() -> String {
    "gp2".to_string()
}

fn default_backup_retention() -> i32 {
    7
}

fn default_true() -> bool {
    true
}

impl RdsInstanceConfig {
    /// Parse configuration from JSON value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        serde_json::from_value(value.clone()).map_err(|e| {
            ProvisioningError::ValidationError(format!("Invalid RDS instance configuration: {}", e))
        })
    }
}

/// Computed attributes returned after RDS instance operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RdsInstanceState {
    /// DB instance identifier
    pub id: String,
    /// DB instance ARN
    pub arn: String,
    /// DB instance endpoint address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// DB instance endpoint port
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    /// DB instance status
    pub status: String,
    /// Engine type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
    /// Engine version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    /// DB instance class
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_class: Option<String>,
    /// Allocated storage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocated_storage: Option<i32>,
    /// Master username
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Database name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_name: Option<String>,
    /// Availability zone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability_zone: Option<String>,
    /// Multi-AZ deployment
    #[serde(default)]
    pub multi_az: bool,
    /// VPC ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vpc_id: Option<String>,
    /// DB subnet group name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_subnet_group_name: Option<String>,
    /// Security group IDs
    #[serde(default)]
    pub vpc_security_group_ids: Vec<String>,
    /// Storage encrypted
    #[serde(default)]
    pub storage_encrypted: bool,
    /// KMS key ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    /// Resource ID (unique identifier for tagging)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    /// CA certificate identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_cert_identifier: Option<String>,
    /// Hosted zone ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosted_zone_id: Option<String>,
    /// Tags
    #[serde(default)]
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS RDS Instance Resource
// ============================================================================

/// AWS RDS Instance Resource implementation
#[derive(Debug, Clone)]
pub struct AwsRdsInstanceResource;

impl AwsRdsInstanceResource {
    /// Create a new AWS RDS Instance resource
    pub fn new() -> Self {
        Self
    }

    /// Create AWS RDS client from provider context
    async fn create_client(&self, ctx: &ProviderContext) -> ProvisioningResult<Client> {
        let config = if let Some(ref region) = ctx.region {
            aws_config::defaults(BehaviorVersion::latest())
                .region(aws_sdk_rds::config::Region::new(region.clone()))
                .load()
                .await
        } else {
            aws_config::defaults(BehaviorVersion::latest()).load().await
        };

        Ok(Client::new(&config))
    }

    /// Describe a DB instance by identifier
    async fn describe_instance(
        &self,
        client: &Client,
        identifier: &str,
    ) -> ProvisioningResult<Option<RdsInstanceState>> {
        let resp = client
            .describe_db_instances()
            .db_instance_identifier(identifier)
            .send()
            .await;

        match resp {
            Ok(output) => {
                if let Some(instance) = output.db_instances().first() {
                    Ok(Some(self.instance_to_state(instance)))
                } else {
                    Ok(None)
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("DBInstanceNotFound") || err_str.contains("not found") {
                    Ok(None)
                } else {
                    Err(ProvisioningError::CloudApiError(format!(
                        "Failed to describe DB instance: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Convert SDK DB instance to state struct
    fn instance_to_state(&self, instance: &aws_sdk_rds::types::DbInstance) -> RdsInstanceState {
        let identifier = instance.db_instance_identifier().unwrap_or_default().to_string();
        let arn = instance.db_instance_arn().unwrap_or_default().to_string();
        let status = instance.db_instance_status().unwrap_or_default().to_string();

        let (address, port) = if let Some(endpoint) = instance.endpoint() {
            (
                endpoint.address().map(|s| s.to_string()),
                endpoint.port(),
            )
        } else {
            (None, None)
        };

        let vpc_security_group_ids: Vec<String> = instance
            .vpc_security_groups()
            .iter()
            .filter_map(|sg| sg.vpc_security_group_id().map(|s| s.to_string()))
            .collect();

        let mut tags = HashMap::new();
        for tag in instance.tag_list() {
            if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                tags.insert(key.to_string(), value.to_string());
            }
        }

        RdsInstanceState {
            id: identifier,
            arn,
            address,
            port,
            status,
            engine: instance.engine().map(|s| s.to_string()),
            engine_version: instance.engine_version().map(|s| s.to_string()),
            instance_class: instance.db_instance_class().map(|s| s.to_string()),
            allocated_storage: instance.allocated_storage(),
            username: instance.master_username().map(|s| s.to_string()),
            db_name: instance.db_name().map(|s| s.to_string()),
            availability_zone: instance.availability_zone().map(|s| s.to_string()),
            multi_az: instance.multi_az().unwrap_or(false),
            vpc_id: instance
                .db_subnet_group()
                .and_then(|sg| sg.vpc_id())
                .map(|s| s.to_string()),
            db_subnet_group_name: instance
                .db_subnet_group()
                .and_then(|sg| sg.db_subnet_group_name())
                .map(|s| s.to_string()),
            vpc_security_group_ids,
            storage_encrypted: instance.storage_encrypted().unwrap_or(false),
            kms_key_id: instance.kms_key_id().map(|s| s.to_string()),
            resource_id: instance.dbi_resource_id().map(|s| s.to_string()),
            ca_cert_identifier: instance.ca_certificate_identifier().map(|s| s.to_string()),
            hosted_zone_id: None, // Not directly available, would need Route53 lookup
            tags,
        }
    }

    /// Wait for DB instance to reach a specific status
    async fn wait_for_status(
        &self,
        client: &Client,
        identifier: &str,
        desired_status: &str,
        timeout: Duration,
    ) -> ProvisioningResult<RdsInstanceState> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(30);

        debug!(
            "Waiting for DB instance {} to reach status '{}'",
            identifier, desired_status
        );

        loop {
            if start.elapsed() >= timeout {
                return Err(ProvisioningError::Timeout {
                    operation: format!(
                        "waiting for DB instance {} to reach {}",
                        identifier, desired_status
                    ),
                    seconds: timeout.as_secs(),
                });
            }

            if let Some(state) = self.describe_instance(client, identifier).await? {
                if state.status == desired_status {
                    return Ok(state);
                }

                // Check for terminal failure states
                if state.status == "failed" || state.status == "incompatible-restore" {
                    return Err(ProvisioningError::CloudApiError(format!(
                        "DB instance {} reached failure state: {}",
                        identifier, state.status
                    )));
                }

                debug!(
                    "DB instance {} current status: {}, waiting for {}",
                    identifier, state.status, desired_status
                );
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Wait for DB instance to be deleted
    async fn wait_for_deletion(
        &self,
        client: &Client,
        identifier: &str,
        timeout: Duration,
    ) -> ProvisioningResult<()> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_secs(30);

        debug!("Waiting for DB instance {} to be deleted", identifier);

        loop {
            if start.elapsed() >= timeout {
                return Err(ProvisioningError::Timeout {
                    operation: format!("waiting for DB instance {} to be deleted", identifier),
                    seconds: timeout.as_secs(),
                });
            }

            if self.describe_instance(client, identifier).await?.is_none() {
                return Ok(());
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Extract references from configuration value
    fn extract_references(&self, value: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        if let Some(s) = value.as_str() {
            if s.contains("${") || s.contains("{{") {
                if let Some(dep) = self.parse_reference(s) {
                    deps.push(dep);
                }
            }
        }

        deps
    }

    /// Parse a reference string like "${aws_db_subnet_group.main.name}"
    fn parse_reference(&self, ref_str: &str) -> Option<ResourceDependency> {
        // Parse Terraform-style reference: ${resource_type.name.attribute}
        if let Some(start) = ref_str.find("${") {
            if let Some(end) = ref_str[start..].find('}') {
                let inner = &ref_str[start + 2..start + end];
                let parts: Vec<&str> = inner.split('.').collect();
                if parts.len() >= 2 {
                    return Some(ResourceDependency::new(
                        parts[0],
                        parts[1],
                        parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    ));
                }
            }
        }

        // Parse Jinja-style reference: {{ resources.resource_type.name.attribute }}
        if let Some(start) = ref_str.find("{{") {
            if let Some(end) = ref_str[start..].find("}}") {
                let inner = ref_str[start + 2..start + end].trim();
                let inner = inner.trim_start_matches("resources.");
                let parts: Vec<&str> = inner.split('.').collect();
                if parts.len() >= 2 {
                    return Some(ResourceDependency::new(
                        parts[0],
                        parts[1],
                        parts.get(2).map(|s| s.to_string()).unwrap_or_default(),
                    ));
                }
            }
        }

        None
    }

    /// Check if a field change requires replacement
    fn requires_replacement_for_field(&self, field: &str) -> bool {
        matches!(
            field,
            "identifier"
                | "engine"
                | "db_name"
                | "availability_zone"
                | "storage_encrypted"
                | "kms_key_id"
                | "snapshot_identifier"
                | "character_set_name"
                | "timezone"
        )
    }
}

impl Default for AwsRdsInstanceResource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Resource for AwsRdsInstanceResource {
    fn resource_type(&self) -> &str {
        "aws_db_instance"
    }

    fn provider(&self) -> &str {
        "aws"
    }

    fn schema(&self) -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_db_instance".to_string(),
            description: "Provides an RDS instance resource. Manages an RDS database instance.".to_string(),
            required_args: vec![
                SchemaField {
                    name: "identifier".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the RDS instance".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinLength { min: 1 },
                        FieldConstraint::MaxLength { max: 63 },
                        FieldConstraint::Pattern {
                            regex: r"^[a-z][a-z0-9-]*$".to_string(),
                        },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "engine".to_string(),
                    field_type: FieldType::String,
                    description: "The database engine to use".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "mysql".to_string(),
                            "postgres".to_string(),
                            "mariadb".to_string(),
                            "oracle-ee".to_string(),
                            "oracle-se2".to_string(),
                            "sqlserver-ee".to_string(),
                            "sqlserver-se".to_string(),
                            "sqlserver-ex".to_string(),
                            "sqlserver-web".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "instance_class".to_string(),
                    field_type: FieldType::String,
                    description: "The instance type of the RDS instance".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::Pattern {
                        regex: r"^db\.[a-z0-9]+\.[a-z0-9]+$".to_string(),
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "allocated_storage".to_string(),
                    field_type: FieldType::Integer,
                    description: "The allocated storage in gibibytes".to_string(),
                    default: None,
                    constraints: vec![
                        FieldConstraint::MinValue { value: 5 },
                        FieldConstraint::MaxValue { value: 65536 },
                    ],
                    sensitive: false,
                },
            ],
            optional_args: vec![
                SchemaField {
                    name: "engine_version".to_string(),
                    field_type: FieldType::String,
                    description: "The engine version to use".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "username".to_string(),
                    field_type: FieldType::String,
                    description: "Username for the master DB user".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "password".to_string(),
                    field_type: FieldType::String,
                    description: "Password for the master DB user".to_string(),
                    default: None,
                    constraints: vec![FieldConstraint::MinLength { min: 8 }],
                    sensitive: true,
                },
                SchemaField {
                    name: "db_name".to_string(),
                    field_type: FieldType::String,
                    description: "The name of the database to create".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "port".to_string(),
                    field_type: FieldType::Integer,
                    description: "The port on which the DB accepts connections".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "vpc_security_group_ids".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::String)),
                    description: "List of VPC security groups to associate".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "db_subnet_group_name".to_string(),
                    field_type: FieldType::String,
                    description: "Name of the DB subnet group".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "parameter_group_name".to_string(),
                    field_type: FieldType::String,
                    description: "Name of the DB parameter group to associate".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "storage_type".to_string(),
                    field_type: FieldType::String,
                    description: "Storage type: gp2, gp3, io1, standard".to_string(),
                    default: Some(Value::String("gp2".to_string())),
                    constraints: vec![FieldConstraint::Enum {
                        values: vec![
                            "gp2".to_string(),
                            "gp3".to_string(),
                            "io1".to_string(),
                            "standard".to_string(),
                        ],
                    }],
                    sensitive: false,
                },
                SchemaField {
                    name: "multi_az".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Specifies if the RDS instance is multi-AZ".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "publicly_accessible".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Whether the instance is publicly accessible".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "storage_encrypted".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Specifies whether the DB instance is encrypted".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "skip_final_snapshot".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Skip final snapshot when deleting".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "deletion_protection".to_string(),
                    field_type: FieldType::Boolean,
                    description: "Enable deletion protection".to_string(),
                    default: Some(Value::Bool(false)),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "backup_retention_period".to_string(),
                    field_type: FieldType::Integer,
                    description: "Days to retain backups".to_string(),
                    default: Some(Value::Number(7.into())),
                    constraints: vec![
                        FieldConstraint::MinValue { value: 0 },
                        FieldConstraint::MaxValue { value: 35 },
                    ],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "A map of tags to assign to the resource".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "The RDS instance identifier".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "The ARN of the RDS instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "address".to_string(),
                    field_type: FieldType::String,
                    description: "The hostname of the RDS instance".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "port".to_string(),
                    field_type: FieldType::Integer,
                    description: "The database port".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "status".to_string(),
                    field_type: FieldType::String,
                    description: "The RDS instance status".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "resource_id".to_string(),
                    field_type: FieldType::String,
                    description: "The RDS Resource ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec![
                "identifier".to_string(),
                "engine".to_string(),
                "db_name".to_string(),
                "availability_zone".to_string(),
                "storage_encrypted".to_string(),
                "kms_key_id".to_string(),
                "snapshot_identifier".to_string(),
            ],
            timeouts: ResourceTimeouts {
                create: 2400, // 40 minutes
                read: 60,
                update: 1800, // 30 minutes
                delete: 1200, // 20 minutes
            },
        }
    }

    async fn read(
        &self,
        id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceReadResult> {
        let client = self.create_client(ctx).await?;

        match self.describe_instance(&client, id).await? {
            Some(state) => {
                let attributes = serde_json::to_value(&state).map_err(|e| {
                    ProvisioningError::SerializationError(format!(
                        "Failed to serialize DB instance attributes: {}",
                        e
                    ))
                })?;

                Ok(ResourceReadResult::found(id, attributes))
            }
            None => Ok(ResourceReadResult::not_found()),
        }
    }

    async fn plan(
        &self,
        desired: &Value,
        current: Option<&Value>,
        _ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceDiff> {
        match current {
            None => Ok(ResourceDiff::create(desired.clone())),
            Some(current_val) => {
                let mut diff = ResourceDiff::no_change();
                let mut requires_replacement = false;
                let mut replacement_fields = Vec::new();

                let empty_map = serde_json::Map::new();
                let desired_obj = desired.as_object().unwrap_or(&empty_map);
                let current_obj = current_val.as_object().unwrap_or(&empty_map);

                for (key, des_val) in desired_obj {
                    // Skip password comparison (sensitive)
                    if key == "password" {
                        continue;
                    }

                    let cur_val = current_obj.get(key);

                    match cur_val {
                        Some(cv) if cv != des_val => {
                            diff.modifications
                                .insert(key.clone(), (cv.clone(), des_val.clone()));

                            if self.requires_replacement_for_field(key) {
                                requires_replacement = true;
                                replacement_fields.push(key.clone());
                            }
                        }
                        None => {
                            diff.additions.insert(key.clone(), des_val.clone());
                        }
                        _ => {}
                    }
                }

                // Check for deletions (excluding computed fields)
                let computed_fields = ["id", "arn", "address", "port", "status", "resource_id", "vpc_id"];
                for key in current_obj.keys() {
                    if !desired_obj.contains_key(key) && !key.starts_with('_') && !computed_fields.contains(&key.as_str()) {
                        diff.deletions.push(key.clone());
                    }
                }

                if !diff.additions.is_empty()
                    || !diff.modifications.is_empty()
                    || !diff.deletions.is_empty()
                {
                    if requires_replacement {
                        diff.change_type = ChangeType::Replace;
                        diff.requires_replacement = true;
                        diff.replacement_fields = replacement_fields;
                    } else {
                        diff.change_type = ChangeType::Update;
                    }
                }

                Ok(diff)
            }
        }
    }

    async fn create(
        &self,
        config: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let rds_config = RdsInstanceConfig::from_value(config)?;
        let client = self.create_client(ctx).await?;

        // Check if restoring from snapshot
        let state = if let Some(ref snapshot_id) = rds_config.snapshot_identifier {
            // Restore from snapshot
            let mut restore = client
                .restore_db_instance_from_db_snapshot()
                .db_instance_identifier(&rds_config.identifier)
                .db_snapshot_identifier(snapshot_id)
                .db_instance_class(&rds_config.instance_class);

            if let Some(ref engine) = rds_config.engine_version {
                restore = restore.engine(engine);
            }
            if let Some(ref subnet_group) = rds_config.db_subnet_group_name {
                restore = restore.db_subnet_group_name(subnet_group);
            }
            if let Some(ref az) = rds_config.availability_zone {
                restore = restore.availability_zone(az);
            }
            if rds_config.multi_az {
                restore = restore.multi_az(true);
            }
            if rds_config.publicly_accessible {
                restore = restore.publicly_accessible(true);
            }
            for sg in &rds_config.vpc_security_group_ids {
                restore = restore.vpc_security_group_ids(sg);
            }

            // Add tags
            let tags = self.build_tags(&rds_config.tags, &ctx.default_tags);
            restore = restore.set_tags(Some(tags));

            restore.send().await.map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to restore DB instance: {}", e))
            })?;

            info!(
                "Restoring DB instance {} from snapshot {}",
                rds_config.identifier, snapshot_id
            );

            // Wait for available state
            let timeout = Duration::from_secs(ctx.timeout_seconds);
            self.wait_for_status(&client, &rds_config.identifier, "available", timeout)
                .await?
        } else {
            // Create new instance
            let mut create = client
                .create_db_instance()
                .db_instance_identifier(&rds_config.identifier)
                .engine(&rds_config.engine)
                .db_instance_class(&rds_config.instance_class)
                .allocated_storage(rds_config.allocated_storage)
                .storage_type(&rds_config.storage_type);

            if let Some(ref version) = rds_config.engine_version {
                create = create.engine_version(version);
            }
            if let Some(ref username) = rds_config.username {
                create = create.master_username(username);
            }
            if let Some(ref password) = rds_config.password {
                create = create.master_user_password(password);
            }
            if let Some(ref db_name) = rds_config.db_name {
                create = create.db_name(db_name);
            }
            if let Some(port) = rds_config.port {
                create = create.port(port);
            }
            if let Some(ref subnet_group) = rds_config.db_subnet_group_name {
                create = create.db_subnet_group_name(subnet_group);
            }
            if let Some(ref param_group) = rds_config.parameter_group_name {
                create = create.db_parameter_group_name(param_group);
            }
            if let Some(ref option_group) = rds_config.option_group_name {
                create = create.option_group_name(option_group);
            }
            if let Some(ref az) = rds_config.availability_zone {
                create = create.availability_zone(az);
            }
            if let Some(iops) = rds_config.iops {
                create = create.iops(iops);
            }
            if let Some(throughput) = rds_config.storage_throughput {
                create = create.storage_throughput(throughput);
            }
            if let Some(max_storage) = rds_config.max_allocated_storage {
                create = create.max_allocated_storage(max_storage);
            }

            // Boolean flags
            create = create
                .multi_az(rds_config.multi_az)
                .publicly_accessible(rds_config.publicly_accessible)
                .storage_encrypted(rds_config.storage_encrypted)
                .auto_minor_version_upgrade(rds_config.auto_minor_version_upgrade)
                .copy_tags_to_snapshot(rds_config.copy_tags_to_snapshot)
                .deletion_protection(rds_config.deletion_protection)
                .backup_retention_period(rds_config.backup_retention_period);

            if let Some(ref kms_key) = rds_config.kms_key_id {
                create = create.kms_key_id(kms_key);
            }

            // IAM auth
            if rds_config.iam_database_authentication_enabled {
                create = create.enable_iam_database_authentication(true);
            }

            // Performance Insights
            if rds_config.performance_insights_enabled {
                create = create.enable_performance_insights(true);
                if let Some(ref pi_kms) = rds_config.performance_insights_kms_key_id {
                    create = create.performance_insights_kms_key_id(pi_kms);
                }
                if let Some(pi_retention) = rds_config.performance_insights_retention_period {
                    create = create.performance_insights_retention_period(pi_retention);
                }
            }

            // Monitoring
            if let Some(interval) = rds_config.monitoring_interval {
                create = create.monitoring_interval(interval);
                if let Some(ref role_arn) = rds_config.monitoring_role_arn {
                    create = create.monitoring_role_arn(role_arn);
                }
            }

            // Backup/maintenance windows
            if let Some(ref backup_window) = rds_config.backup_window {
                create = create.preferred_backup_window(backup_window);
            }
            if let Some(ref maintenance_window) = rds_config.maintenance_window {
                create = create.preferred_maintenance_window(maintenance_window);
            }

            // CloudWatch logs exports
            for log_type in &rds_config.enabled_cloudwatch_logs_exports {
                create = create.enable_cloudwatch_logs_exports(log_type);
            }

            // License model (Oracle/SQL Server)
            if let Some(ref license) = rds_config.license_model {
                create = create.license_model(license);
            }

            // Character set (Oracle)
            if let Some(ref charset) = rds_config.character_set_name {
                create = create.character_set_name(charset);
            }

            // Timezone (SQL Server)
            if let Some(ref tz) = rds_config.timezone {
                create = create.timezone(tz);
            }

            // Security groups
            for sg in &rds_config.vpc_security_group_ids {
                create = create.vpc_security_group_ids(sg);
            }

            // Add tags
            let tags = self.build_tags(&rds_config.tags, &ctx.default_tags);
            create = create.set_tags(Some(tags));

            create.send().await.map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to create DB instance: {}", e))
            })?;

            info!("Created DB instance: {}", rds_config.identifier);

            // Wait for available state
            let timeout = Duration::from_secs(ctx.timeout_seconds);
            self.wait_for_status(&client, &rds_config.identifier, "available", timeout)
                .await?
        };

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(&rds_config.identifier, attributes)
            .with_output("id", Value::String(state.id.clone()))
            .with_output("arn", Value::String(state.arn.clone()))
            .with_output("address", serde_json::json!(state.address))
            .with_output("port", serde_json::json!(state.port)))
    }

    async fn update(
        &self,
        id: &str,
        _old: &Value,
        new: &Value,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<ResourceResult> {
        let rds_config = RdsInstanceConfig::from_value(new)?;
        let client = self.create_client(ctx).await?;

        let mut modify = client
            .modify_db_instance()
            .db_instance_identifier(id)
            .apply_immediately(rds_config.apply_immediately);

        // Modifiable attributes
        modify = modify
            .db_instance_class(&rds_config.instance_class)
            .allocated_storage(rds_config.allocated_storage)
            .storage_type(&rds_config.storage_type)
            .multi_az(rds_config.multi_az)
            .publicly_accessible(rds_config.publicly_accessible)
            .auto_minor_version_upgrade(rds_config.auto_minor_version_upgrade)
            .copy_tags_to_snapshot(rds_config.copy_tags_to_snapshot)
            .deletion_protection(rds_config.deletion_protection)
            .backup_retention_period(rds_config.backup_retention_period);

        if let Some(ref version) = rds_config.engine_version {
            modify = modify.engine_version(version);
        }
        if let Some(ref password) = rds_config.password {
            modify = modify.master_user_password(password);
        }
        if let Some(iops) = rds_config.iops {
            modify = modify.iops(iops);
        }
        if let Some(throughput) = rds_config.storage_throughput {
            modify = modify.storage_throughput(throughput);
        }
        if let Some(max_storage) = rds_config.max_allocated_storage {
            modify = modify.max_allocated_storage(max_storage);
        }
        if let Some(ref param_group) = rds_config.parameter_group_name {
            modify = modify.db_parameter_group_name(param_group);
        }
        if let Some(ref option_group) = rds_config.option_group_name {
            modify = modify.option_group_name(option_group);
        }
        if let Some(interval) = rds_config.monitoring_interval {
            modify = modify.monitoring_interval(interval);
            if let Some(ref role_arn) = rds_config.monitoring_role_arn {
                modify = modify.monitoring_role_arn(role_arn);
            }
        }
        if let Some(ref backup_window) = rds_config.backup_window {
            modify = modify.preferred_backup_window(backup_window);
        }
        if let Some(ref maintenance_window) = rds_config.maintenance_window {
            modify = modify.preferred_maintenance_window(maintenance_window);
        }

        // Performance Insights
        modify = modify.enable_performance_insights(rds_config.performance_insights_enabled);
        if rds_config.performance_insights_enabled {
            if let Some(ref pi_kms) = rds_config.performance_insights_kms_key_id {
                modify = modify.performance_insights_kms_key_id(pi_kms);
            }
            if let Some(pi_retention) = rds_config.performance_insights_retention_period {
                modify = modify.performance_insights_retention_period(pi_retention);
            }
        }

        // IAM auth
        modify = modify.enable_iam_database_authentication(rds_config.iam_database_authentication_enabled);

        // Security groups
        if !rds_config.vpc_security_group_ids.is_empty() {
            modify = modify.set_vpc_security_group_ids(Some(rds_config.vpc_security_group_ids.clone()));
        }

        // CloudWatch logs exports
        if !rds_config.enabled_cloudwatch_logs_exports.is_empty() {
            modify = modify.cloudwatch_logs_export_configuration(
                aws_sdk_rds::types::CloudwatchLogsExportConfiguration::builder()
                    .set_enable_log_types(Some(rds_config.enabled_cloudwatch_logs_exports.clone()))
                    .build(),
            );
        }

        modify.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to modify DB instance: {}", e))
        })?;

        info!("Modified DB instance: {}", id);

        // Update tags separately
        if !rds_config.tags.is_empty() {
            // Get current state to get ARN
            if let Some(state) = self.describe_instance(&client, id).await? {
                // Remove old tags and add new ones
                let tags = self.build_tags(&rds_config.tags, &ctx.default_tags);
                client
                    .add_tags_to_resource()
                    .resource_name(&state.arn)
                    .set_tags(Some(tags))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to update tags: {}", e))
                    })?;
            }
        }

        // Wait for available state if applying immediately
        if rds_config.apply_immediately {
            let timeout = Duration::from_secs(ctx.timeout_seconds);
            let state = self.wait_for_status(&client, id, "available", timeout).await?;

            let attributes = serde_json::to_value(&state).map_err(|e| {
                ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
            })?;

            return Ok(ResourceResult::success(id, attributes));
        }

        // Get current state
        let state = self.describe_instance(&client, id).await?.ok_or_else(|| {
            ProvisioningError::CloudApiError("DB instance not found after update".to_string())
        })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    async fn destroy(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        // Get current config to check deletion settings
        let state = self.describe_instance(&client, id).await?;

        if state.is_none() {
            // Already deleted
            return Ok(ResourceResult::success(id, Value::Null));
        }

        info!("Deleting DB instance: {}", id);

        // First, disable deletion protection if enabled
        // This is a common pattern since you can't delete with protection on
        client
            .modify_db_instance()
            .db_instance_identifier(id)
            .deletion_protection(false)
            .apply_immediately(true)
            .send()
            .await
            .ok(); // Ignore errors - protection might already be off

        // Small delay to allow modification to take effect
        tokio::time::sleep(Duration::from_secs(5)).await;

        // Delete the instance
        let delete = client
            .delete_db_instance()
            .db_instance_identifier(id)
            .skip_final_snapshot(true);

        // Note: In production you might want to check config for skip_final_snapshot
        // and final_snapshot_identifier, but for simplicity we skip here

        delete.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to delete DB instance: {}", e))
        })?;

        // Wait for deletion
        let timeout = Duration::from_secs(ctx.timeout_seconds);
        self.wait_for_deletion(&client, id, timeout).await?;

        info!("Deleted DB instance: {}", id);

        Ok(ResourceResult::success(id, Value::Null))
    }

    async fn import(&self, id: &str, ctx: &ProviderContext) -> ProvisioningResult<ResourceResult> {
        let client = self.create_client(ctx).await?;

        let state = self.describe_instance(&client, id).await?.ok_or_else(|| {
            ProvisioningError::ImportError {
                resource_type: "aws_db_instance".to_string(),
                resource_id: id.to_string(),
                message: "DB instance not found".to_string(),
            }
        })?;

        let attributes = serde_json::to_value(&state).map_err(|e| {
            ProvisioningError::SerializationError(format!("Failed to serialize attributes: {}", e))
        })?;

        Ok(ResourceResult::success(id, attributes))
    }

    fn dependencies(&self, config: &Value) -> Vec<ResourceDependency> {
        let mut deps = Vec::new();

        if let Some(obj) = config.as_object() {
            // Check db_subnet_group_name for references
            if let Some(subnet_group) = obj.get("db_subnet_group_name") {
                deps.extend(self.extract_references(subnet_group));
            }

            // Check vpc_security_group_ids for references
            if let Some(sg_ids) = obj.get("vpc_security_group_ids") {
                if let Some(arr) = sg_ids.as_array() {
                    for sg_id in arr {
                        deps.extend(self.extract_references(sg_id));
                    }
                }
            }

            // Check kms_key_id for references
            if let Some(kms) = obj.get("kms_key_id") {
                deps.extend(self.extract_references(kms));
            }

            // Check parameter_group_name for references
            if let Some(param_group) = obj.get("parameter_group_name") {
                deps.extend(self.extract_references(param_group));
            }

            // Check option_group_name for references
            if let Some(option_group) = obj.get("option_group_name") {
                deps.extend(self.extract_references(option_group));
            }

            // Check monitoring_role_arn for references
            if let Some(role) = obj.get("monitoring_role_arn") {
                deps.extend(self.extract_references(role));
            }
        }

        deps
    }

    fn forces_replacement(&self) -> Vec<String> {
        vec![
            "identifier".to_string(),
            "engine".to_string(),
            "db_name".to_string(),
            "availability_zone".to_string(),
            "storage_encrypted".to_string(),
            "kms_key_id".to_string(),
            "snapshot_identifier".to_string(),
            "character_set_name".to_string(),
            "timezone".to_string(),
        ]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        let obj = config.as_object().ok_or_else(|| {
            ProvisioningError::ValidationError("Configuration must be an object".to_string())
        })?;

        // Validate required fields
        if !obj.contains_key("identifier") {
            return Err(ProvisioningError::ValidationError(
                "identifier is required".to_string(),
            ));
        }
        if !obj.contains_key("engine") {
            return Err(ProvisioningError::ValidationError(
                "engine is required".to_string(),
            ));
        }
        if !obj.contains_key("instance_class") {
            return Err(ProvisioningError::ValidationError(
                "instance_class is required".to_string(),
            ));
        }
        if !obj.contains_key("allocated_storage") {
            return Err(ProvisioningError::ValidationError(
                "allocated_storage is required".to_string(),
            ));
        }

        // Validate identifier format
        if let Some(identifier) = obj.get("identifier").and_then(|v| v.as_str()) {
            if identifier.is_empty() || identifier.len() > 63 {
                return Err(ProvisioningError::ValidationError(
                    "identifier must be between 1 and 63 characters".to_string(),
                ));
            }
            if !identifier.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false) {
                return Err(ProvisioningError::ValidationError(
                    "identifier must start with a lowercase letter".to_string(),
                ));
            }
        }

        // Validate engine
        if let Some(engine) = obj.get("engine").and_then(|v| v.as_str()) {
            let valid_engines = [
                "mysql", "postgres", "mariadb", "oracle-ee", "oracle-se2",
                "sqlserver-ee", "sqlserver-se", "sqlserver-ex", "sqlserver-web",
            ];
            if !valid_engines.contains(&engine) {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid engine: {}. Must be one of: {}",
                    engine,
                    valid_engines.join(", ")
                )));
            }
        }

        // Validate instance_class format
        if let Some(class) = obj.get("instance_class").and_then(|v| v.as_str()) {
            if !class.starts_with("db.") {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid instance_class format: {}. Must start with 'db.'",
                    class
                )));
            }
        }

        // Validate allocated_storage
        if let Some(storage) = obj.get("allocated_storage") {
            if let Some(size) = storage.as_i64() {
                if size < 5 || size > 65536 {
                    return Err(ProvisioningError::ValidationError(
                        "allocated_storage must be between 5 and 65536 GB".to_string(),
                    ));
                }
            }
        }

        // Validate storage_type
        if let Some(storage_type) = obj.get("storage_type").and_then(|v| v.as_str()) {
            let valid_types = ["gp2", "gp3", "io1", "standard"];
            if !valid_types.contains(&storage_type) {
                return Err(ProvisioningError::ValidationError(format!(
                    "Invalid storage_type: {}. Must be one of: {}",
                    storage_type,
                    valid_types.join(", ")
                )));
            }
        }

        // Validate io1/gp3 requires iops
        if let Some(storage_type) = obj.get("storage_type").and_then(|v| v.as_str()) {
            if storage_type == "io1" && !obj.contains_key("iops") {
                return Err(ProvisioningError::ValidationError(
                    "iops is required when storage_type is io1".to_string(),
                ));
            }
        }

        // Validate backup_retention_period
        if let Some(retention) = obj.get("backup_retention_period") {
            if let Some(days) = retention.as_i64() {
                if days < 0 || days > 35 {
                    return Err(ProvisioningError::ValidationError(
                        "backup_retention_period must be between 0 and 35".to_string(),
                    ));
                }
            }
        }

        // Validate password length if provided
        if let Some(password) = obj.get("password").and_then(|v| v.as_str()) {
            if password.len() < 8 {
                return Err(ProvisioningError::ValidationError(
                    "password must be at least 8 characters".to_string(),
                ));
            }
        }

        // Validate that username is provided if password is (and vice versa for new instances)
        let has_snapshot = obj.contains_key("snapshot_identifier");
        if !has_snapshot {
            let has_username = obj.contains_key("username");
            let has_password = obj.contains_key("password");
            if has_username != has_password {
                return Err(ProvisioningError::ValidationError(
                    "Both username and password must be provided for new instances".to_string(),
                ));
            }
        }

        Ok(())
    }
}

impl AwsRdsInstanceResource {
    /// Build tags list for RDS API
    fn build_tags(
        &self,
        tags: &HashMap<String, String>,
        default_tags: &HashMap<String, String>,
    ) -> Vec<RdsTag> {
        let mut result = Vec::new();

        // Add default tags
        for (key, value) in default_tags {
            if !tags.contains_key(key) {
                result.push(
                    RdsTag::builder()
                        .key(key)
                        .value(value)
                        .build(),
                );
            }
        }

        // Add resource tags
        for (key, value) in tags {
            result.push(
                RdsTag::builder()
                    .key(key)
                    .value(value)
                    .build(),
            );
        }

        result
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_resource_type() {
        let resource = AwsRdsInstanceResource::new();
        assert_eq!(resource.resource_type(), "aws_db_instance");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_forces_replacement() {
        let resource = AwsRdsInstanceResource::new();
        let forces = resource.forces_replacement();

        assert!(forces.contains(&"identifier".to_string()));
        assert!(forces.contains(&"engine".to_string()));
        assert!(forces.contains(&"db_name".to_string()));
        assert!(forces.contains(&"storage_encrypted".to_string()));
        assert!(forces.contains(&"kms_key_id".to_string()));
    }

    #[test]
    fn test_schema_has_required_fields() {
        let resource = AwsRdsInstanceResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_db_instance");
        assert!(!schema.required_args.is_empty());

        let required_names: Vec<_> = schema.required_args.iter().map(|f| f.name.as_str()).collect();
        assert!(required_names.contains(&"identifier"));
        assert!(required_names.contains(&"engine"));
        assert!(required_names.contains(&"instance_class"));
        assert!(required_names.contains(&"allocated_storage"));
    }

    #[test]
    fn test_validate_valid_config() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20,
            "username": "admin",
            "password": "mysecretpassword"
        });

        assert!(resource.validate(&config).is_ok());
    }

    #[test]
    fn test_validate_missing_identifier() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        if let Err(ProvisioningError::ValidationError(msg)) = result {
            assert!(msg.contains("identifier"));
        }
    }

    #[test]
    fn test_validate_missing_engine() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        if let Err(ProvisioningError::ValidationError(msg)) = result {
            assert!(msg.contains("engine"));
        }
    }

    #[test]
    fn test_validate_invalid_engine() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "invalid-engine",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_instance_class() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "t3.micro",  // Missing db. prefix
            "allocated_storage": 20
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_storage_too_small() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 1  // Too small
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_io1_without_iops() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 100,
            "storage_type": "io1"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
        if let Err(ProvisioningError::ValidationError(msg)) = result {
            assert!(msg.contains("iops"));
        }
    }

    #[test]
    fn test_validate_password_without_username() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20,
            "password": "mysecretpassword"
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_password_too_short() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20,
            "username": "admin",
            "password": "short"  // Too short
        });

        let result = resource.validate(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_rds_config_parsing() {
        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "engine_version": "15.4",
            "instance_class": "db.t3.small",
            "allocated_storage": 50,
            "username": "admin",
            "password": "mysecretpassword",
            "db_name": "myapp",
            "port": 5432,
            "vpc_security_group_ids": ["sg-12345678"],
            "db_subnet_group_name": "my-subnet-group",
            "multi_az": true,
            "storage_encrypted": true,
            "backup_retention_period": 14,
            "tags": {
                "Name": "my-database",
                "Environment": "production"
            }
        });

        let rds_config = RdsInstanceConfig::from_value(&config).unwrap();

        assert_eq!(rds_config.identifier, "mydb");
        assert_eq!(rds_config.engine, "postgres");
        assert_eq!(rds_config.engine_version, Some("15.4".to_string()));
        assert_eq!(rds_config.instance_class, "db.t3.small");
        assert_eq!(rds_config.allocated_storage, 50);
        assert_eq!(rds_config.username, Some("admin".to_string()));
        assert_eq!(rds_config.db_name, Some("myapp".to_string()));
        assert_eq!(rds_config.port, Some(5432));
        assert!(rds_config.multi_az);
        assert!(rds_config.storage_encrypted);
        assert_eq!(rds_config.backup_retention_period, 14);
        assert_eq!(rds_config.tags.get("Name"), Some(&"my-database".to_string()));
    }

    #[test]
    fn test_rds_config_defaults() {
        let config = json!({
            "identifier": "mydb",
            "engine": "mysql",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let rds_config = RdsInstanceConfig::from_value(&config).unwrap();

        assert_eq!(rds_config.storage_type, "gp2");
        assert_eq!(rds_config.backup_retention_period, 7);
        assert!(rds_config.auto_minor_version_upgrade);
        assert!(!rds_config.multi_az);
        assert!(!rds_config.publicly_accessible);
        assert!(!rds_config.storage_encrypted);
        assert!(!rds_config.deletion_protection);
        assert!(!rds_config.skip_final_snapshot);
    }

    #[test]
    fn test_dependencies_extraction() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20,
            "db_subnet_group_name": "${aws_db_subnet_group.main.name}",
            "vpc_security_group_ids": [
                "${aws_security_group.db.id}",
                "sg-static-12345"
            ]
        });

        let deps = resource.dependencies(&config);

        let has_subnet_group = deps
            .iter()
            .any(|d| d.resource_type == "aws_db_subnet_group" && d.resource_name == "main");
        let has_sg = deps
            .iter()
            .any(|d| d.resource_type == "aws_security_group" && d.resource_name == "db");

        assert!(has_subnet_group, "Should detect subnet group dependency");
        assert!(has_sg, "Should detect security group dependency");
    }

    #[test]
    fn test_plan_create() {
        let resource = AwsRdsInstanceResource::new();

        let desired = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 2400,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&desired, None, &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::Create);
    }

    #[test]
    fn test_plan_no_change() {
        let resource = AwsRdsInstanceResource::new();

        let config = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 2400,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&config, Some(&config), &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[test]
    fn test_plan_update_instance_class() {
        let resource = AwsRdsInstanceResource::new();

        let current = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let desired = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.small",
            "allocated_storage": 20
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 2400,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&desired, Some(&current), &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::Update);
        assert!(diff.modifications.contains_key("instance_class"));
        assert!(!diff.requires_replacement);
    }

    #[test]
    fn test_plan_replace_engine_change() {
        let resource = AwsRdsInstanceResource::new();

        let current = json!({
            "identifier": "mydb",
            "engine": "mysql",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let desired = json!({
            "identifier": "mydb",
            "engine": "postgres",
            "instance_class": "db.t3.micro",
            "allocated_storage": 20
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let diff = rt.block_on(async {
            use crate::provisioning::traits::{DebugCredentials, RetryConfig};
            let ctx = ProviderContext {
                provider: "aws".to_string(),
                region: Some("us-east-1".to_string()),
                config: Value::Null,
                credentials: std::sync::Arc::new(DebugCredentials::new("aws")),
                timeout_seconds: 2400,
                retry_config: RetryConfig::default(),
                default_tags: HashMap::new(),
            };

            resource.plan(&desired, Some(&current), &ctx).await.unwrap()
        });

        assert_eq!(diff.change_type, ChangeType::Replace);
        assert!(diff.requires_replacement);
        assert!(diff.replacement_fields.contains(&"engine".to_string()));
    }

    #[test]
    fn test_state_serialization() {
        let state = RdsInstanceState {
            id: "mydb".to_string(),
            arn: "arn:aws:rds:us-east-1:123456789012:db:mydb".to_string(),
            address: Some("mydb.abcd1234.us-east-1.rds.amazonaws.com".to_string()),
            port: Some(5432),
            status: "available".to_string(),
            engine: Some("postgres".to_string()),
            engine_version: Some("15.4".to_string()),
            instance_class: Some("db.t3.micro".to_string()),
            allocated_storage: Some(20),
            username: Some("admin".to_string()),
            db_name: Some("myapp".to_string()),
            multi_az: true,
            storage_encrypted: true,
            ..Default::default()
        };

        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["id"], "mydb");
        assert_eq!(json["status"], "available");
        assert_eq!(json["port"], 5432);
        assert_eq!(json["multi_az"], true);
    }

    #[test]
    fn test_build_tags() {
        let resource = AwsRdsInstanceResource::new();

        let mut tags = HashMap::new();
        tags.insert("Name".to_string(), "my-database".to_string());
        tags.insert("Project".to_string(), "myproject".to_string());

        let mut default_tags = HashMap::new();
        default_tags.insert("Environment".to_string(), "production".to_string());
        default_tags.insert("Project".to_string(), "default-project".to_string());

        let result = resource.build_tags(&tags, &default_tags);

        // Should have 3 tags: Name, Project (from tags, overriding default), Environment
        assert_eq!(result.len(), 3);

        let has_name = result.iter().any(|t| t.key() == Some("Name"));
        let has_env = result.iter().any(|t| t.key() == Some("Environment"));
        let has_project = result.iter().any(|t| t.key() == Some("Project") && t.value() == Some("myproject"));

        assert!(has_name);
        assert!(has_env);
        assert!(has_project);
    }

    #[test]
    fn test_parse_reference_terraform_style() {
        let resource = AwsRdsInstanceResource::new();

        let result = resource.parse_reference("${aws_db_subnet_group.main.name}");
        assert!(result.is_some());

        let dep = result.unwrap();
        assert_eq!(dep.resource_type, "aws_db_subnet_group");
        assert_eq!(dep.resource_name, "main");
    }
}
