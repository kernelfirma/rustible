//! Resource Implementations for Infrastructure Provisioning
//!
//! This module contains resource implementations organized by cloud provider.
//! Each resource implements the `Resource` trait defined in the traits module.
//!
//! # Structure
//!
//! - `aws/` - AWS resource implementations
//!   - `security_group` - EC2 Security Groups

#[cfg(feature = "aws")]
pub mod aws;
