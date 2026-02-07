//! HPC (High Performance Computing) modules for Rustible
//!
//! This module provides configuration management modules specific to HPC
//! cluster environments. Modules are organized by subsystem:
//!
//! - **common**: Cluster baseline configuration (limits, sysctl, directories)
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

pub mod common;
#[cfg(feature = "slurm")]
pub mod slurm;
pub mod lmod;
pub mod mpi;
#[cfg(feature = "gpu")]
pub mod gpu;
#[cfg(feature = "ofed")]
pub mod ofed;
#[cfg(feature = "parallel_fs")]
pub mod fs;

pub use common::HpcBaselineModule;
#[cfg(feature = "slurm")]
pub use slurm::{SlurmConfigModule, SlurmOpsModule};
pub use lmod::LmodModule;
pub use mpi::MpiModule;
#[cfg(feature = "gpu")]
pub use gpu::NvidiaGpuModule;
#[cfg(feature = "ofed")]
pub use ofed::RdmaStackModule;
#[cfg(feature = "parallel_fs")]
pub use fs::{LustreClientModule, BeegfsClientModule};
