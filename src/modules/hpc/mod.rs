//! HPC (High Performance Computing) modules for Rustible
//!
//! This module provides configuration management modules specific to HPC
//! cluster environments. Modules are organized by subsystem:
//!
//! - **common**: Cluster baseline configuration (limits, sysctl, directories)
//! - **munge**: MUNGE authentication (Slurm prerequisite)
//! - **nfs**: NFS server and client management
//! - **healthcheck**: HPC node health validation
//! - **facts**: HPC-specific fact gathering (CPU, NUMA, GPU, IB)
//! - **slurm**: Slurm workload manager (controller, compute, operations)
//! - **lmod**: Lmod / Environment Modules software management
//! - **mpi**: MPI library configuration (OpenMPI, Intel MPI)
//! - **gpu**: GPU management (NVIDIA drivers, CUDA, ROCm)
//! - **ofed**: InfiniBand / RDMA / OFED stack management
//! - **fs**: Parallel filesystem clients (Lustre, BeeGFS)
//!
//! # Target Distributions
//!
//! HPC modules target these distributions initially:
//! - Rocky Linux / Alma Linux 9 (RHEL-family)
//! - Ubuntu 22.04 LTS (Debian-family)
//!
//! Modules detect the OS family and fail with clear error messages on
//! unsupported distributions.
//!
//! # Conventions
//!
//! All HPC modules follow these conventions:
//!
//! ## Idempotency
//! - Modules check current state before making changes
//! - Re-running a module with the same parameters produces no changes
//! - State is detected via command output parsing, not file markers
//!
//! ## Check Mode
//! - All modules support `check_mode` (dry-run)
//! - In check mode, modules report what *would* change without acting
//!
//! ## Structured Output
//! - Modules return parsed data in `ModuleOutput.data` (not raw stdout)
//! - Example: Slurm modules return parsed `scontrol` output as JSON
//!
//! ## Error Handling
//! - Unsupported OS → `ModuleError::Unsupported` with clear message
//! - Missing prerequisites → `ModuleError::ExecutionFailed` with install hint

#[cfg(feature = "parallel_fs")]
pub mod beegfs_target;
pub mod boot_profile;
pub mod common;
#[cfg(feature = "gpu")]
pub mod cuda;
#[cfg(feature = "gpu")]
pub mod dcgm;
pub mod discovery;
#[cfg(feature = "gpu")]
pub mod fabric_manager;
pub mod facts;
#[cfg(feature = "parallel_fs")]
pub mod fs;
#[cfg(feature = "gpu")]
pub mod gdrcopy;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod healthcheck;
pub mod hpc_job;
pub mod hpc_queue;
pub mod hpc_server;
#[cfg(feature = "ofed")]
pub mod ib_diagnostics;
#[cfg(feature = "ofed")]
pub mod ib_partition;
#[cfg(feature = "ofed")]
pub mod ib_validate;
pub mod image_pipeline;
pub mod ipmi;
#[cfg(feature = "ofed")]
pub mod ipoib;
#[cfg(feature = "identity")]
pub mod kerberos;
pub mod lmod;
#[cfg(feature = "lsf")]
pub mod lsf;
#[cfg(feature = "parallel_fs")]
pub mod lustre_mount;
#[cfg(feature = "parallel_fs")]
pub mod lustre_ost;
#[cfg(feature = "gpu")]
pub mod mig_config;
pub mod mpi;
pub mod munge;
#[cfg(feature = "gpu")]
pub mod nccl;
pub mod nfs;
#[cfg(feature = "gpu")]
pub mod nvidia_container_toolkit;
#[cfg(feature = "gpu")]
pub mod nvidia_driver;
#[cfg(feature = "gpu")]
pub mod nvidia_peermem;
#[cfg(feature = "ofed")]
pub mod ofed;
#[cfg(feature = "ofed")]
pub mod opensm;
#[cfg(feature = "slurm")]
pub mod partition_policy;
#[cfg(feature = "pbs")]
pub mod pbs_job;
#[cfg(feature = "pbs")]
pub mod pbs_queue;
#[cfg(feature = "pbs")]
pub mod pbs_server;
pub mod power;
#[cfg(feature = "bare_metal")]
pub mod pxe;
#[cfg(feature = "redfish")]
pub mod redfish;
pub mod scheduler;
#[cfg(feature = "slurm")]
pub mod scheduler_orchestration;
#[cfg(feature = "pbs")]
pub mod scheduler_pbs;
#[cfg(feature = "slurm")]
pub mod scheduler_slurm;
#[cfg(feature = "slurm")]
pub mod slurm;
#[cfg(feature = "slurm")]
pub mod slurm_account;
#[cfg(feature = "slurm")]
pub mod slurm_info;
#[cfg(feature = "slurm")]
pub mod slurm_job;
#[cfg(feature = "slurm")]
pub mod slurm_node;
#[cfg(feature = "slurm")]
pub mod slurm_partition;
#[cfg(feature = "slurm")]
pub mod slurmrestd;
#[cfg(feature = "identity")]
pub mod sssd;
pub mod toolchain;
#[cfg(feature = "bare_metal")]
pub mod warewulf;

#[cfg(feature = "parallel_fs")]
pub use beegfs_target::BeegfsTargetModule;
pub use boot_profile::BootProfileModule;
pub use common::HpcBaselineModule;
#[cfg(feature = "gpu")]
pub use cuda::CudaToolkitModule;
#[cfg(feature = "gpu")]
pub use dcgm::DcgmModule;
pub use discovery::HpcDiscoveryModule;
#[cfg(feature = "gpu")]
pub use fabric_manager::FabricManagerModule;
pub use facts::HpcFactsModule;
#[cfg(feature = "parallel_fs")]
pub use fs::{BeegfsClientModule, LustreClientModule};
#[cfg(feature = "gpu")]
pub use gdrcopy::GdrcopyModule;
#[cfg(feature = "gpu")]
pub use gpu::NvidiaGpuModule;
pub use healthcheck::HpcHealthcheckModule;
pub use hpc_job::HpcJobModule;
pub use hpc_queue::HpcQueueModule;
pub use hpc_server::HpcServerModule;
#[cfg(feature = "ofed")]
pub use ib_diagnostics::IbDiagnosticsModule;
#[cfg(feature = "ofed")]
pub use ib_partition::IbPartitionModule;
#[cfg(feature = "ofed")]
pub use ib_validate::IbValidateModule;
pub use image_pipeline::ImagePipelineModule;
pub use ipmi::{IpmiBootModule, IpmiPowerModule};
#[cfg(feature = "ofed")]
pub use ipoib::IpoibModule;
#[cfg(feature = "identity")]
pub use kerberos::KerberosClientModule;
pub use lmod::LmodModule;
#[cfg(feature = "lsf")]
pub use lsf::{LsfHostModule, LsfPolicyModule, LsfQueueModule};
#[cfg(feature = "parallel_fs")]
pub use lustre_mount::LustreMountModule;
#[cfg(feature = "parallel_fs")]
pub use lustre_ost::LustreOstModule;
#[cfg(feature = "gpu")]
pub use mig_config::MigConfigModule;
pub use mpi::MpiModule;
pub use munge::MungeModule;
#[cfg(feature = "gpu")]
pub use nccl::NcclModule;
pub use nfs::{NfsClientModule, NfsServerModule};
#[cfg(feature = "gpu")]
pub use nvidia_container_toolkit::NvidiaContainerToolkitModule;
#[cfg(feature = "gpu")]
pub use nvidia_driver::NvidiaDriverModule;
#[cfg(feature = "gpu")]
pub use nvidia_peermem::NvidiaPeermemModule;
#[cfg(feature = "ofed")]
pub use ofed::RdmaStackModule;
#[cfg(feature = "ofed")]
pub use opensm::OpensmConfigModule;
#[cfg(feature = "slurm")]
pub use partition_policy::PartitionPolicyModule;
#[cfg(feature = "pbs")]
pub use pbs_job::PbsJobModule;
#[cfg(feature = "pbs")]
pub use pbs_queue::PbsQueueModule;
#[cfg(feature = "pbs")]
pub use pbs_server::PbsServerModule;
pub use power::HpcPowerModule;
#[cfg(feature = "bare_metal")]
pub use pxe::{PxeHostModule, PxeProfileModule};
#[cfg(feature = "redfish")]
pub use redfish::{RedfishInfoModule, RedfishPowerModule};
pub use scheduler::{HpcScheduler, JobInfo, JobState, QueueInfo, ServerInfo};
#[cfg(feature = "slurm")]
pub use scheduler_orchestration::SchedulerOrchestrationModule;
#[cfg(feature = "pbs")]
pub use scheduler_pbs::PbsScheduler;
#[cfg(feature = "slurm")]
pub use scheduler_slurm::SlurmScheduler;
#[cfg(feature = "slurm")]
pub use slurm::{SlurmConfigModule, SlurmOpsModule};
#[cfg(feature = "slurm")]
pub use slurm_account::{SlurmAccountModule, SlurmQosModule};
#[cfg(feature = "slurm")]
pub use slurm_info::SlurmInfoModule;
#[cfg(feature = "slurm")]
pub use slurm_job::SlurmJobModule;
#[cfg(feature = "slurm")]
pub use slurm_node::SlurmNodeModule;
#[cfg(feature = "slurm")]
pub use slurm_partition::SlurmPartitionModule;
#[cfg(feature = "slurm")]
pub use slurmrestd::SlurmrestdModule;
#[cfg(feature = "identity")]
pub use sssd::{SssdConfigModule, SssdDomainModule};
pub use toolchain::HpcToolchainModule;
#[cfg(feature = "bare_metal")]
pub use warewulf::{WarewulfImageModule, WarewulfNodeModule};
