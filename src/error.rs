//! Error types for Rustible.
//!
//! This module defines the error types used throughout Rustible, providing
//! rich error information for debugging and user feedback.

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for Rustible operations.
pub type Result<T> = std::result::Result<T, Error>;

/// The main error type for Rustible.
#[derive(Error, Debug)]
pub enum Error {
    // ========================================================================
    // Playbook Errors
    // ========================================================================
    /// Error parsing a playbook file.
    #[error("Failed to parse playbook '{path}': {message}")]
    PlaybookParse {
        /// Path to the playbook file
        path: PathBuf,
        /// Error message
        message: String,
        /// Source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Error validating playbook structure.
    #[error("Playbook validation failed: {0}")]
    PlaybookValidation(String),

    /// Play not found.
    #[error("Play '{0}' not found in playbook")]
    PlayNotFound(String),

    // ========================================================================
    // Task Errors
    // ========================================================================
    /// Task execution failed.
    #[error("Task '{task}' failed on host '{host}': {message}")]
    TaskFailed {
        /// Task name
        task: String,
        /// Target host
        host: String,
        /// Error message
        message: String,
    },

    /// Task timeout.
    #[error("Task '{task}' timed out on host '{host}' after {timeout_secs} seconds")]
    TaskTimeout {
        /// Task name
        task: String,
        /// Target host
        host: String,
        /// Timeout in seconds
        timeout_secs: u64,
    },

    /// Task skipped due to condition.
    #[error("Task '{0}' skipped")]
    TaskSkipped(String),

    // ========================================================================
    // Module Errors
    // ========================================================================
    /// Module not found.
    #[error("Module '{0}' not found")]
    ModuleNotFound(String),

    /// Invalid module arguments.
    #[error("Invalid arguments for module '{module}': {message}")]
    ModuleArgs {
        /// Module name
        module: String,
        /// Error message
        message: String,
    },

    /// Module execution failed.
    #[error("Module '{module}' execution failed: {message}")]
    ModuleExecution {
        /// Module name
        module: String,
        /// Error message
        message: String,
    },

    // ========================================================================
    // Inventory Errors
    // ========================================================================
    /// Error loading inventory.
    #[error("Failed to load inventory from '{path}': {message}")]
    InventoryLoad {
        /// Path to inventory
        path: PathBuf,
        /// Error message
        message: String,
    },

    /// Host not found in inventory.
    #[error("Host '{0}' not found in inventory")]
    HostNotFound(String),

    /// Group not found in inventory.
    #[error("Group '{0}' not found in inventory")]
    GroupNotFound(String),

    /// Invalid host pattern.
    #[error("Invalid host pattern: '{0}'")]
    InvalidHostPattern(String),

    // ========================================================================
    // Connection Errors
    // ========================================================================
    /// Failed to connect to host.
    #[error("Failed to connect to '{host}': {message}")]
    ConnectionFailed {
        /// Target host
        host: String,
        /// Error message
        message: String,
    },

    /// Connection timeout.
    #[error("Connection to '{host}' timed out after {timeout_secs} seconds")]
    ConnectionTimeout {
        /// Target host
        host: String,
        /// Timeout in seconds
        timeout_secs: u64,
    },

    /// Authentication failed.
    #[error("Authentication failed for '{user}@{host}': {message}")]
    AuthenticationFailed {
        /// Username
        user: String,
        /// Target host
        host: String,
        /// Error message
        message: String,
    },

    /// Command execution failed on remote.
    #[error("Command failed on '{host}' with exit code {exit_code}: {message}")]
    RemoteCommandFailed {
        /// Target host
        host: String,
        /// Exit code
        exit_code: i32,
        /// Error message
        message: String,
    },

    /// File transfer failed.
    #[error("File transfer failed: {0}")]
    FileTransfer(String),

    // ========================================================================
    // Variable Errors
    // ========================================================================
    /// Undefined variable.
    #[error("Undefined variable: '{0}'")]
    UndefinedVariable(String),

    /// Invalid variable value.
    #[error("Invalid value for variable '{name}': {message}")]
    InvalidVariableValue {
        /// Variable name
        name: String,
        /// Error message
        message: String,
    },

    /// Variable file not found.
    #[error("Variables file not found: {0}")]
    VariablesFileNotFound(PathBuf),

    // ========================================================================
    // Template Errors
    // ========================================================================
    /// Template syntax error.
    #[error("Template syntax error in '{template}': {message}")]
    TemplateSyntax {
        /// Template name or path
        template: String,
        /// Error message
        message: String,
    },

    /// Template rendering error.
    #[error("Template rendering failed for '{template}': {message}")]
    TemplateRender {
        /// Template name or path
        template: String,
        /// Error message
        message: String,
    },

    // ========================================================================
    // Role Errors
    // ========================================================================
    /// Role not found.
    #[error("Role '{0}' not found")]
    RoleNotFound(String),

    /// Role dependency error.
    #[error("Role dependency error: {0}")]
    RoleDependency(String),

    /// Invalid role structure.
    #[error("Invalid role structure in '{role}': {message}")]
    InvalidRole {
        /// Role name
        role: String,
        /// Error message
        message: String,
    },

    // ========================================================================
    // Vault Errors
    // ========================================================================
    /// Vault decryption failed.
    #[error("Failed to decrypt vault: {0}")]
    VaultDecryption(String),

    /// Vault encryption failed.
    #[error("Failed to encrypt vault: {0}")]
    VaultEncryption(String),

    /// Invalid vault password.
    #[error("Invalid vault password")]
    InvalidVaultPassword,

    /// Vault file not found.
    #[error("Vault file not found: {0}")]
    VaultFileNotFound(PathBuf),

    // ========================================================================
    // Handler Errors
    // ========================================================================
    /// Handler not found.
    #[error("Handler '{0}' not found")]
    HandlerNotFound(String),

    /// Handler execution failed.
    #[error("Handler '{handler}' failed on host '{host}': {message}")]
    HandlerFailed {
        /// Handler name
        handler: String,
        /// Target host
        host: String,
        /// Error message
        message: String,
    },

    // ========================================================================
    // Configuration Errors
    // ========================================================================
    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Invalid configuration value.
    #[error("Invalid configuration value for '{key}': {message}")]
    InvalidConfig {
        /// Configuration key
        key: String,
        /// Error message
        message: String,
    },

    // ========================================================================
    // IO Errors
    // ========================================================================
    /// File not found.
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // ========================================================================
    // Serialization Errors
    // ========================================================================
    /// YAML parsing error.
    #[error("YAML parse error: {0}")]
    YamlParse(#[from] serde_yaml::Error),

    /// JSON parsing error.
    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),

    /// TOML parsing error.
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// Template error.
    #[error("Template error: {0}")]
    Template(#[from] minijinja::Error),

    /// Generic vault error.
    #[error("Vault error: {0}")]
    Vault(String),

    /// State error.
    #[error("State error: {0}")]
    State(String),

    // ========================================================================
    // Other Errors
    // ========================================================================
    /// Strategy error.
    #[error("Execution strategy error: {0}")]
    Strategy(String),

    /// Privilege escalation failed.
    #[error("Privilege escalation failed on '{host}': {message}")]
    BecomeError {
        /// Target host
        host: String,
        /// Error message
        message: String,
    },

    /// Internal error.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Generic error with source.
    #[error("{message}")]
    Other {
        /// Error message
        message: String,
        /// Source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl Error {
    /// Creates a new playbook parse error.
    pub fn playbook_parse(
        path: impl Into<PathBuf>,
        message: impl Into<String>,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    ) -> Self {
        Self::PlaybookParse {
            path: path.into(),
            message: message.into(),
            source,
        }
    }

    /// Creates a new task failed error.
    pub fn task_failed(
        task: impl Into<String>,
        host: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::TaskFailed {
            task: task.into(),
            host: host.into(),
            message: message.into(),
        }
    }

    /// Creates a new connection failed error.
    pub fn connection_failed(host: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ConnectionFailed {
            host: host.into(),
            message: message.into(),
        }
    }

    /// Creates a new module args error.
    pub fn module_args(module: impl Into<String>, message: impl Into<String>) -> Self {
        Self::ModuleArgs {
            module: module.into(),
            message: message.into(),
        }
    }

    /// Creates a new template render error.
    pub fn template_render(template: impl Into<String>, message: impl Into<String>) -> Self {
        Self::TemplateRender {
            template: template.into(),
            message: message.into(),
        }
    }

    /// Returns true if this error is recoverable.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Error::TaskSkipped(_) | Error::ConnectionTimeout { .. } | Error::TaskTimeout { .. }
        )
    }

    /// Returns the error code for CLI exit status.
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::TaskFailed { .. } | Error::ModuleExecution { .. } => 2,
            Error::ConnectionFailed { .. } | Error::AuthenticationFailed { .. } => 3,
            Error::PlaybookParse { .. } | Error::PlaybookValidation(_) => 4,
            Error::InventoryLoad { .. } | Error::HostNotFound(_) => 5,
            Error::VaultDecryption(_) | Error::InvalidVaultPassword => 6,
            _ => 1,
        }
    }
}

// ============================================================================
// Enriched Error Types with Actionable Hints
// ============================================================================

/// Context information for errors to help with debugging and user feedback.
#[derive(Debug, Clone, Default)]
pub struct ErrorContext {
    /// The file where the error occurred
    pub file: Option<PathBuf>,
    /// The line number in the file
    pub line: Option<usize>,
    /// The task name (if applicable)
    pub task: Option<String>,
    /// The play name (if applicable)
    pub play: Option<String>,
    /// The host (if applicable)
    pub host: Option<String>,
}

impl ErrorContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the file path
    pub fn with_file(mut self, file: impl Into<PathBuf>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// Set the line number
    pub fn with_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }

    /// Set the task name
    pub fn with_task(mut self, task: impl Into<String>) -> Self {
        self.task = Some(task.into());
        self
    }

    /// Set the play name
    pub fn with_play(mut self, play: impl Into<String>) -> Self {
        self.play = Some(play.into());
        self
    }

    /// Set the host
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Format the context as a location string
    pub fn location_string(&self) -> String {
        let mut parts = Vec::new();

        if let Some(file) = &self.file {
            let mut loc = format!("File: {}", file.display());
            if let Some(line) = self.line {
                loc.push_str(&format!(", line {}", line));
            }
            parts.push(loc);
        }

        if let Some(play) = &self.play {
            parts.push(format!("Play: '{}'", play));
        }

        if let Some(task) = &self.task {
            parts.push(format!("Task: '{}'", task));
        }

        if let Some(host) = &self.host {
            parts.push(format!("Host: '{}'", host));
        }

        if parts.is_empty() {
            String::new()
        } else {
            parts.join(", ")
        }
    }
}

impl std::fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let loc = self.location_string();
        if !loc.is_empty() {
            write!(f, "\n  Location: {}", loc)
        } else {
            Ok(())
        }
    }
}

/// An enriched error message with actionable hints.
#[derive(Debug, Clone)]
pub struct EnrichedError {
    /// The base error message
    pub message: String,
    /// Hint for how to fix the issue
    pub hint: String,
    /// Additional context (file, line, task, etc.)
    pub context: Option<ErrorContext>,
    /// Suggestions for fixing the issue
    pub suggestions: Vec<String>,
}

impl EnrichedError {
    /// Create a new enriched error
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: String::new(),
            context: None,
            suggestions: Vec::new(),
        }
    }

    /// Add a hint
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// Add context
    pub fn with_context(mut self, context: ErrorContext) -> Self {
        self.context = Some(context);
        self
    }

    /// Add a suggestion
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Format as a complete error message
    pub fn format(&self) -> String {
        let mut output = self.message.clone();

        if !self.hint.is_empty() {
            output.push_str(&format!("\n  Hint: {}", self.hint));
        }

        if let Some(ctx) = &self.context {
            output.push_str(&ctx.to_string());
        }

        if !self.suggestions.is_empty() {
            output.push_str("\n  Suggestions:");
            for suggestion in &self.suggestions {
                output.push_str(&format!("\n    - {}", suggestion));
            }
        }

        output
    }
}

impl std::fmt::Display for EnrichedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.format())
    }
}

// ============================================================================
// Module-specific error hint generators
// ============================================================================

/// Generate module-specific argument hints.
pub fn get_module_args_hint(module: &str) -> String {
    match module {
        "copy" => "Required: src, dest. Optional: owner, group, mode, backup, force.".to_string(),
        "file" => {
            "Required: path. Optional: state (file/directory/link/absent), owner, group, mode."
                .to_string()
        }
        "template" => {
            "Required: src, dest. Optional: owner, group, mode, backup, force.".to_string()
        }
        "command" | "shell" => {
            "Required: cmd (or free-form). Optional: chdir, creates, removes.".to_string()
        }
        "apt" => {
            "Required: name (or pkg). Optional: state, update_cache, cache_valid_time.".to_string()
        }
        "yum" | "dnf" => "Required: name. Optional: state, enablerepo, disablerepo.".to_string(),
        "service" => {
            "Required: name. Optional: state (started/stopped/restarted/reloaded), enabled."
                .to_string()
        }
        "user" => {
            "Required: name. Optional: state, uid, groups, shell, home, password.".to_string()
        }
        "group" => "Required: name. Optional: state, gid.".to_string(),
        "lineinfile" => {
            "Required: path. Optional: line, regexp, state, insertafter, insertbefore.".to_string()
        }
        "blockinfile" => {
            "Required: path, block. Optional: marker, insertafter, insertbefore, state.".to_string()
        }
        _ => "Check the module documentation for required and optional arguments.".to_string(),
    }
}

/// Generate module-specific execution hints.
pub fn get_module_execution_hint(module: &str) -> String {
    match module {
        "command" | "shell" => "Check command spelling and ensure it exists in PATH.".to_string(),
        "copy" | "template" => "Verify source file exists and destination is writable.".to_string(),
        "file" => "Check permissions and ensure parent directories exist.".to_string(),
        "apt" => "Ensure apt repositories are configured and reachable.".to_string(),
        "yum" | "dnf" => "Ensure yum/dnf repositories are configured and reachable.".to_string(),
        "service" => "Verify the service exists and is managed by systemd/init.".to_string(),
        "user" | "group" => {
            "Check if you have permission to manage users/groups (may need become: yes)."
                .to_string()
        }
        _ => "Check task output for specific error details.".to_string(),
    }
}

/// Generate connection-specific hints.
pub fn get_connection_hint(error_message: &str) -> String {
    if error_message.contains("Connection refused") {
        return "Check if SSH service is running and port 22 is accessible.".to_string();
    }
    if error_message.contains("No route to host") {
        return "Check network connectivity and firewall rules.".to_string();
    }
    if error_message.contains("Permission denied") {
        return "Check SSH key permissions (chmod 600) and authorized_keys on remote host."
            .to_string();
    }
    if error_message.contains("Host key verification failed") {
        return "Add host key to known_hosts or use host_key_checking=false.".to_string();
    }
    if error_message.contains("timeout") || error_message.contains("Timeout") {
        return "Increase connection timeout or check for network issues.".to_string();
    }
    "Check network connectivity, SSH key permissions, and host availability.".to_string()
}

/// Generate authentication-specific troubleshooting steps.
pub fn get_auth_troubleshooting() -> Vec<String> {
    vec![
        "Check SSH key permissions: chmod 600 ~/.ssh/id_rsa".to_string(),
        "Verify the correct user is specified in inventory".to_string(),
        "Test SSH manually: ssh <user>@<host>".to_string(),
        "Check if password authentication is required (use --ask-pass)".to_string(),
        "Verify SSH agent has the key loaded: ssh-add -l".to_string(),
        "Check authorized_keys file on remote host".to_string(),
        "Review SSH server logs: /var/log/auth.log".to_string(),
    ]
}

/// Generate become (privilege escalation) suggestions based on method.
pub fn get_become_suggestions(method: &str) -> Vec<String> {
    match method {
        "sudo" => vec![
            "Verify user has sudo privileges: sudo -l".to_string(),
            "Check /etc/sudoers configuration".to_string(),
            "Try with --ask-become-pass if password is required".to_string(),
            "Verify become_user exists on target system".to_string(),
            "Check sudo logs: /var/log/sudo.log".to_string(),
        ],
        "su" => vec![
            "Verify target user password is correct".to_string(),
            "Use --ask-become-pass to provide password interactively".to_string(),
            "Check if 'su' is available on the system".to_string(),
            "Verify become_user exists on target system".to_string(),
        ],
        "doas" => vec![
            "Check /etc/doas.conf configuration".to_string(),
            "Verify user has doas privileges".to_string(),
            "Ensure doas is installed on target system".to_string(),
        ],
        _ => vec![
            "Verify the become_method is correctly configured".to_string(),
            "Check user has necessary privileges".to_string(),
            "Use --ask-become-pass if password required".to_string(),
            "Review privilege escalation settings in rustible.cfg".to_string(),
        ],
    }
}

// ============================================================================
// Enriched Error Constructors
// ============================================================================

impl Error {
    /// Create an enriched task failed error with full context.
    pub fn task_failed_enriched(
        task: impl Into<String>,
        host: impl Into<String>,
        message: impl Into<String>,
        context: Option<ErrorContext>,
    ) -> EnrichedError {
        let task_str = task.into();
        let host_str = host.into();
        let msg_str = message.into();

        EnrichedError {
            message: format!(
                "Task '{}' failed on host '{}': {}",
                task_str, host_str, msg_str
            ),
            hint: "Check the task arguments and ensure the target state is achievable.".to_string(),
            context,
            suggestions: vec![
                "Use '-vvv' for more detailed output".to_string(),
                "Check module documentation for required arguments".to_string(),
                "Verify target host state and permissions".to_string(),
            ],
        }
    }

    /// Create an enriched connection failed error with hints.
    pub fn connection_failed_enriched(
        host: impl Into<String>,
        message: impl Into<String>,
    ) -> EnrichedError {
        let host_str = host.into();
        let msg_str = message.into();
        let hint = get_connection_hint(&msg_str);

        EnrichedError {
            message: format!("Failed to connect to '{}': {}", host_str, msg_str),
            hint,
            context: Some(ErrorContext::new().with_host(host_str)),
            suggestions: vec![
                "Check network connectivity".to_string(),
                "Verify SSH key permissions".to_string(),
                "Ensure host is reachable".to_string(),
                "Test with: ssh <user>@<host>".to_string(),
            ],
        }
    }

    /// Create an enriched authentication failed error.
    pub fn auth_failed_enriched(
        user: impl Into<String>,
        host: impl Into<String>,
        message: impl Into<String>,
    ) -> EnrichedError {
        let user_str = user.into();
        let host_str = host.into();

        EnrichedError {
            message: format!(
                "Authentication failed for '{}@{}': {}",
                user_str,
                host_str,
                message.into()
            ),
            hint: "Check SSH key permissions and authorized_keys configuration.".to_string(),
            context: Some(ErrorContext::new().with_host(host_str)),
            suggestions: get_auth_troubleshooting(),
        }
    }

    /// Create an enriched module args error.
    pub fn module_args_enriched(
        module: impl Into<String>,
        message: impl Into<String>,
        context: Option<ErrorContext>,
    ) -> EnrichedError {
        let module_str = module.into();
        let hint = get_module_args_hint(&module_str);

        EnrichedError {
            message: format!(
                "Invalid arguments for module '{}': {}",
                module_str,
                message.into()
            ),
            hint,
            context,
            suggestions: vec![
                format!("Run 'rustible-doc {}' for module documentation", module_str),
                "Check YAML syntax and indentation".to_string(),
            ],
        }
    }

    /// Create an enriched module execution error.
    pub fn module_execution_enriched(
        module: impl Into<String>,
        message: impl Into<String>,
        context: Option<ErrorContext>,
    ) -> EnrichedError {
        let module_str = module.into();
        let hint = get_module_execution_hint(&module_str);

        EnrichedError {
            message: format!(
                "Module '{}' execution failed: {}",
                module_str,
                message.into()
            ),
            hint,
            context,
            suggestions: vec!["Use '-vvv' for more detailed output".to_string()],
        }
    }

    /// Create an enriched become error.
    pub fn become_failed_enriched(
        host: impl Into<String>,
        message: impl Into<String>,
        method: &str,
    ) -> EnrichedError {
        let host_str = host.into();

        EnrichedError {
            message: format!(
                "Privilege escalation failed on '{}': {}",
                host_str,
                message.into()
            ),
            hint: format!("Using become method: '{}'", method),
            context: Some(ErrorContext::new().with_host(host_str)),
            suggestions: get_become_suggestions(method),
        }
    }

    /// Create an enriched playbook parse error.
    pub fn playbook_parse_enriched(
        path: impl Into<PathBuf>,
        message: impl Into<String>,
        line: Option<usize>,
    ) -> EnrichedError {
        let path_buf = path.into();

        let mut context = ErrorContext::new().with_file(path_buf.clone());
        if let Some(l) = line {
            context = context.with_line(l);
        }

        EnrichedError {
            message: format!(
                "Failed to parse playbook '{}': {}",
                path_buf.display(),
                message.into()
            ),
            hint: "Check YAML syntax and indentation. Use 'yamllint' to validate.".to_string(),
            context: Some(context),
            suggestions: vec![
                "Verify proper YAML indentation (2 spaces recommended)".to_string(),
                "Check for missing colons after keys".to_string(),
                "Ensure strings with special characters are quoted".to_string(),
            ],
        }
    }

    /// Create an enriched undefined variable error.
    pub fn undefined_variable_enriched(
        variable: impl Into<String>,
        similar_vars: &[String],
        context: Option<ErrorContext>,
    ) -> EnrichedError {
        let var_str = variable.into();

        let hint = if similar_vars.is_empty() {
            "Check variable name spelling. Variables are case-sensitive.".to_string()
        } else {
            format!(
                "Did you mean: {}?",
                similar_vars
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        EnrichedError {
            message: format!("Undefined variable: '{}'", var_str),
            hint,
            context,
            suggestions: vec![
                "Define the variable in vars, defaults, or inventory".to_string(),
                "Check for typos in variable name".to_string(),
                "Use 'default' filter: {{ var | default('value') }}".to_string(),
            ],
        }
    }

    /// Create an enriched host not found error.
    pub fn host_not_found_enriched(
        host: impl Into<String>,
        available_hosts: &[String],
    ) -> EnrichedError {
        let host_str = host.into();

        let hint = if available_hosts.is_empty() {
            "Inventory appears to be empty. Check your inventory file path with -i option."
                .to_string()
        } else if available_hosts.len() <= 10 {
            format!("Available hosts: {}", available_hosts.join(", "))
        } else {
            format!(
                "Did you mean: {}? ({} total hosts)",
                available_hosts
                    .iter()
                    .take(5)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                available_hosts.len()
            )
        };

        EnrichedError {
            message: format!("Host '{}' not found in inventory", host_str),
            hint,
            context: None,
            suggestions: vec![
                "Run 'rustible list-hosts -i <inventory>' to see all hosts".to_string(),
                "Check inventory file syntax".to_string(),
                "Verify host is in the correct group".to_string(),
            ],
        }
    }

    /// Create an enriched handler not found error.
    pub fn handler_not_found_enriched(
        handler: impl Into<String>,
        available_handlers: &[String],
    ) -> EnrichedError {
        let handler_str = handler.into();

        let hint = if available_handlers.is_empty() {
            "No handlers defined in this play.".to_string()
        } else {
            format!("Available handlers: {}", available_handlers.join(", "))
        };

        EnrichedError {
            message: format!("Handler '{}' not found", handler_str),
            hint,
            context: None,
            suggestions: vec![
                "Check handler name matches exactly (case-sensitive)".to_string(),
                "Ensure handler is defined in 'handlers' section".to_string(),
                "Consider using 'listen' for multiple trigger names".to_string(),
            ],
        }
    }

    /// Create an enriched role not found error.
    pub fn role_not_found_enriched(
        role: impl Into<String>,
        searched_paths: &[PathBuf],
    ) -> EnrichedError {
        let role_str = role.into();
        let paths = searched_paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");

        EnrichedError {
            message: format!("Role '{}' not found", role_str),
            hint: format!(
                "Searched paths: {}",
                if paths.is_empty() { "./roles" } else { &paths }
            ),
            context: None,
            suggestions: vec![
                "Check role name spelling".to_string(),
                "Install missing roles with 'rustible-galaxy install'".to_string(),
                "Verify roles_path in configuration".to_string(),
            ],
        }
    }
}

/// Extension trait for adding context to errors.
pub trait ResultExt<T> {
    /// Adds context to an error.
    fn context(self, message: impl Into<String>) -> Result<T>;

    /// Adds context with a closure that is only evaluated on error.
    fn with_context<F, S>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> S,
        S: Into<String>;
}

impl<T, E> ResultExt<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn context(self, message: impl Into<String>) -> Result<T> {
        self.map_err(|e| Error::Other {
            message: message.into(),
            source: Some(Box::new(e)),
        })
    }

    fn with_context<F, S>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> S,
        S: Into<String>,
    {
        self.map_err(|e| Error::Other {
            message: f().into(),
            source: Some(Box::new(e)),
        })
    }
}
