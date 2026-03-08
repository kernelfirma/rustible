//! AWS (Amazon Web Services) modules for cloud infrastructure management.
//!
//! This module provides native Rust implementations for managing AWS resources
//! using the official AWS SDK for Rust.
//!
//! ## Available Modules
//!
//! - [`AwsS3Module`](s3::AwsS3Module): S3 bucket and object management
//! - [`Ec2InstanceModule`](ec2::Ec2InstanceModule): EC2 instance lifecycle management
//! - [`Ec2SecurityGroupModule`](ec2::Ec2SecurityGroupModule): Security group management
//! - [`Ec2VpcModule`](ec2::Ec2VpcModule): VPC and subnet management
//! - [`AwsIamRoleModule`](iam::AwsIamRoleModule): IAM role management
//! - [`AwsIamPolicyModule`](iam::AwsIamPolicyModule): IAM managed policy management
//!
//! ## Authentication
//!
//! AWS credentials are loaded from the standard AWS credential chain:
//!
//! 1. Environment variables (`AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`)
//! 2. AWS credentials file (`~/.aws/credentials`)
//! 3. IAM instance profile (when running on EC2)
//! 4. ECS task role (when running in ECS)
//!
//! The region can be specified via:
//! - Module parameter (`region`)
//! - Environment variable (`AWS_REGION` or `AWS_DEFAULT_REGION`)
//! - AWS config file (`~/.aws/config`)

pub mod ec2;
pub mod iam;
pub mod s3;

pub use ec2::{Ec2InstanceModule, Ec2SecurityGroupModule, Ec2VpcModule};
pub use iam::{AwsIamPolicyModule, AwsIamRoleModule};
pub use s3::AwsS3Module;
