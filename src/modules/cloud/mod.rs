//! Cloud provider modules for infrastructure provisioning.
//!
//! This module provides integrations with major cloud providers for
//! infrastructure-as-code workflows. Supported providers include:
//!
//! - **AWS**: Amazon Web Services (EC2, S3, VPC, etc.)
//! - **Azure**: Microsoft Azure (VMs, Resource Groups, Networking)
//! - **GCP**: Google Cloud Platform (Compute Engine, Networking)
//! - **Kubernetes**: Container orchestration (requires `kubernetes` feature)
//!
//! ## Feature Flags
//!
//! Cloud modules are gated behind feature flags to minimize dependencies:
//!
//! - `aws`: Enable AWS SDK modules (EC2, S3, VPC, IAM)
//! - `azure`: Enable Azure SDK modules (VMs, Resource Groups, Networking)
//! - `gcp`: Enable GCP SDK modules (Compute Engine, Networking)
//! - `kubernetes`: Enable Kubernetes modules (Deployments, Services, ConfigMaps)
//!
//! ## Example
//!
//! ```yaml
//! # AWS Example
//! - name: Create S3 bucket
//!   aws_s3:
//!     bucket: my-application-bucket
//!     state: present
//!     region: us-west-2
//!
//! - name: Launch EC2 instance
//!   aws_ec2_instance:
//!     name: web-server-01
//!     instance_type: t3.micro
//!     image_id: ami-0abcdef1234567890
//!     state: running
//!
//! # Azure Example
//! - name: Create Azure VM
//!   azure_vm:
//!     name: web-server-01
//!     resource_group: my-rg
//!     location: eastus
//!     vm_size: Standard_B2s
//!     state: present
//!
//! # GCP Example
//! - name: Create Compute Engine instance
//!   gcp_compute_instance:
//!     name: web-server-01
//!     zone: us-central1-a
//!     machine_type: e2-medium
//!     state: running
//!
//! # Kubernetes Example
//! - name: Create deployment
//!   k8s_deployment:
//!     name: my-app
//!     namespace: default
//!     replicas: 3
//!     state: present
//! ```

#[cfg(feature = "aws")]
pub mod aws;

#[cfg(feature = "azure")]
pub mod azure;

#[cfg(feature = "gcp")]
pub mod gcp;

#[cfg(feature = "kubernetes")]
pub mod kubernetes;

// Re-export AWS modules
#[cfg(feature = "aws")]
pub use aws::{
    AwsEbsVolumeModule, AwsIamPolicyModule, AwsIamRoleModule, AwsS3Module,
    AwsSecurityGroupRuleModule, Ec2InstanceModule, Ec2SecurityGroupModule, Ec2VpcModule,
};

// Re-export Azure modules
#[cfg(feature = "azure")]
pub use azure::{AzureNetworkInterfaceModule, AzureResourceGroupModule, AzureVmModule};

// Re-export GCP modules
#[cfg(feature = "gcp")]
pub use gcp::{
    GcpComputeFirewallModule, GcpComputeInstanceModule, GcpComputeNetworkModule,
    GcpServiceAccountModule,
};

// Re-export Kubernetes modules
#[cfg(feature = "kubernetes")]
pub use kubernetes::{K8sConfigMapModule, K8sDeploymentModule, K8sSecretModule, K8sServiceModule};
