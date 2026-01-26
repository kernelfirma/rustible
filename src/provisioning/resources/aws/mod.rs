//! AWS Resource Implementations
//!
//! This module contains AWS resource implementations for the provisioning system.
//! Each resource implements the `Resource` trait for declarative infrastructure management.
//!
//! # Available Resources
//!
//! - `aws_db_subnet_group` - RDS DB Subnet Groups
//! - `aws_ebs_volume` - EBS Volumes
//! - `aws_eip` - Elastic IPs
//! - `aws_iam_policy` - IAM Policies
//! - `aws_iam_role` - IAM Roles
//! - `aws_instance` - EC2 Instances
//! - `aws_internet_gateway` - Internet Gateways
//! - `aws_nat_gateway` - NAT Gateways
//! - `aws_route_table` - Route Tables
//! - `aws_security_group` - EC2 Security Groups
//! - `aws_security_group_rule` - Individual Security Group Rules
//! - `aws_subnet` - VPC Subnets
//! - `aws_vpc` - Virtual Private Clouds

pub mod db_subnet_group;
pub mod ebs_volume;
pub mod elastic_ip;
pub mod iam_policy;
pub mod iam_role;
pub mod instance;
pub mod internet_gateway;
pub mod load_balancer;
pub mod nat_gateway;
pub mod rds_instance;
pub mod route_table;
pub mod s3_bucket;
pub mod security_group;
pub mod security_group_rule;
pub mod subnet;
pub mod vpc;

pub use db_subnet_group::{AwsDbSubnetGroupResource, DbSubnetGroupConfig, DbSubnetGroupState};
pub use ebs_volume::{AwsEbsVolumeResource, EbsVolumeConfig, EbsVolumeState};
pub use elastic_ip::{AwsElasticIpResource, ElasticIpAttributes, ElasticIpConfig};
pub use iam_policy::{AwsIamPolicyResource, IamPolicyAttributes, IamPolicyConfig};
pub use iam_role::{AwsIamRoleResource, IamRoleAttributes, IamRoleConfig};
pub use instance::{AwsInstanceResource, InstanceAttributes, InstanceConfig, RootBlockDevice};
pub use internet_gateway::{
    AwsInternetGatewayResource, InternetGatewayAttributes, InternetGatewayConfig,
};
pub use load_balancer::{AwsLoadBalancerResource, LoadBalancerConfig, LoadBalancerState};
pub use nat_gateway::{AwsNatGatewayResource, NatGatewayAttributes, NatGatewayConfig};
pub use route_table::{
    AwsRouteTableResource, RouteConfig, RouteTableAssociation, RouteTableAttributes,
    RouteTableConfig,
};
pub use rds_instance::{AwsRdsInstanceResource, RdsInstanceConfig, RdsInstanceState};
pub use s3_bucket::{AwsS3BucketResource, S3BucketConfig, S3BucketState};
pub use security_group::AwsSecurityGroupResource;
pub use security_group_rule::{
    AwsSecurityGroupRuleResource, RuleType, SecurityGroupRuleConfig, SecurityGroupRuleState,
};
pub use subnet::AwsSubnetResource;
pub use vpc::AwsVpcResource;
