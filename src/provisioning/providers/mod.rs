//! Cloud Provider Implementations
//!
//! This module contains provider-specific implementations for infrastructure provisioning.
//!
//! # Available Providers
//!
//! - `aws`: Amazon Web Services (EC2, VPC, S3, etc.)
//! - `redfish`: Bare-metal BMC management via the DMTF Redfish REST API
//! - `openstack`: OpenStack cloud (Nova, Neutron) via Keystone v3 auth
//! - `vsphere`: VMware vSphere VMs via the `govc` CLI (experimental)
//!
//! # Future Providers
//!
//! - `azure`: Microsoft Azure (planned)
//! - `gcp`: Google Cloud Platform (planned)
//! - `digitalocean`: DigitalOcean (planned)

#[cfg(feature = "aws")]
pub mod aws;

#[cfg(all(feature = "azure", feature = "experimental"))]
pub mod azure;

#[cfg(all(feature = "gcp", feature = "experimental"))]
pub mod gcp;

#[cfg(feature = "redfish")]
pub mod redfish;

#[cfg(feature = "openstack")]
pub mod openstack;

pub mod vsphere;
