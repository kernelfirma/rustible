//! AWS Resource Implementations
//!
//! This module contains AWS resource implementations for the provisioning system.
//! Each resource implements the `Resource` trait for declarative infrastructure management.
//!
//! # Available Resources
//!
//! - `aws_eip` - Elastic IPs
//! - `aws_iam_policy` - IAM Policies
//! - `aws_iam_role` - IAM Roles
//! - `aws_instance` - EC2 Instances
//! - `aws_internet_gateway` - Internet Gateways
//! - `aws_nat_gateway` - NAT Gateways
//! - `aws_route_table` - Route Tables
//! - `aws_security_group` - EC2 Security Groups
//! - `aws_subnet` - VPC Subnets
//! - `aws_vpc` - Virtual Private Clouds

pub mod elastic_ip;
pub mod iam_policy;
pub mod iam_role;
pub mod instance;
pub mod internet_gateway;
pub mod nat_gateway;
pub mod route_table;
pub mod security_group;
pub mod subnet;
pub mod vpc;

pub use elastic_ip::{AwsElasticIpResource, ElasticIpAttributes, ElasticIpConfig};
pub use iam_policy::{AwsIamPolicyResource, IamPolicyAttributes, IamPolicyConfig};
pub use iam_role::{AwsIamRoleResource, IamRoleAttributes, IamRoleConfig};
pub use instance::{AwsInstanceResource, InstanceAttributes, InstanceConfig, RootBlockDevice};
pub use internet_gateway::{AwsInternetGatewayResource, InternetGatewayAttributes, InternetGatewayConfig};
pub use nat_gateway::{AwsNatGatewayResource, NatGatewayAttributes, NatGatewayConfig};
pub use route_table::{AwsRouteTableResource, RouteConfig, RouteTableAssociation, RouteTableAttributes, RouteTableConfig};
pub use security_group::AwsSecurityGroupResource;
pub use subnet::AwsSubnetResource;
pub use vpc::AwsVpcResource;
