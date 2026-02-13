//! Security Module for Privilege Escalation
//!
//! This module provides comprehensive security controls for privilege escalation
//! (become) functionality in Rustible, including:
//!
//! - **Input Validation**: Strict validation of usernames, methods, and paths
//! - **Password Caching**: Secure password caching with configurable TTL
//! - **Audit Trail**: Comprehensive logging of all privileged operations
//! - **Least Privilege**: Enforcement options for minimal privilege escalation
//!
//! ## Security Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    Privilege Escalation Request                      │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Input Validation Layer                          │
//! │              (username, method, path sanitization)                   │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    Least Privilege Enforcement                       │
//! │              (allowlist, command restrictions)                       │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Password Cache Layer                            │
//! │              (TTL-based, memory-safe storage)                        │
//! └─────────────────────────────────────────────────────────────────────┘
//!                                    │
//!                                    ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Audit Trail Logger                              │
//! │              (structured logging, compliance)                        │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::security::{
//!     BecomeValidator, PasswordCache, AuditLogger, LeastPrivilegePolicy,
//! };
//! # let password = "secret";
//!
//! // Validate escalation request
//! let validator = BecomeValidator::new();
//! validator.validate_username("www-data")?;
//! validator.validate_method("sudo")?;
//!
//! // Cache password with TTL
//! let cache = PasswordCache::new();
//! cache.store("host1", "root", password);
//!
//! // Log privileged operation
//! let logger = AuditLogger::new();
//! logger.log_escalation_start("host1", "root", "sudo", "apt install nginx");
//! # Ok(())
//! # }
//! ```

pub mod audit;
pub mod input;
pub mod password_cache;
pub mod path;
pub mod rate_limit;
pub mod rbac;
pub mod secret;
pub mod signing;
pub mod template;
pub mod validation;

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use thiserror::Error;

// Re-export main types
pub use audit::{AuditEntry, AuditLogger, AuditSeverity};
pub use input::{
    sanitize_shell_arg, validate_hostname, validate_identifier, validate_url, InputValidator,
    SanitizationLevel,
};
pub use password_cache::{CachedPassword, PasswordCache, PasswordCacheConfig};
pub use path::{
    validate_path_no_traversal, validate_path_strict, validate_path_within_base, PathSecurityError,
};
pub use rate_limit::{RateLimiter, RateLimiterConfig};
pub use secret::{SecretBytes, SecretString};
pub use template::{TemplateSanitizer, TemplateSecurityPolicy};
pub use validation::{BecomeValidator, ValidationResult};

/// Errors that can occur during security operations
#[derive(Error, Debug, Clone)]
pub enum SecurityError {
    #[error("Invalid username: {0}")]
    InvalidUsername(String),

    #[error("Unsupported escalation method: {0}")]
    UnsupportedMethod(String),

    #[error("Invalid path contains shell metacharacters: {0}")]
    InvalidPath(String),

    #[error("Command injection detected: {0}")]
    CommandInjection(String),

    #[error("Privilege escalation denied by policy: {0}")]
    PolicyDenied(String),

    #[error("Password cache expired for {0}")]
    PasswordExpired(String),

    #[error("Audit logging failed: {0}")]
    AuditFailed(String),

    #[error("Invalid environment variable name: {0}")]
    InvalidEnvName(String),

    #[error("Path traversal detected: {0}")]
    PathTraversal(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    #[error("Template security violation: {0}")]
    TemplateViolation(String),
}

/// Result type for security operations
pub type SecurityResult<T> = Result<T, SecurityError>;

/// Supported privilege escalation methods
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EscalationMethod {
    /// sudo - Superuser do (most common)
    Sudo,
    /// su - Switch user
    Su,
    /// doas - OpenBSD privilege elevation
    Doas,
    /// pbrun - PowerBroker
    Pbrun,
    /// pfexec - Solaris privilege execution
    Pfexec,
    /// runas - Windows run as
    Runas,
    /// dzdo - Centrify DirectAuthorize
    Dzdo,
    /// ksu - Kerberos su
    Ksu,
    /// pmrun - Privilege Manager
    Pmrun,
}

impl EscalationMethod {
    /// All supported methods
    pub const ALL: &'static [EscalationMethod] = &[
        EscalationMethod::Sudo,
        EscalationMethod::Su,
        EscalationMethod::Doas,
        EscalationMethod::Pbrun,
        EscalationMethod::Pfexec,
        EscalationMethod::Runas,
        EscalationMethod::Dzdo,
        EscalationMethod::Ksu,
        EscalationMethod::Pmrun,
    ];

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "sudo" => Some(EscalationMethod::Sudo),
            "su" => Some(EscalationMethod::Su),
            "doas" => Some(EscalationMethod::Doas),
            "pbrun" => Some(EscalationMethod::Pbrun),
            "pfexec" => Some(EscalationMethod::Pfexec),
            "runas" => Some(EscalationMethod::Runas),
            "dzdo" => Some(EscalationMethod::Dzdo),
            "ksu" => Some(EscalationMethod::Ksu),
            "pmrun" => Some(EscalationMethod::Pmrun),
            _ => None,
        }
    }

    /// Get the command name
    pub fn command(&self) -> &'static str {
        match self {
            EscalationMethod::Sudo => "sudo",
            EscalationMethod::Su => "su",
            EscalationMethod::Doas => "doas",
            EscalationMethod::Pbrun => "pbrun",
            EscalationMethod::Pfexec => "pfexec",
            EscalationMethod::Runas => "runas",
            EscalationMethod::Dzdo => "dzdo",
            EscalationMethod::Ksu => "ksu",
            EscalationMethod::Pmrun => "pmrun",
        }
    }

    /// Check if method supports password via stdin
    pub fn supports_stdin_password(&self) -> bool {
        matches!(
            self,
            EscalationMethod::Sudo | EscalationMethod::Su | EscalationMethod::Doas
        )
    }
}

impl std::str::FromStr for EscalationMethod {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EscalationMethod::from_str(s).ok_or(())
    }
}

impl fmt::Display for EscalationMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.command())
    }
}

/// Least privilege enforcement policy
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeastPrivilegePolicy {
    /// Allowed target users (empty = all allowed)
    #[serde(default)]
    pub allowed_users: HashSet<String>,

    /// Denied target users
    #[serde(default)]
    pub denied_users: HashSet<String>,

    /// Allowed escalation methods (empty = all supported)
    #[serde(default)]
    pub allowed_methods: HashSet<String>,

    /// Command prefixes that are allowed
    #[serde(default)]
    pub allowed_command_prefixes: Vec<String>,

    /// Command prefixes that are denied
    #[serde(default)]
    pub denied_command_prefixes: Vec<String>,

    /// Whether to require explicit become for each task
    #[serde(default)]
    pub require_explicit_become: bool,

    /// Maximum escalation timeout in seconds
    #[serde(default)]
    pub max_escalation_timeout: Option<u64>,

    /// Whether to audit all privileged operations
    #[serde(default = "default_true")]
    pub audit_all_operations: bool,

    /// Restrict to NOPASSWD operations only
    #[serde(default)]
    pub nopasswd_only: bool,
}

fn default_true() -> bool {
    true
}

impl LeastPrivilegePolicy {
    /// Create a new permissive policy (minimal restrictions)
    pub fn permissive() -> Self {
        Self::default()
    }

    /// Create a restrictive policy (root only, sudo only)
    pub fn restrictive() -> Self {
        let mut policy = Self::default();
        policy.allowed_users.insert("root".to_string());
        policy.allowed_methods.insert("sudo".to_string());
        policy.audit_all_operations = true;
        policy
    }

    /// Create a locked-down policy (explicit allowlist required)
    pub fn locked_down() -> Self {
        Self {
            allowed_users: HashSet::new(),
            denied_users: HashSet::new(),
            allowed_methods: HashSet::from(["sudo".to_string()]),
            allowed_command_prefixes: Vec::new(),
            denied_command_prefixes: vec![
                "rm -rf /".to_string(),
                "dd if=".to_string(),
                "mkfs".to_string(),
                "shutdown".to_string(),
                "reboot".to_string(),
                "init 0".to_string(),
                "init 6".to_string(),
            ],
            require_explicit_become: true,
            max_escalation_timeout: Some(300), // 5 minutes max
            audit_all_operations: true,
            nopasswd_only: false,
        }
    }

    /// Check if a user is allowed
    pub fn is_user_allowed(&self, user: &str) -> bool {
        // Check denied list first
        if self.denied_users.contains(user) {
            return false;
        }

        // If allowed list is empty, allow all (except denied)
        if self.allowed_users.is_empty() {
            return true;
        }

        // Otherwise, must be in allowed list
        self.allowed_users.contains(user)
    }

    /// Check if a method is allowed
    pub fn is_method_allowed(&self, method: &str) -> bool {
        if self.allowed_methods.is_empty() {
            // If no allowed methods specified, allow all supported
            EscalationMethod::from_str(method).is_some()
        } else {
            self.allowed_methods.contains(method)
        }
    }

    /// Check if a command is allowed
    pub fn is_command_allowed(&self, command: &str) -> bool {
        // Check denied prefixes
        for denied in &self.denied_command_prefixes {
            if command.starts_with(denied) {
                return false;
            }
        }

        // If allowed prefixes specified, command must match one
        if !self.allowed_command_prefixes.is_empty() {
            return self
                .allowed_command_prefixes
                .iter()
                .any(|prefix| command.starts_with(prefix));
        }

        true
    }

    /// Validate an escalation request against the policy
    pub fn validate_request(&self, user: &str, method: &str, command: &str) -> SecurityResult<()> {
        if !self.is_user_allowed(user) {
            return Err(SecurityError::PolicyDenied(format!(
                "User '{}' is not allowed by policy",
                user
            )));
        }

        if !self.is_method_allowed(method) {
            return Err(SecurityError::PolicyDenied(format!(
                "Escalation method '{}' is not allowed by policy",
                method
            )));
        }

        if !self.is_command_allowed(command) {
            return Err(SecurityError::PolicyDenied(format!(
                "Command '{}' is not allowed by policy",
                command
            )));
        }

        Ok(())
    }
}

/// Enhanced ExecuteOptions with security features
///
/// The escalation password is stored in a `SecretString` which automatically
/// zeroes memory on drop, preventing secret leakage.
#[derive(Clone, Default)]
pub struct SecureExecuteOptions {
    /// Working directory for the command
    pub cwd: Option<String>,

    /// Environment variables to set
    pub env: std::collections::HashMap<String, String>,

    /// Timeout in seconds
    pub timeout: Option<u64>,

    /// Run command with privilege escalation
    pub escalate: bool,

    /// User to escalate to (default: root)
    pub escalate_user: Option<String>,

    /// Method for privilege escalation
    pub escalate_method: Option<String>,

    /// Password for privilege escalation (stored in SecretString for auto-zeroization)
    escalate_password: Option<SecretString>,

    /// Custom flags for the escalation method
    pub escalate_flags: Option<String>,

    /// Whether this operation has been validated
    validated: bool,
}

impl SecureExecuteOptions {
    /// Create new secure execute options
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the working directory with validation
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> SecurityResult<Self> {
        let cwd_str = cwd.into();
        BecomeValidator::new().validate_path(&cwd_str)?;
        self.cwd = Some(cwd_str);
        Ok(self)
    }

    /// Enable privilege escalation with validation
    pub fn with_escalation(
        mut self,
        user: Option<String>,
        method: Option<String>,
    ) -> SecurityResult<Self> {
        let validator = BecomeValidator::new();

        if let Some(ref u) = user {
            validator.validate_username(u)?;
        }

        if let Some(ref m) = method {
            validator.validate_method(m)?;
        }

        self.escalate = true;
        self.escalate_user = user;
        self.escalate_method = method;
        self.validated = true;
        Ok(self)
    }

    /// Set the escalation password (stored in SecretString for auto-zeroization)
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        self.escalate_password = Some(SecretString::new(password));
        self
    }

    /// Set custom escalation flags
    pub fn with_flags(mut self, flags: impl Into<String>) -> Self {
        self.escalate_flags = Some(flags.into());
        self
    }

    /// Get the password (for internal use only)
    pub(crate) fn password(&self) -> Option<&str> {
        self.escalate_password.as_ref().map(|s| s.expose())
    }

    /// Check if options have been validated
    pub fn is_validated(&self) -> bool {
        self.validated
    }

    /// Clear the password from memory (happens automatically on drop via SecretString)
    pub fn clear_password(&mut self) {
        // SecretString handles zeroization automatically when dropped
        self.escalate_password = None;
    }
}

// Custom Debug implementation to redact password
impl std::fmt::Debug for SecureExecuteOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecureExecuteOptions")
            .field("cwd", &self.cwd)
            .field("env", &self.env)
            .field("timeout", &self.timeout)
            .field("escalate", &self.escalate)
            .field("escalate_user", &self.escalate_user)
            .field("escalate_method", &self.escalate_method)
            .field(
                "escalate_password",
                &self.escalate_password.as_ref().map(|_| "[REDACTED]"),
            )
            .field("escalate_flags", &self.escalate_flags)
            .field("validated", &self.validated)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escalation_method_parsing() {
        assert_eq!(
            EscalationMethod::from_str("sudo"),
            Some(EscalationMethod::Sudo)
        );
        assert_eq!(
            EscalationMethod::from_str("SUDO"),
            Some(EscalationMethod::Sudo)
        );
        assert_eq!(
            EscalationMethod::from_str("doas"),
            Some(EscalationMethod::Doas)
        );
        assert_eq!(EscalationMethod::from_str("unknown"), None);
    }

    #[test]
    fn test_least_privilege_policy() {
        let policy = LeastPrivilegePolicy::restrictive();

        assert!(policy.is_user_allowed("root"));
        assert!(!policy.is_user_allowed("admin")); // Not in allowed list

        assert!(policy.is_method_allowed("sudo"));
        assert!(!policy.is_method_allowed("su")); // Not in allowed list
    }

    #[test]
    fn test_locked_down_policy_denied_commands() {
        let policy = LeastPrivilegePolicy::locked_down();

        assert!(!policy.is_command_allowed("rm -rf /"));
        assert!(!policy.is_command_allowed("shutdown now"));
        assert!(policy.is_command_allowed("apt install nginx"));
    }

    #[test]
    fn test_secure_execute_options_password_redaction() {
        let options = SecureExecuteOptions::new().with_password("secret_password".to_string());

        let debug_output = format!("{:?}", options);
        assert!(debug_output.contains("[REDACTED]"));
        assert!(!debug_output.contains("secret_password"));
    }

    #[test]
    fn test_security_error_display() {
        let err = SecurityError::InvalidUsername("root; rm -rf /".to_string());
        assert!(err.to_string().contains("Invalid username"));

        let err = SecurityError::UnsupportedMethod("unknown".to_string());
        assert!(err.to_string().contains("Unsupported escalation method"));
    }
}
