//! AWS Route Table Resource for infrastructure provisioning
//!
//! This module implements the `Resource` trait for AWS Route Tables, enabling declarative
//! Route Table management through the provisioning system.
//!
//! ## Example
//!
//! ```yaml
//! resources:
//!   aws_route_table:
//!     public:
//!       vpc_id: "{{ resources.aws_vpc.main.id }}"
//!       routes:
//!         - cidr_block: "0.0.0.0/0"
//!           gateway_id: "{{ resources.aws_internet_gateway.main.id }}"
//!       tags:
//!         Name: public-route-table
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
use aws_sdk_ec2::types::{Filter, ResourceType, Tag, TagSpecification};
#[cfg(feature = "aws")]
use aws_sdk_ec2::Client;

use crate::provisioning::error::{ProvisioningError, ProvisioningResult};
use crate::provisioning::traits::{
    ChangeType, FieldConstraint, FieldType, ProviderContext, Resource, ResourceDependency,
    ResourceDiff, ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts,
    SchemaField,
};

// ============================================================================
// Route Table Resource Configuration
// ============================================================================

/// Route configuration for a route table
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteConfig {
    /// Destination CIDR block for the route
    pub cidr_block: Option<String>,
    /// Destination IPv6 CIDR block for the route
    pub ipv6_cidr_block: Option<String>,
    /// Destination prefix list ID
    pub destination_prefix_list_id: Option<String>,
    /// Internet Gateway ID for the route target
    pub gateway_id: Option<String>,
    /// NAT Gateway ID for the route target
    pub nat_gateway_id: Option<String>,
    /// VPC Peering Connection ID for the route target
    pub vpc_peering_connection_id: Option<String>,
    /// Transit Gateway ID for the route target
    pub transit_gateway_id: Option<String>,
    /// Network Interface ID for the route target
    pub network_interface_id: Option<String>,
    /// Instance ID for the route target
    pub instance_id: Option<String>,
    /// VPC Endpoint ID for the route target
    pub vpc_endpoint_id: Option<String>,
    /// Egress-only Internet Gateway ID for IPv6 routes
    pub egress_only_gateway_id: Option<String>,
}

impl RouteConfig {
    /// Parse route from JSON value
    pub fn from_value(value: &Value) -> ProvisioningResult<Self> {
        Ok(Self {
            cidr_block: value
                .get("cidr_block")
                .and_then(|v| v.as_str())
                .map(String::from),
            ipv6_cidr_block: value
                .get("ipv6_cidr_block")
                .and_then(|v| v.as_str())
                .map(String::from),
            destination_prefix_list_id: value
                .get("destination_prefix_list_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            gateway_id: value
                .get("gateway_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            nat_gateway_id: value
                .get("nat_gateway_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            vpc_peering_connection_id: value
                .get("vpc_peering_connection_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            transit_gateway_id: value
                .get("transit_gateway_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            network_interface_id: value
                .get("network_interface_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            instance_id: value
                .get("instance_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            vpc_endpoint_id: value
                .get("vpc_endpoint_id")
                .and_then(|v| v.as_str())
                .map(String::from),
            egress_only_gateway_id: value
                .get("egress_only_gateway_id")
                .and_then(|v| v.as_str())
                .map(String::from),
        })
    }

    /// Validate route configuration
    pub fn validate(&self) -> ProvisioningResult<()> {
        // Must have at least one destination
        if self.cidr_block.is_none()
            && self.ipv6_cidr_block.is_none()
            && self.destination_prefix_list_id.is_none()
        {
            return Err(ProvisioningError::ValidationError(
                "Route must have cidr_block, ipv6_cidr_block, or destination_prefix_list_id"
                    .to_string(),
            ));
        }

        // Must have exactly one target
        let targets = [
            self.gateway_id.is_some(),
            self.nat_gateway_id.is_some(),
            self.vpc_peering_connection_id.is_some(),
            self.transit_gateway_id.is_some(),
            self.network_interface_id.is_some(),
            self.instance_id.is_some(),
            self.vpc_endpoint_id.is_some(),
            self.egress_only_gateway_id.is_some(),
        ];
        let target_count = targets.iter().filter(|&&x| x).count();

        if target_count == 0 {
            return Err(ProvisioningError::ValidationError(
                "Route must have a target (gateway_id, nat_gateway_id, etc.)".to_string(),
            ));
        }
        if target_count > 1 {
            return Err(ProvisioningError::ValidationError(
                "Route can only have one target".to_string(),
            ));
        }

        Ok(())
    }
}

/// Route Table resource attributes (computed from cloud)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteTableAttributes {
    /// Route Table ID (e.g., rtb-12345678)
    pub id: String,
    /// Route Table ARN
    pub arn: String,
    /// VPC ID the route table belongs to
    pub vpc_id: String,
    /// Owner ID (AWS account ID)
    pub owner_id: String,
    /// Routes in the table
    pub routes: Vec<RouteConfig>,
    /// Associated subnets
    pub associations: Vec<RouteTableAssociation>,
    /// Whether this is the main route table
    pub main: bool,
    /// Tags
    pub tags: HashMap<String, String>,
}

/// Route table association information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteTableAssociation {
    /// Association ID
    pub id: String,
    /// Subnet ID (if subnet association)
    pub subnet_id: Option<String>,
    /// Gateway ID (if gateway association)
    pub gateway_id: Option<String>,
    /// Whether this is the main association
    pub main: bool,
}

/// Route Table configuration
#[derive(Debug, Clone)]
pub struct RouteTableConfig {
    pub vpc_id: String,
    pub routes: Vec<RouteConfig>,
    pub tags: HashMap<String, String>,
}

// ============================================================================
// AWS Route Table Resource Implementation
// ============================================================================

/// AWS Route Table resource for infrastructure provisioning
///
/// This resource manages AWS Route Tables, which contain routing rules
/// that determine where network traffic is directed.
#[derive(Debug, Clone, Default)]
pub struct AwsRouteTableResource;

impl AwsRouteTableResource {
    /// Create a new Route Table resource
    pub fn new() -> Self {
        Self
    }

    /// Build the resource schema
    fn build_schema() -> ResourceSchema {
        ResourceSchema {
            resource_type: "aws_route_table".to_string(),
            description: "AWS Route Table for VPC routing".to_string(),
            required_args: vec![SchemaField {
                name: "vpc_id".to_string(),
                field_type: FieldType::String,
                description: "The VPC ID to create the route table in".to_string(),
                default: None,
                constraints: vec![FieldConstraint::Pattern {
                    regex: r"^vpc-[a-f0-9]+$".to_string(),
                }],
                sensitive: false,
            }],
            optional_args: vec![
                SchemaField {
                    name: "routes".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::Any)),
                    description: "List of route configurations".to_string(),
                    default: Some(Value::Array(vec![])),
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "tags".to_string(),
                    field_type: FieldType::Map(Box::new(FieldType::String)),
                    description: "Tags to apply to the route table".to_string(),
                    default: Some(Value::Object(Default::default())),
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            computed_attrs: vec![
                SchemaField {
                    name: "id".to_string(),
                    field_type: FieldType::String,
                    description: "Route Table ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "arn".to_string(),
                    field_type: FieldType::String,
                    description: "Route Table ARN".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "owner_id".to_string(),
                    field_type: FieldType::String,
                    description: "Owner account ID".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
                SchemaField {
                    name: "associations".to_string(),
                    field_type: FieldType::List(Box::new(FieldType::Any)),
                    description: "Route table associations".to_string(),
                    default: None,
                    constraints: vec![],
                    sensitive: false,
                },
            ],
            force_new: vec!["vpc_id".to_string()],
            timeouts: ResourceTimeouts {
                create: 120,
                read: 60,
                update: 120,
                delete: 120,
            },
        }
    }

    /// Extract configuration values from JSON
    fn extract_config(config: &Value) -> ProvisioningResult<RouteTableConfig> {
        let vpc_id = config
            .get("vpc_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProvisioningError::ValidationError("vpc_id is required".to_string()))?
            .to_string();

        let routes = if let Some(routes_value) = config.get("routes") {
            if let Some(arr) = routes_value.as_array() {
                arr.iter()
                    .map(RouteConfig::from_value)
                    .collect::<ProvisioningResult<Vec<_>>>()?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

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

        Ok(RouteTableConfig {
            vpc_id,
            routes,
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

    /// Read Route Table by ID from AWS
    #[cfg(feature = "aws")]
    async fn read_route_table_by_id(
        client: &Client,
        route_table_id: &str,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<Option<RouteTableAttributes>> {
        let resp = client
            .describe_route_tables()
            .route_table_ids(route_table_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to describe route table: {}", e))
            })?;

        if let Some(rt) = resp.route_tables().first() {
            let rt_id = rt.route_table_id().unwrap_or_default().to_string();
            let vpc_id = rt.vpc_id().unwrap_or_default().to_string();
            let owner_id = rt.owner_id().unwrap_or_default().to_string();

            // Extract routes (excluding local route)
            let routes: Vec<RouteConfig> = rt
                .routes()
                .iter()
                .filter(|r| r.gateway_id() != Some("local"))
                .map(|r| RouteConfig {
                    cidr_block: r.destination_cidr_block().map(String::from),
                    ipv6_cidr_block: r.destination_ipv6_cidr_block().map(String::from),
                    destination_prefix_list_id: r.destination_prefix_list_id().map(String::from),
                    gateway_id: r.gateway_id().map(String::from),
                    nat_gateway_id: r.nat_gateway_id().map(String::from),
                    vpc_peering_connection_id: r.vpc_peering_connection_id().map(String::from),
                    transit_gateway_id: r.transit_gateway_id().map(String::from),
                    network_interface_id: r.network_interface_id().map(String::from),
                    instance_id: r.instance_id().map(String::from),
                    vpc_endpoint_id: r
                        .gateway_id()
                        .filter(|g| g.starts_with("vpce-"))
                        .map(String::from),
                    egress_only_gateway_id: r.egress_only_internet_gateway_id().map(String::from),
                })
                .collect();

            // Extract associations
            let associations: Vec<RouteTableAssociation> = rt
                .associations()
                .iter()
                .map(|a| RouteTableAssociation {
                    id: a
                        .route_table_association_id()
                        .unwrap_or_default()
                        .to_string(),
                    subnet_id: a.subnet_id().map(String::from),
                    gateway_id: a.gateway_id().map(String::from),
                    main: a.main().unwrap_or(false),
                })
                .collect();

            let main = associations.iter().any(|a| a.main);

            // Extract tags
            let mut tags = HashMap::new();
            for tag in rt.tags() {
                if let (Some(key), Some(value)) = (tag.key(), tag.value()) {
                    tags.insert(key.to_string(), value.to_string());
                }
            }

            // Build ARN
            let region = ctx.region.as_deref().unwrap_or("us-east-1");
            let arn = format!("arn:aws:ec2:{}:{}:route-table/{}", region, owner_id, rt_id);

            Ok(Some(RouteTableAttributes {
                id: rt_id,
                arn,
                vpc_id,
                owner_id,
                routes,
                associations,
                main,
                tags,
            }))
        } else {
            Ok(None)
        }
    }

    /// Create Route Table in AWS
    #[cfg(feature = "aws")]
    async fn create_route_table(
        client: &Client,
        config: &RouteTableConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<RouteTableAttributes> {
        // Build tags including default tags from context
        let mut all_tags: Vec<Tag> = ctx
            .default_tags
            .iter()
            .map(|(k, v)| Tag::builder().key(k).value(v).build())
            .collect();

        for (k, v) in &config.tags {
            all_tags.push(Tag::builder().key(k).value(v).build());
        }

        // Create route table
        let resp = client
            .create_route_table()
            .vpc_id(&config.vpc_id)
            .tag_specifications(
                TagSpecification::builder()
                    .resource_type(ResourceType::RouteTable)
                    .set_tags(Some(all_tags))
                    .build(),
            )
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to create route table: {}", e))
            })?;

        let rt = resp.route_table().ok_or_else(|| {
            ProvisioningError::CloudApiError("No route table returned from create".to_string())
        })?;

        let rt_id = rt.route_table_id().unwrap_or_default().to_string();

        // Add routes
        for route in &config.routes {
            Self::create_route(client, &rt_id, route).await?;
        }

        // Read the full route table attributes
        Self::read_route_table_by_id(client, &rt_id, ctx)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read created route table".to_string())
            })
    }

    /// Create a route in a route table
    #[cfg(feature = "aws")]
    async fn create_route(
        client: &Client,
        route_table_id: &str,
        route: &RouteConfig,
    ) -> ProvisioningResult<()> {
        let mut req = client.create_route().route_table_id(route_table_id);

        if let Some(ref cidr) = route.cidr_block {
            req = req.destination_cidr_block(cidr);
        }
        if let Some(ref cidr) = route.ipv6_cidr_block {
            req = req.destination_ipv6_cidr_block(cidr);
        }
        if let Some(ref pl) = route.destination_prefix_list_id {
            req = req.destination_prefix_list_id(pl);
        }
        if let Some(ref gw) = route.gateway_id {
            req = req.gateway_id(gw);
        }
        if let Some(ref nat) = route.nat_gateway_id {
            req = req.nat_gateway_id(nat);
        }
        if let Some(ref vpc_peer) = route.vpc_peering_connection_id {
            req = req.vpc_peering_connection_id(vpc_peer);
        }
        if let Some(ref tgw) = route.transit_gateway_id {
            req = req.transit_gateway_id(tgw);
        }
        if let Some(ref eni) = route.network_interface_id {
            req = req.network_interface_id(eni);
        }
        if let Some(ref inst) = route.instance_id {
            req = req.instance_id(inst);
        }
        if let Some(ref vpce) = route.vpc_endpoint_id {
            req = req.vpc_endpoint_id(vpce);
        }
        if let Some(ref eigw) = route.egress_only_gateway_id {
            req = req.egress_only_internet_gateway_id(eigw);
        }

        req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to create route: {}", e))
        })?;

        Ok(())
    }

    /// Delete a route from a route table
    #[cfg(feature = "aws")]
    async fn delete_route(
        client: &Client,
        route_table_id: &str,
        route: &RouteConfig,
    ) -> ProvisioningResult<()> {
        let mut req = client.delete_route().route_table_id(route_table_id);

        if let Some(ref cidr) = route.cidr_block {
            req = req.destination_cidr_block(cidr);
        }
        if let Some(ref cidr) = route.ipv6_cidr_block {
            req = req.destination_ipv6_cidr_block(cidr);
        }
        if let Some(ref pl) = route.destination_prefix_list_id {
            req = req.destination_prefix_list_id(pl);
        }

        req.send().await.map_err(|e| {
            ProvisioningError::CloudApiError(format!("Failed to delete route: {}", e))
        })?;

        Ok(())
    }

    /// Update Route Table in AWS
    #[cfg(feature = "aws")]
    async fn update_route_table(
        client: &Client,
        rt_id: &str,
        old_config: &RouteTableConfig,
        new_config: &RouteTableConfig,
        ctx: &ProviderContext,
    ) -> ProvisioningResult<RouteTableAttributes> {
        // Update routes: delete removed routes, create new routes
        for old_route in &old_config.routes {
            if !new_config.routes.contains(old_route) {
                Self::delete_route(client, rt_id, old_route).await?;
            }
        }

        for new_route in &new_config.routes {
            if !old_config.routes.contains(new_route) {
                Self::create_route(client, rt_id, new_route).await?;
            }
        }

        // Update tags if changed
        if old_config.tags != new_config.tags {
            // Delete old tags that are no longer present
            let tags_to_delete: Vec<_> = old_config
                .tags
                .keys()
                .filter(|k| !new_config.tags.contains_key(*k))
                .map(|k| Tag::builder().key(k).build())
                .collect();

            if !tags_to_delete.is_empty() {
                client
                    .delete_tags()
                    .resources(rt_id)
                    .set_tags(Some(tags_to_delete))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to delete tags: {}", e))
                    })?;
            }

            // Create/update tags
            let tags_to_create: Vec<_> = new_config
                .tags
                .iter()
                .map(|(k, v)| Tag::builder().key(k).value(v).build())
                .collect();

            if !tags_to_create.is_empty() {
                client
                    .create_tags()
                    .resources(rt_id)
                    .set_tags(Some(tags_to_create))
                    .send()
                    .await
                    .map_err(|e| {
                        ProvisioningError::CloudApiError(format!("Failed to create tags: {}", e))
                    })?;
            }
        }

        // Read updated route table
        Self::read_route_table_by_id(client, rt_id, ctx)
            .await?
            .ok_or_else(|| {
                ProvisioningError::CloudApiError("Failed to read updated route table".to_string())
            })
    }

    /// Delete Route Table in AWS
    #[cfg(feature = "aws")]
    async fn delete_route_table(client: &Client, rt_id: &str) -> ProvisioningResult<()> {
        client
            .delete_route_table()
            .route_table_id(rt_id)
            .send()
            .await
            .map_err(|e| {
                ProvisioningError::CloudApiError(format!("Failed to delete route table: {}", e))
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

        if let (Some(desired_obj), Some(current_obj)) = (desired.as_object(), current.as_object()) {
            for (key, desired_val) in desired_obj {
                if ["id", "arn", "owner_id", "associations", "main"].contains(&key.as_str()) {
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
                if ["id", "arn", "owner_id", "associations", "main"].contains(&key.as_str()) {
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

        // VPC dependency
        if let Some(vpc_id) = config.get("vpc_id").and_then(|v| v.as_str()) {
            if vpc_id.contains("resources.aws_vpc.") {
                if let Some(name) = vpc_id
                    .strip_prefix("{{ resources.aws_vpc.")
                    .and_then(|s| s.strip_suffix(".id }}"))
                {
                    deps.push(ResourceDependency {
                        resource_type: "aws_vpc".to_string(),
                        resource_name: name.to_string(),
                        attribute: "id".to_string(),
                        hard: true,
                    });
                }
            }
        }

        // Route dependencies
        if let Some(routes) = config.get("routes").and_then(|v| v.as_array()) {
            for route in routes {
                // Gateway ID dependency
                if let Some(gw_id) = route.get("gateway_id").and_then(|v| v.as_str()) {
                    if gw_id.contains("resources.aws_internet_gateway.") {
                        if let Some(name) = gw_id
                            .strip_prefix("{{ resources.aws_internet_gateway.")
                            .and_then(|s| s.strip_suffix(".id }}"))
                        {
                            deps.push(ResourceDependency {
                                resource_type: "aws_internet_gateway".to_string(),
                                resource_name: name.to_string(),
                                attribute: "id".to_string(),
                                hard: true,
                            });
                        }
                    }
                }

                // NAT Gateway ID dependency
                if let Some(nat_id) = route.get("nat_gateway_id").and_then(|v| v.as_str()) {
                    if nat_id.contains("resources.aws_nat_gateway.") {
                        if let Some(name) = nat_id
                            .strip_prefix("{{ resources.aws_nat_gateway.")
                            .and_then(|s| s.strip_suffix(".id }}"))
                        {
                            deps.push(ResourceDependency {
                                resource_type: "aws_nat_gateway".to_string(),
                                resource_name: name.to_string(),
                                attribute: "id".to_string(),
                                hard: true,
                            });
                        }
                    }
                }
            }
        }

        deps
    }
}

#[async_trait]
impl Resource for AwsRouteTableResource {
    fn resource_type(&self) -> &str {
        "aws_route_table"
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

        match Self::read_route_table_by_id(&client, id, ctx).await? {
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
        let rt_config = Self::extract_config(config)?;
        let client = Self::create_client(ctx).await?;

        match Self::create_route_table(&client, &rt_config, ctx).await {
            Ok(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;

                let mut result = ResourceResult::success(&attrs.id, attributes);
                result
                    .outputs
                    .insert("id".to_string(), Value::String(attrs.id.clone()));
                result
                    .outputs
                    .insert("arn".to_string(), Value::String(attrs.arn));
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

        match Self::update_route_table(&client, id, &old_config, &new_config, ctx).await {
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

        match Self::delete_route_table(&client, id).await {
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

        match Self::read_route_table_by_id(&client, id, ctx).await? {
            Some(attrs) => {
                let attributes = serde_json::to_value(&attrs)
                    .map_err(|e| ProvisioningError::SerializationError(e.to_string()))?;
                Ok(ResourceResult::success(id, attributes))
            }
            None => Err(ProvisioningError::ImportError {
                resource_type: "aws_route_table".to_string(),
                resource_id: id.to_string(),
                message: "Route table not found".to_string(),
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
        vec!["vpc_id".to_string()]
    }

    fn validate(&self, config: &Value) -> ProvisioningResult<()> {
        // Validate required vpc_id
        config
            .get("vpc_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ProvisioningError::ValidationError("vpc_id is required".to_string()))?;

        // Validate routes if provided
        if let Some(routes) = config.get("routes").and_then(|v| v.as_array()) {
            for route in routes {
                let route_config = RouteConfig::from_value(route)?;
                route_config.validate()?;
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
        let resource = AwsRouteTableResource::new();
        assert_eq!(resource.resource_type(), "aws_route_table");
        assert_eq!(resource.provider(), "aws");
    }

    #[test]
    fn test_schema() {
        let resource = AwsRouteTableResource::new();
        let schema = resource.schema();

        assert_eq!(schema.resource_type, "aws_route_table");
        assert_eq!(schema.required_args.len(), 1);
        assert_eq!(schema.required_args[0].name, "vpc_id");
        assert_eq!(schema.optional_args.len(), 2);
        assert_eq!(schema.force_new, vec!["vpc_id"]);
    }

    #[test]
    fn test_route_config_validation() {
        // Valid route
        let route = RouteConfig {
            cidr_block: Some("0.0.0.0/0".to_string()),
            gateway_id: Some("igw-12345".to_string()),
            ..Default::default()
        };
        assert!(route.validate().is_ok());

        // Missing destination
        let route = RouteConfig {
            gateway_id: Some("igw-12345".to_string()),
            ..Default::default()
        };
        assert!(route.validate().is_err());

        // Missing target
        let route = RouteConfig {
            cidr_block: Some("0.0.0.0/0".to_string()),
            ..Default::default()
        };
        assert!(route.validate().is_err());

        // Multiple targets
        let route = RouteConfig {
            cidr_block: Some("0.0.0.0/0".to_string()),
            gateway_id: Some("igw-12345".to_string()),
            nat_gateway_id: Some("nat-12345".to_string()),
            ..Default::default()
        };
        assert!(route.validate().is_err());
    }

    #[test]
    fn test_dependencies_extraction() {
        let config = serde_json::json!({
            "vpc_id": "{{ resources.aws_vpc.main.id }}",
            "routes": [{
                "cidr_block": "0.0.0.0/0",
                "gateway_id": "{{ resources.aws_internet_gateway.main.id }}"
            }]
        });

        let deps = AwsRouteTableResource::extract_dependencies(&config);
        assert_eq!(deps.len(), 2);
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_vpc" && d.resource_name == "main"));
        assert!(deps
            .iter()
            .any(|d| d.resource_type == "aws_internet_gateway" && d.resource_name == "main"));
    }

    #[test]
    fn test_validate_config() {
        let resource = AwsRouteTableResource::new();

        // Valid config
        let config = serde_json::json!({
            "vpc_id": "vpc-12345",
            "routes": [{
                "cidr_block": "0.0.0.0/0",
                "gateway_id": "igw-12345"
            }],
            "tags": {
                "Name": "test-rt"
            }
        });
        assert!(resource.validate(&config).is_ok());

        // Missing vpc_id
        let config = serde_json::json!({
            "routes": []
        });
        assert!(resource.validate(&config).is_err());
    }

    #[test]
    fn test_compute_diff_create() {
        let desired = serde_json::json!({
            "vpc_id": "vpc-12345",
            "routes": []
        });

        let diff = AwsRouteTableResource::compute_diff(&desired, None, &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::Create);
    }

    #[test]
    fn test_compute_diff_no_change() {
        let desired = serde_json::json!({
            "vpc_id": "vpc-12345",
            "routes": []
        });

        let current = serde_json::json!({
            "vpc_id": "vpc-12345",
            "routes": [],
            "id": "rtb-12345"
        });

        let diff = AwsRouteTableResource::compute_diff(&desired, Some(&current), &[]).unwrap();
        assert_eq!(diff.change_type, ChangeType::NoOp);
    }

    #[test]
    fn test_compute_diff_replace() {
        let desired = serde_json::json!({
            "vpc_id": "vpc-67890",
            "routes": []
        });

        let current = serde_json::json!({
            "vpc_id": "vpc-12345",
            "routes": [],
            "id": "rtb-12345"
        });

        let force_new = vec!["vpc_id".to_string()];
        let diff =
            AwsRouteTableResource::compute_diff(&desired, Some(&current), &force_new).unwrap();
        assert_eq!(diff.change_type, ChangeType::Replace);
        assert!(diff.requires_replacement);
    }
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            cidr_block: None,
            ipv6_cidr_block: None,
            destination_prefix_list_id: None,
            gateway_id: None,
            nat_gateway_id: None,
            vpc_peering_connection_id: None,
            transit_gateway_id: None,
            network_interface_id: None,
            instance_id: None,
            vpc_endpoint_id: None,
            egress_only_gateway_id: None,
        }
    }
}
