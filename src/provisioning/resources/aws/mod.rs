//! AWS Resource Implementations
//!
//! This module contains AWS resource implementations for the provisioning system.
//! Each resource implements the `Resource` trait for declarative infrastructure management.
//!
//! # Available Resources
//!
//! - `aws_instance` - EC2 Instances
//! - `aws_security_group` - EC2 Security Groups
//! - `aws_subnet` - VPC Subnets
//! - `aws_vpc` - Virtual Private Clouds

pub mod instance;
pub mod security_group;
pub mod subnet;
pub mod vpc;

pub use instance::{AwsInstanceResource, InstanceAttributes, InstanceConfig, RootBlockDevice};
pub use security_group::AwsSecurityGroupResource;
pub use subnet::AwsSubnetResource;
pub use vpc::AwsVpcResource;
