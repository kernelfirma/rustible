//! # Rustible - A Modern Configuration Management Tool
//!
//! Rustible is an async-first, type-safe configuration management and automation tool
//! written in Rust. It serves as a modern alternative to Ansible with improved performance,
//! better error handling, and parallel execution by default.
//!
//! ## Core Concepts
//!
//! - **Playbooks**: YAML-defined automation workflows containing plays and tasks
//! - **Inventory**: Collection of hosts organized into groups with variables
//! - **Modules**: Units of work that execute actions on target hosts
//! - **Tasks**: Individual units of execution that invoke modules
//! - **Handlers**: Special tasks triggered by notifications from other tasks
//! - **Roles**: Reusable collections of tasks, handlers, files, and templates
//! - **Facts**: System information gathered from target hosts
//! - **Connections**: Transport layer for communicating with hosts (SSH, local, etc.)
//!
//! ## Architecture Overview
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                           CLI Interface                              │
//! │                    (clap-based command parsing)                      │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Playbook Engine                              │
//! │              (Async execution with tokio runtime)                    │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!          ┌─────────────────────────┼─────────────────────────┐
//!          ▼                         ▼                         ▼
//! ┌─────────────────┐   ┌─────────────────────┐   ┌─────────────────────┐
//! │    Inventory    │   │   Module Registry   │   │   Template Engine   │
//! │    (hosts +     │   │   (built-in +       │   │   (Jinja2-compat    │
//! │     groups)     │   │    custom)          │   │    via minijinja)   │
//! └─────────────────┘   └─────────────────────┘   └─────────────────────┘
//!          │                         │                         │
//!          └─────────────────────────┼─────────────────────────┘
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Connection Manager                              │
//! │          (SSH, Local, Docker, Kubernetes connections)                │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Target Hosts                                 │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick Example
//!
//! ```rust,ignore
//! use rustible::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Load inventory
//!     let inventory = Inventory::from_file("inventory.yml").await?;
//!
//!     // Load and parse playbook
//!     let playbook = Playbook::from_file("playbook.yml").await?;
//!
//!     // Create executor with default settings
//!     let executor = PlaybookExecutor::new()
//!         .with_inventory(inventory)
//!         .with_parallelism(10)
//!         .build()?;
//!
//!     // Execute playbook
//!     let result = executor.run(&playbook).await?;
//!
//!     // Report results
//!     println!("{}", result.summary());
//!     Ok(())
//! }
//! ```

// Clippy configuration
#![warn(clippy::all)]
// Enable pedantic for local development, CI allows these
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
// Development-time allowances
#![allow(dead_code)]
#![allow(unused_variables)]

// Re-export commonly used items in prelude
pub mod prelude {
    //! Convenient re-exports of commonly used types and traits.
    //!
    //! This prelude provides quick access to the most commonly needed types:
    //!
    //! - **Connections**: Various connection types (SSH, Local, Docker)
    //! - **Execution**: Playbook and task executors
    //! - **Inventory**: Hosts, groups, and variables
    //! - **Modules**: Module system and registry
    //! - **Callbacks**: Common callback plugins (see [`callback::prelude`] for more)
    //! - **Errors**: Error handling types
    //!
    //! # Example
    //!
    //! ```rust,ignore
    //! use rustible::prelude::*;
    //!
    //! #[tokio::main]
    //! async fn main() -> Result<()> {
    //!     let inventory = Inventory::from_file("inventory.yml").await?;
    //!     let playbook = Playbook::from_file("playbook.yml").await?;
    //!
    //!     let executor = PlaybookExecutor::new()
    //!         .with_inventory(inventory)
    //!         .with_parallelism(10)
    //!         .build()?;
    //!
    //!     let result = executor.run(&playbook).await?;
    //!     Ok(())
    //! }
    //! ```
    //!
    //! [`callback::prelude`]: crate::callback::prelude

    // Connection types
    pub use crate::connection::config::RetryConfig;
    pub use crate::connection::docker::DockerConnection;
    pub use crate::connection::local::LocalConnection;
    #[cfg(feature = "russh")]
    pub use crate::connection::russh::{RusshConnection, RusshConnectionBuilder};
    #[cfg(feature = "ssh2-backend")]
    pub use crate::connection::ssh::{SshConnection, SshConnectionBuilder};
    pub use crate::connection::{
        CommandResult, Connection, ConnectionBuilder, ConnectionConfig, ConnectionError,
        ConnectionFactory, ConnectionResult, ConnectionType, ExecuteOptions, FileStat, HostConfig,
        TransferOptions,
    };

    // Error handling
    pub use crate::error::{Error, Result};

    // Execution engine
    pub use crate::executor::{PlaybookExecutor, TaskExecutor};

    // Facts system
    pub use crate::facts::Facts;

    // Handlers
    pub use crate::handlers::Handler;

    // Inventory
    pub use crate::inventory::{Group, Host, Inventory};

    // Module system
    pub use crate::modules::{Module, ModuleRegistry, ModuleResult};

    // Playbooks
    pub use crate::playbook::{Play, Playbook, Task};

    // Roles
    pub use crate::roles::Role;

    // Core traits
    pub use crate::traits::*;

    // Variables
    pub use crate::vars::Variables;

    // Common callback plugins (for full callback API, use callback::prelude)
    pub use crate::callback::{
        BoxedCallback, DefaultCallback, MinimalCallback, NullCallback, ProgressCallback,
        SharedCallback,
    };

    // Caching system
    pub use crate::cache::{CacheConfig, CacheManager, CacheMetrics, CacheStatus};
}

// ============================================================================
// Core Modules
// ============================================================================

/// Error types and result aliases for Rustible operations.
///
/// This module provides the main [`Error`](error::Error) enum that covers all possible
/// error conditions in Rustible, including connection failures, module errors,
/// parsing issues, and template rendering failures.
pub mod error;

/// Core traits that define the interfaces for extensible components.
///
/// Contains traits for connections, modules, and other pluggable components
/// that can be extended by users.
pub mod traits;

/// Shared utility functions.
pub mod utils;

/// Variable management and precedence handling.
///
/// This module handles the complex variable precedence rules similar to Ansible,
/// including host vars, group vars, play vars, and extra vars from the command line.
pub mod vars;

// ============================================================================
// Playbook Components
// ============================================================================

/// Handler system for triggered task execution.
///
/// Handlers are special tasks that only run when notified by other tasks.
/// They are typically used for service restarts or other actions that should
/// only happen once per play even if multiple tasks trigger them.
pub mod handlers;

/// Playbook parsing and representation.
///
/// This module handles loading, parsing, and representing YAML playbooks.
/// It supports the full Ansible playbook syntax including plays, tasks,
/// handlers, variables, and conditionals.
pub mod playbook;

// Parser module (not fully public due to API compatibility issues)
mod parser;

/// Schema validation for playbooks.
///
/// This module provides comprehensive playbook validation with JSON Schema-style
/// checks for module arguments. It catches configuration errors early
/// before execution begins.
///
/// Re-exported from the internal parser module.
pub mod schema {
    pub use crate::parser::schema::*;
}

/// Role management for reusable task collections.
///
/// Roles are a way to organize playbooks into reusable components.
/// Each role can contain tasks, handlers, files, templates, and variables.
pub mod roles;

/// Task definition and processing.
///
/// Tasks are the individual units of work in a playbook. This module
/// handles task parsing, loop expansion, conditional evaluation, and
/// delegation to modules for execution.
pub mod tasks;

// ============================================================================
// Infrastructure
// ============================================================================

/// Connection layer for remote host communication.
///
/// This module provides the [`Connection`](connection::Connection) trait and implementations
/// for various transport mechanisms:
/// - **SSH** (via russh or ssh2): Secure remote execution and file transfer
/// - **Local**: Direct execution on the control node
/// - **Docker**: Container-based execution
///
/// The connection layer handles command execution, file transfers, and
/// privilege escalation (sudo/su).
pub mod connection;

/// System fact gathering and caching.
///
/// Facts are system information gathered from target hosts, such as
/// OS type, network configuration, and hardware details. This module
/// provides mechanisms for collecting, caching, and querying facts.
pub mod facts;

/// Include handling for dynamic task inclusion.
///
/// Supports `include_tasks`, `import_tasks`, and similar constructs
/// for modular playbook organization.
pub mod include;

/// Host and group inventory management.
///
/// The inventory defines the target hosts and their groupings. This module
/// supports various inventory sources including YAML files, dynamic inventory
/// scripts, and programmatic construction.
pub mod inventory;

// ============================================================================
// Execution Engine
// ============================================================================

/// Core task execution engine with parallel execution support.
///
/// This module provides the main [`Executor`](executor::Executor) that orchestrates
/// playbook execution across multiple hosts. Key features include:
/// - **Parallel execution**: Run tasks across multiple hosts concurrently
/// - **Execution strategies**: Linear, free, and host-pinned modes
/// - **Handler management**: Automatic handler triggering and deduplication
/// - **Dependency resolution**: Topological sorting for task ordering
///
/// # Example
///
/// ```rust,ignore
/// use rustible::executor::{Executor, ExecutorConfig};
///
/// let config = ExecutorConfig {
///     forks: 10,
///     check_mode: false,
///     ..Default::default()
/// };
///
/// let executor = Executor::new(config);
/// let results = executor.run_playbook(&playbook).await?;
/// ```
pub mod executor;

/// Execution strategy implementations.
///
/// Defines different strategies for how tasks are distributed across hosts:
/// - **Linear**: All hosts complete a task before moving to the next
/// - **Free**: Each host proceeds independently at maximum speed
/// - **Host-pinned**: Dedicated workers per host for optimal cache locality
pub mod strategy;

// ============================================================================
// Caching System
// ============================================================================

/// Intelligent caching system for improved performance.
///
/// This module provides comprehensive caching for:
/// - **Fact Caching**: Cache gathered facts from hosts with TTL-based expiration
/// - **Playbook Parse Caching**: Cache parsed playbook structures
/// - **Variable Caching**: Cache resolved variable contexts
/// - **Role Caching**: Cache loaded roles and their contents
///
/// The cache system supports multiple invalidation strategies:
/// - TTL-based expiration
/// - Dependency-based invalidation (file changes)
/// - Memory pressure eviction
///
/// # Performance Benefits
///
/// - Facts gathering: ~3-5s saved per cached host
/// - Playbook parsing: ~15x faster for repeated executions
/// - Variable resolution: ~80% reduction in template rendering time
/// - Role loading: Near-instant for cached roles
///
/// # Example
///
/// ```rust,ignore
/// use rustible::cache::{CacheManager, CacheConfig};
///
/// // Create a cache manager with production settings
/// let cache = CacheManager::with_config(CacheConfig::production());
///
/// // Cache and retrieve facts
/// cache.facts.insert_raw("host1", gathered_facts);
/// if let Some(facts) = cache.facts.get("host1") {
///     println!("Cached facts available for host1");
/// }
///
/// // Get cache statistics
/// let status = cache.status();
/// println!("Cache hit rate: {:.2}%", status.facts_hit_rate * 100.0);
/// ```
pub mod cache;

// ============================================================================
// Modules (Built-in task implementations)
// ============================================================================

/// Built-in module implementations for common automation tasks.
///
/// Modules are the workhorses of Rustible, performing the actual work on target
/// systems. This crate includes modules for:
///
/// - **Package management**: `apt`, `yum`, `dnf`, `pip`
/// - **File operations**: `copy`, `file`, `template`, `lineinfile`
/// - **System administration**: `user`, `group`, `service`
/// - **Command execution**: `command`, `shell`
/// - **Source control**: `git`
///
/// Custom modules can be implemented by implementing the [`Module`](modules::Module) trait.
///
/// # Example
///
/// ```rust,ignore
/// use rustible::modules::{ModuleRegistry, ModuleContext, ModuleParams};
///
/// let registry = ModuleRegistry::with_builtins();
/// let params: ModuleParams = serde_json::from_value(serde_json::json!({
///     "name": "nginx",
///     "state": "present"
/// }))?;
///
/// let result = registry.execute("apt", &params, &context)?;
/// ```
pub mod modules;

// ============================================================================
// Templating and Variables
// ============================================================================

/// Jinja2-compatible template engine powered by minijinja.
///
/// This module provides template rendering for files and strings using
/// a syntax compatible with Ansible's Jinja2 templates. Supports filters,
/// tests, and custom extensions.
pub mod template;

// ============================================================================
// Vault (Encrypted secrets management)
// ============================================================================

/// Ansible Vault-compatible encryption for sensitive data.
///
/// Provides encryption and decryption of sensitive data using AES-256
/// encryption, compatible with Ansible Vault format. Supports both
/// file-level and inline variable encryption.
pub mod vault;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration management for Rustible behavior.
///
/// Handles loading and merging configuration from multiple sources:
/// environment variables, config files, and command-line arguments.
pub mod config;

// ============================================================================
// Reporting and Output
// ============================================================================

/// Output formatting and reporting utilities.
///
/// Provides various output formats for playbook execution results,
/// including human-readable console output and machine-parseable formats.
pub mod output;

// ============================================================================
// Callback Plugins
// ============================================================================

/// Callback plugin system for execution event handling.
///
/// Callbacks receive notifications about execution events (task start/end,
/// host unreachable, etc.) and can be used for logging, metrics, or
/// custom integrations.
///
/// # Built-in Callbacks
///
/// - [`DefaultCallback`](callback::DefaultCallback): Standard output formatting
/// - [`MinimalCallback`](callback::MinimalCallback): Quiet output mode
/// - [`ProgressCallback`](callback::ProgressCallback): Progress bar display
/// - [`NullCallback`](callback::NullCallback): No output (for testing)
pub mod callback;

// ============================================================================
// Diagnostics and Debugging
// ============================================================================

/// Diagnostic tools for debugging and troubleshooting.
///
/// Provides debugging capabilities: Debug Mode, Variable Inspection,
/// Step-by-step Execution, Breakpoint Support, and State Dump.
pub mod diagnostics;

// ============================================================================
// Metrics and Observability
// ============================================================================

/// Metrics and observability for Rustible.
///
/// Provides metrics collection and export: Connection Metrics, Pool Metrics,
/// Command Metrics, and Prometheus Export.
pub mod metrics;

/// Structured logging system based on loggingsucks.com philosophy.
///
/// Provides wide-event logging with JSON output, trace ID propagation,
/// and intelligent sampling for queryable analytics.
pub mod logging;

// ============================================================================
// State Management
// ============================================================================

/// State management system for tracking execution state, diffs, and rollback.
///
/// This module provides comprehensive state tracking, persistence, diff reporting,
/// rollback capability, and dependency tracking between tasks.
pub mod state;

/// Recovery system for handling failures, checkpoints, and transactions.
pub mod recovery;

// ============================================================================
// Infrastructure Provisioning (Terraform-like)
// ============================================================================

/// Infrastructure provisioning module for declarative cloud resource management.
///
/// This module provides Terraform-like capabilities for provisioning infrastructure
/// resources via cloud provider APIs. It enables Rustible to serve as a unified
/// replacement for both Ansible and Terraform.
///
/// # Features
///
/// - **Declarative Resources**: Define infrastructure in YAML
/// - **Plan/Apply Workflow**: Preview changes before applying
/// - **State Management**: Track provisioned resources
/// - **Provider Support**: AWS, Azure, GCP (with AWS as primary)
/// - **Dependency Resolution**: Automatic ordering based on references
///
/// # Example
///
/// ```yaml
/// # infrastructure.rustible.yml
/// providers:
///   aws:
///     region: us-east-1
///
/// resources:
///   aws_vpc:
///     main:
///       cidr_block: "10.0.0.0/16"
///       tags:
///         Name: production-vpc
///
///   aws_subnet:
///     public:
///       vpc_id: "{{ resources.aws_vpc.main.id }}"
///       cidr_block: "10.0.1.0/24"
/// ```
///
/// ```rust,ignore
/// use rustible::provisioning::{ProvisioningExecutor, InfrastructureConfig};
///
/// let config = InfrastructureConfig::from_file("infrastructure.yml").await?;
/// let executor = ProvisioningExecutor::new(config).await?;
///
/// // Generate and review plan
/// let plan = executor.plan().await?;
/// println!("{}", plan.summary());
///
/// // Apply changes
/// let result = executor.apply(&plan).await?;
/// ```
#[cfg(feature = "provisioning")]
pub mod provisioning;

// ============================================================================
// REST API (Optional)
// ============================================================================

/// REST API server for Rustible.
///
/// This module provides a REST API for Rustible, enabling programmatic access
/// to playbook execution, inventory management, and job monitoring.
///
/// # Features
///
/// - **Playbook Execution**: Submit playbooks for execution via HTTP
/// - **Inventory Management**: Query hosts, groups, and variables
/// - **Job Management**: Monitor job status and history
/// - **Real-time Output**: WebSocket support for live execution output
/// - **Authentication**: JWT-based authentication
///
/// # Example
///
/// ```rust,ignore
/// use rustible::api::{ApiServer, ApiConfig};
///
/// #[tokio::main]
/// async fn main() {
///     let config = ApiConfig::default();
///     let server = ApiServer::new(config);
///     server.run().await.unwrap();
/// }
/// ```
#[cfg(feature = "api")]
pub mod api;

// ============================================================================
// Galaxy Support (Ansible Galaxy integration)
// ============================================================================

/// Ansible Galaxy support for installing collections and roles.
///
/// This module provides comprehensive support for installing and managing
/// Ansible Galaxy collections and roles with:
///
/// - **Robust API client**: HTTP client with retry logic and timeout handling
/// - **Collection installation**: Install collections from Galaxy or tarballs
/// - **Role installation**: Install roles from Galaxy or Git repositories
/// - **Requirements parsing**: Parse and process requirements.yml files
/// - **Local caching**: Cache downloaded artifacts with integrity verification
/// - **Offline mode**: Fall back to cached artifacts when Galaxy is unavailable
///
/// # Example
///
/// ```rust,ignore
/// use rustible::galaxy::{Galaxy, GalaxyConfig, RequirementsFile};
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     // Create Galaxy client with default configuration
///     let config = GalaxyConfig::default();
///     let galaxy = Galaxy::new(config)?;
///
///     // Install a collection
///     galaxy.install_collection("community.general", Some("5.0.0"), None).await?;
///
///     // Install from requirements.yml
///     let requirements = RequirementsFile::from_path("requirements.yml").await?;
///     galaxy.install_requirements(&requirements).await?;
///
///     Ok(())
/// }
/// ```
pub mod galaxy;

// ============================================================================
// Lockfile Support (Reproducible Builds)
// ============================================================================

/// Lockfile support for reproducible playbook execution.
///
/// This module provides lockfile functionality similar to Cargo.lock or package-lock.json,
/// enabling reproducible playbook executions by pinning versions of:
///
/// - Ansible Galaxy roles and collections
/// - Python module dependencies
/// - External resources (URLs, git refs)
///
/// # Example
///
/// ```rust,ignore
/// use rustible::lockfile::{Lockfile, LockfileManager};
///
/// // Create lockfile for a playbook
/// let mut lockfile = Lockfile::new("playbook.yml")?;
///
/// // Verify playbook matches locked state
/// let manager = LockfileManager::new("playbook.yml").frozen(true);
/// manager.verify("playbook.yml")?;
/// ```
pub mod lockfile;

// ============================================================================
// Version Information
// ============================================================================

/// Returns the current version of Rustible.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Returns detailed version information including build metadata.
pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: env!("CARGO_PKG_VERSION"),
        rust_version: option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("unknown"),
        target: std::env::consts::ARCH,
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    }
}

/// Detailed version information for the Rustible build.
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Semantic version string
    pub version: &'static str,
    /// Minimum Rust version required
    pub rust_version: &'static str,
    /// Target triple for the build
    pub target: &'static str,
    /// Build profile (debug or release)
    pub profile: &'static str,
}

impl std::fmt::Display for VersionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rustible {} ({}, {})",
            self.version, self.target, self.profile
        )
    }
}
