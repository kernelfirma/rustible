//! Core traits defining the fundamental abstractions in Rustible.
//!
//! This module contains the primary trait definitions that form the backbone
//! of Rustible's architecture. These traits enable extensibility, allowing
//! users to implement custom modules, connections, and inventory sources.

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use crate::error::Result;
use crate::facts::Facts;
use crate::vars::Variables;

// ============================================================================
// Module Traits
// ============================================================================

/// Represents a module that can be executed on a target host.
///
/// Modules are the units of work in Rustible. Each module performs a specific
/// action such as copying files, managing packages, or executing commands.
///
/// # Idempotency
///
/// Modules should be idempotent whenever possible - running the same module
/// with the same arguments multiple times should produce the same result.
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::traits::{Module, ModuleArgs, ExecutionContext};
/// use rustible::traits::ModuleResult;
/// use async_trait::async_trait;
///
/// #[derive(Debug)]
/// struct CopyModule;
///
/// #[async_trait]
/// impl Module for CopyModule {
///     fn name(&self) -> &str {
///         "copy"
///     }
///
///     async fn execute(
///         &self,
///         args: &dyn ModuleArgs,
///         ctx: &ExecutionContext,
///     ) -> Result<ModuleResult> {
///         // Implementation here
///         Ok(ModuleResult::changed("File copied successfully"))
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[async_trait]
pub trait Module: Send + Sync + Debug {
    /// Returns the unique name of this module.
    ///
    /// This name is used to reference the module in playbooks.
    fn name(&self) -> &str;

    /// Returns a description of what this module does.
    fn description(&self) -> &str {
        "No description available"
    }

    /// Returns the expected arguments schema for this module.
    ///
    /// Used for validation and documentation generation.
    fn args_schema(&self) -> Option<&ModuleSchema> {
        None
    }

    /// Validates the provided arguments before execution.
    ///
    /// This is called before `execute` to catch configuration errors early.
    fn validate_args(&self, args: &dyn ModuleArgs) -> Result<()> {
        let _ = args;
        Ok(())
    }

    /// Executes the module with the given arguments and context.
    ///
    /// # Arguments
    ///
    /// * `args` - The arguments provided to the module
    /// * `ctx` - The execution context containing connection, facts, and variables
    ///
    /// # Returns
    ///
    /// A `ModuleResult` indicating success/failure and whether changes were made.
    async fn execute(&self, args: &dyn ModuleArgs, ctx: &ExecutionContext) -> Result<ModuleResult>;

    /// Performs a dry-run of the module (check mode).
    ///
    /// Should return what would happen without making actual changes.
    async fn check(&self, args: &dyn ModuleArgs, ctx: &ExecutionContext) -> Result<ModuleResult> {
        // Default implementation just reports what would be checked
        let _ = (args, ctx);
        Ok(ModuleResult::skipped(
            "Check mode not implemented for this module",
        ))
    }

    /// Returns the diff between current and desired state.
    ///
    /// Used when `--diff` flag is provided.
    async fn diff(
        &self,
        args: &dyn ModuleArgs,
        ctx: &ExecutionContext,
    ) -> Result<Option<ModuleDiff>> {
        let _ = (args, ctx);
        Ok(None)
    }
}

/// Arguments passed to a module for execution.
///
/// This trait provides a type-erased interface for accessing module arguments.
pub trait ModuleArgs: Send + Sync + Debug {
    /// Returns the argument value for the given key.
    fn get(&self, key: &str) -> Option<&serde_json::Value>;

    /// Returns all arguments as a map.
    fn as_map(&self) -> &HashMap<String, serde_json::Value>;

    /// Returns all arguments as a JSON value.
    fn as_json(&self) -> serde_json::Value;

    /// Returns this as Any for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Helper function to deserialize module args to a concrete type.
pub fn deserialize_args<T: DeserializeOwned>(args: &dyn ModuleArgs) -> Result<T> {
    let json = args.as_json();
    serde_json::from_value(json).map_err(|e| crate::error::Error::ModuleArgs {
        module: "unknown".to_string(),
        message: e.to_string(),
    })
}

/// Schema definition for module arguments.
#[derive(Debug, Clone)]
pub struct ModuleSchema {
    /// Required arguments
    pub required: Vec<ArgSpec>,
    /// Optional arguments
    pub optional: Vec<ArgSpec>,
    /// Whether additional arguments are allowed
    pub additional_properties: bool,
}

/// Specification for a single argument.
#[derive(Debug, Clone)]
pub struct ArgSpec {
    /// Argument name
    pub name: String,
    /// Argument type
    pub arg_type: ArgType,
    /// Description of the argument
    pub description: String,
    /// Default value if not provided
    pub default: Option<serde_json::Value>,
    /// Valid choices for this argument
    pub choices: Option<Vec<serde_json::Value>>,
}

/// Types of arguments a module can accept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgType {
    /// String value
    String,
    /// Integer value
    Integer,
    /// Boolean value
    Boolean,
    /// List of values
    List,
    /// Dictionary/map
    Dict,
    /// File path
    Path,
    /// Any type
    Any,
}

/// Result of a module execution.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleResult {
    /// Whether the module execution was successful
    pub success: bool,
    /// Whether the module made changes to the target
    pub changed: bool,
    /// Human-readable message about the result
    pub message: String,
    /// Whether the task was skipped
    pub skipped: bool,
    /// Additional output data from the module
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// Warnings generated during execution
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl ModuleResult {
    /// Creates a successful result indicating no changes were made.
    pub fn ok(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: false,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    /// Creates a successful result indicating changes were made.
    pub fn changed(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: true,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    /// Creates a failed result.
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            success: false,
            changed: false,
            message: message.into(),
            skipped: false,
            data: None,
            warnings: Vec::new(),
        }
    }

    /// Creates a skipped result.
    pub fn skipped(message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed: false,
            message: message.into(),
            skipped: true,
            data: None,
            warnings: Vec::new(),
        }
    }

    /// Attaches additional data to the result.
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.data = Some(data);
        self
    }

    /// Adds a warning to the result.
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

/// Represents a diff between current and desired state.
#[derive(Debug, Clone)]
pub struct ModuleDiff {
    /// Description of what will change
    pub description: String,
    /// Before state (if available)
    pub before: Option<String>,
    /// After state (if available)
    pub after: Option<String>,
}

// ============================================================================
// Connection Traits
// ============================================================================

/// Represents a connection to a target host.
///
/// Connections handle the transport layer for executing commands and
/// transferring files to remote (or local) systems.
///
/// # Implementations
///
/// - `SshConnection` - SSH-based remote connections
/// - `LocalConnection` - Local system execution
/// - `DockerConnection` - Docker container connections
/// - `KubernetesConnection` - Kubernetes pod connections
#[async_trait]
pub trait Connection: Send + Sync + Debug {
    /// Returns the connection type name (e.g., "ssh", "local", "docker").
    fn connection_type(&self) -> &str;

    /// Returns the target host identifier.
    fn target(&self) -> &str;

    /// Checks if the connection is currently active.
    fn is_connected(&self) -> bool;

    /// Establishes the connection to the target.
    async fn connect(&mut self) -> Result<()>;

    /// Closes the connection gracefully.
    async fn disconnect(&mut self) -> Result<()>;

    /// Executes a command on the target and returns the output.
    ///
    /// # Arguments
    ///
    /// * `command` - The command to execute
    /// * `options` - Execution options (timeout, environment, etc.)
    ///
    /// # Returns
    ///
    /// A `CommandResult` containing stdout, stderr, and exit code.
    async fn execute_command(
        &self,
        command: &str,
        options: &CommandOptions,
    ) -> Result<CommandResult>;

    /// Copies a file to the target host.
    ///
    /// # Arguments
    ///
    /// * `local_path` - Path to the local file
    /// * `remote_path` - Destination path on the target
    /// * `options` - Transfer options (permissions, owner, etc.)
    async fn put_file(
        &self,
        local_path: &std::path::Path,
        remote_path: &std::path::Path,
        options: &FileTransferOptions,
    ) -> Result<()>;

    /// Retrieves a file from the target host.
    ///
    /// # Arguments
    ///
    /// * `remote_path` - Path to the file on the target
    /// * `local_path` - Destination path on the local system
    async fn get_file(
        &self,
        remote_path: &std::path::Path,
        local_path: &std::path::Path,
    ) -> Result<()>;

    /// Writes content directly to a file on the target.
    async fn put_content(
        &self,
        content: &[u8],
        remote_path: &std::path::Path,
        options: &FileTransferOptions,
    ) -> Result<()>;

    /// Reads content directly from a file on the target.
    async fn get_content(&self, remote_path: &std::path::Path) -> Result<Vec<u8>>;

    /// Checks if a path exists on the target.
    async fn path_exists(&self, path: &std::path::Path) -> Result<bool>;

    /// Gets file metadata from the target.
    async fn stat(&self, path: &std::path::Path) -> Result<FileStat>;

    /// Becomes another user (privilege escalation).
    ///
    /// # Arguments
    ///
    /// * `become_config` - Configuration for privilege escalation
    async fn become_user(&mut self, become_config: &BecomeConfig) -> Result<()>;
}

/// Options for command execution.
#[derive(Debug, Clone, Default)]
pub struct CommandOptions {
    /// Working directory for the command
    pub cwd: Option<std::path::PathBuf>,
    /// Environment variables to set
    pub env: HashMap<String, String>,
    /// Timeout in seconds
    pub timeout: Option<u64>,
    /// Whether to use a pseudo-TTY
    pub use_pty: bool,
    /// Stdin input to provide
    pub stdin: Option<String>,
}

/// Result of command execution.
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code (None if process was killed)
    pub exit_code: Option<i32>,
}

impl CommandResult {
    /// Returns true if the command succeeded (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    /// Returns the combined stdout and stderr.
    pub fn output(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }
}

/// Options for file transfers.
#[derive(Debug, Clone, Default)]
pub struct FileTransferOptions {
    /// File mode/permissions (e.g., 0o644)
    pub mode: Option<u32>,
    /// Owner user
    pub owner: Option<String>,
    /// Owner group
    pub group: Option<String>,
    /// Whether to create parent directories
    pub create_parents: bool,
    /// Whether to backup existing file
    pub backup: bool,
}

/// File metadata from target system.
#[derive(Debug, Clone)]
pub struct FileStat {
    /// File mode/permissions
    pub mode: u32,
    /// Owner user ID
    pub uid: u32,
    /// Owner group ID
    pub gid: u32,
    /// File size in bytes
    pub size: u64,
    /// Modification time (Unix timestamp)
    pub mtime: i64,
    /// Whether this is a directory
    pub is_dir: bool,
    /// Whether this is a symlink
    pub is_symlink: bool,
}

/// Configuration for privilege escalation (become).
#[derive(Debug, Clone, Default)]
pub struct BecomeConfig {
    /// Whether to use privilege escalation
    pub enabled: bool,
    /// Method to use (sudo, su, doas, etc.)
    pub method: String,
    /// User to become
    pub user: String,
    /// Password for escalation (if required)
    pub password: Option<String>,
    /// Additional flags for the become method
    pub flags: Option<String>,
}

// ============================================================================
// Inventory Traits
// ============================================================================

/// Source for loading inventory data.
///
/// Inventory sources provide hosts and groups for playbook execution.
/// Multiple sources can be combined into a single inventory.
#[async_trait]
pub trait InventorySource: Send + Sync + Debug {
    /// Returns the name of this inventory source.
    fn name(&self) -> &str;

    /// Loads the inventory from this source.
    async fn load(&self) -> Result<InventoryData>;

    /// Refreshes the inventory (for dynamic sources).
    async fn refresh(&self) -> Result<InventoryData> {
        self.load().await
    }
}

/// Raw inventory data loaded from a source.
#[derive(Debug, Clone, Default)]
pub struct InventoryData {
    /// Hosts in this inventory
    pub hosts: HashMap<String, HostData>,
    /// Groups and their members
    pub groups: HashMap<String, GroupData>,
    /// Global variables
    pub vars: Variables,
}

/// Host data from inventory.
#[derive(Debug, Clone, Default)]
pub struct HostData {
    /// Hostname or address
    pub name: String,
    /// Host-specific variables
    pub vars: Variables,
    /// Groups this host belongs to
    pub groups: Vec<String>,
}

/// Group data from inventory.
#[derive(Debug, Clone, Default)]
pub struct GroupData {
    /// Group name
    pub name: String,
    /// Hosts in this group
    pub hosts: Vec<String>,
    /// Child groups
    pub children: Vec<String>,
    /// Group-specific variables
    pub vars: Variables,
}

// ============================================================================
// Task Traits
// ============================================================================

/// Represents a task that can be executed.
///
/// Tasks are the basic units of execution in a playbook. Each task
/// typically invokes a module with specific arguments.
#[async_trait]
pub trait Executable: Send + Sync + Debug {
    /// Returns the name of this executable.
    fn name(&self) -> &str;

    /// Executes this task with the given context.
    async fn execute(&self, ctx: &ExecutionContext) -> Result<ExecutionResult>;

    /// Checks if this task should be skipped based on conditions.
    fn should_skip(&self, ctx: &ExecutionContext) -> Result<bool> {
        let _ = ctx;
        Ok(false)
    }
}

/// Context provided during task execution.
#[derive(Debug)]
pub struct ExecutionContext {
    /// The connection to the target host
    pub connection: Arc<dyn Connection>,
    /// Facts gathered from the host
    pub facts: Facts,
    /// Variables available during execution
    pub variables: Variables,
    /// Whether we're in check mode (dry run)
    pub check_mode: bool,
    /// Whether to show diffs
    pub diff_mode: bool,
    /// Verbosity level
    pub verbosity: u8,
}

/// Result of task execution.
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// The host this was executed on
    pub host: String,
    /// Name of the task
    pub task_name: String,
    /// The module result
    pub result: ModuleResult,
    /// Duration of execution
    pub duration: std::time::Duration,
    /// Handlers to notify
    pub notify: Vec<String>,
}

// ============================================================================
// Strategy Traits
// ============================================================================

/// Execution strategy for running tasks across hosts.
///
/// Strategies control how tasks are distributed and executed across
/// the target hosts in a play.
#[async_trait]
pub trait ExecutionStrategy: Send + Sync + Debug {
    /// Returns the name of this strategy.
    fn name(&self) -> &str;

    /// Executes tasks according to this strategy.
    ///
    /// # Arguments
    ///
    /// * `tasks` - The tasks to execute
    /// * `hosts` - The target hosts
    /// * `ctx_factory` - Factory for creating execution contexts
    async fn execute<F>(
        &self,
        tasks: &[Arc<dyn Executable>],
        hosts: &[String],
        ctx_factory: F,
    ) -> Result<Vec<ExecutionResult>>
    where
        F: Fn(&str) -> ExecutionContext + Send + Sync;
}

// ============================================================================
// Callback Traits
// ============================================================================

/// Callback for receiving execution events.
///
/// Callbacks allow customizing the output and handling of execution
/// events such as task start, completion, and failure.
#[async_trait]
pub trait ExecutionCallback: Send + Sync {
    /// Called when a playbook starts.
    async fn on_playbook_start(&self, name: &str) {
        let _ = name;
    }

    /// Called when a playbook ends.
    async fn on_playbook_end(&self, name: &str, success: bool) {
        let _ = (name, success);
    }

    /// Called when a play starts.
    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let _ = (name, hosts);
    }

    /// Called when a play ends.
    async fn on_play_end(&self, name: &str, success: bool) {
        let _ = (name, success);
    }

    /// Called when a task starts.
    async fn on_task_start(&self, name: &str, host: &str) {
        let _ = (name, host);
    }

    /// Called when a task completes.
    async fn on_task_complete(&self, result: &ExecutionResult) {
        let _ = result;
    }

    /// Called when a handler is triggered.
    async fn on_handler_triggered(&self, name: &str) {
        let _ = name;
    }

    /// Called when facts are gathered.
    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let _ = (host, facts);
    }
}

// ============================================================================
// Filter/Test Traits for Templates
// ============================================================================

/// Custom filter for template engine.
pub trait TemplateFilter: Send + Sync {
    /// Returns the name of this filter.
    fn name(&self) -> &str;

    /// Applies the filter to a value.
    fn apply(
        &self,
        value: &serde_json::Value,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value>;
}

/// Custom test for template engine.
pub trait TemplateTest: Send + Sync {
    /// Returns the name of this test.
    fn name(&self) -> &str;

    /// Evaluates the test.
    fn test(&self, value: &serde_json::Value, args: &[serde_json::Value]) -> Result<bool>;
}
