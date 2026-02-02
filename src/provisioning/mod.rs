//! Infrastructure Provisioning Module
//!
//! This module provides declarative infrastructure provisioning capabilities,
//! enabling Rustible to complement Ansible with Terraform-like provisioning for
//! a limited set of resources. It adds native infrastructure provisioning using
//! cloud provider APIs.
//!
//! ## Core Concepts
//!
//! - **Resources**: Declarative infrastructure units (VPCs, instances, etc.)
//! - **Providers**: Cloud provider implementations (AWS, Azure, GCP)
//! - **State**: Persistent tracking of provisioned resources
//! - **Plans**: Execution plans showing what will change
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                 Infrastructure Config (YAML)                 │
//! │           (providers, variables, resources, outputs)         │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    ProvisioningExecutor                      │
//! │         (orchestrates plan/apply/destroy operations)         │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!          ┌───────────────────┼───────────────────┐
//!          ▼                   ▼                   ▼
//! ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
//! │  ResourceRegistry │ │  ExecutionPlan  │ │ ProvisioningState│
//! │  (type→Resource)  │ │  (diff/actions) │ │  (persistence)   │
//! └─────────────────┘ └─────────────────┘ └─────────────────┘
//!          │                   │                   │
//!          ▼                   ▼                   ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     Provider Layer                           │
//! │              (AWS, Azure, GCP implementations)               │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example Usage
//!
//! ```yaml
//! # infrastructure.rustible.yml
//! providers:
//!   aws:
//!     region: us-east-1
//!
//! resources:
//!   aws_vpc:
//!     main:
//!       cidr_block: "10.0.0.0/16"
//!       tags:
//!         Name: production-vpc
//!
//!   aws_subnet:
//!     public:
//!       vpc_id: "{{ resources.aws_vpc.main.id }}"
//!       cidr_block: "10.0.1.0/24"
//! ```
//!
//! ```rust,no_run
//! use rustible::prelude::*;
//! use rustible::provisioning::{ProvisioningExecutor, InfrastructureConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     let config = InfrastructureConfig::from_file("infrastructure.rustible.yml").await?;
//!     let executor = ProvisioningExecutor::new(config)?;
//!
//!     // Generate execution plan
//!     let plan = executor.plan().await?;
//!     println!("{}", plan.summary());
//!
//!     // Apply changes
//!     let result = executor.apply().await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Template Resolution
//!
//! Resources can reference each other using template syntax:
//!
//! ```yaml
//! resources:
//!   aws_subnet:
//!     public:
//!       vpc_id: "{{ resources.aws_vpc.main.id }}"
//! ```
//!
//! The resolver automatically:
//! - Builds dependency graph from references
//! - Resolves in topological order
//! - Injects computed attributes from state
//!
//! Resolution order:
//! 1. Variables are resolved first
//! 2. Provider configurations are resolved
//! 3. Resources are resolved in dependency order
//! 4. Outputs are resolved last
//!
//! ## State Management
//!
//! Rustible provides comprehensive state management:
//!
//! - **Local State**: File-based state in `.rustible/provisioning.state.json`
//! - **Remote Backends**: S3, GCS, Azure Blob, HTTP
//! - **State Locking**: Prevent concurrent modifications
//! - **State Diff**: Compare states to see what changed
//! - **Migration**: Automatic state version upgrades
//!
//! ### Backend Configuration
//!
//! ```yaml
//! terraform:
//!   backend: s3
//!   config:
//!     bucket: my-terraform-state
//!     key: prod/terraform.tfstate
//!     region: us-east-1
//!     dynamodb_table: terraform-locks
//! ```

pub mod config;
pub mod error;
pub mod executor;
pub mod plan;
pub mod providers;
pub mod registry;
pub mod resolver;
pub mod resources;
pub mod state;
pub mod state_backends;
pub mod state_lock;
pub mod template_functions;
pub mod traits;

// Re-export commonly used types
pub use config::{DependencyEdge, InfrastructureConfig, ReferenceType};
pub use error::{ProvisioningError, ProvisioningResult};
pub use executor::ProvisioningExecutor;
pub use plan::{ExecutionPlan, PlannedAction, ResourceChange};
pub use registry::{parse_resource_type, ProviderRegistry, ResourceRegistry};
pub use resolver::{
    PathContext, ProvisionerContext, ResolvedConfig, ResolverContext, TemplateResolver,
    TerraformContext,
};
pub use state::{
    DiffSummary, MigrationRegistry, MigrationV1ToV2, OutputValue, ProvisioningState,
    ProvisioningStateDiff, ResourceId, ResourceIndex, ResourceState, StateChange, StateChangeType,
    StateMigration, StateSummary,
};
pub use state_backends::{BackendConfig, LocalBackend, StateBackend};
pub use state_lock::{LockGuard, LockInfo, StateLockManager};
pub use template_functions::register_infrastructure_functions;
pub use traits::{
    ChangeType, DataSource, FieldConstraint, FieldType, Provider, ProviderConfig, ProviderContext,
    ProviderCredentials, ProviderSchema, Resource, ResourceDependency, ResourceDiff,
    ResourceReadResult, ResourceResult, ResourceSchema, ResourceTimeouts, RetryConfig, SchemaField,
};

// Re-export AWS provider and resources (when feature enabled)
#[cfg(feature = "aws")]
pub use providers::aws::{AwsCredentialChain, AwsCredentials, AwsProvider, CredentialSource};

#[cfg(feature = "aws")]
pub use resources::aws::{
    AwsInstanceResource, AwsSecurityGroupResource, AwsSubnetResource, AwsVpcResource,
};

#[cfg(feature = "aws")]
pub use state_backends::S3Backend;
