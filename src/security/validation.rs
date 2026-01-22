//! Input Validation for Privilege Escalation
//!
//! This module provides strict validation for all inputs used in privilege
//! escalation to prevent command injection and other security vulnerabilities.

use super::{EscalationMethod, SecurityError, SecurityResult};
use crate::utils::shell_escape;
use once_cell::sync::Lazy;
use regex::Regex;

/// Cached regex for POSIX username validation
/// Pattern: starts with letter/underscore, contains alphanumeric/underscore/hyphen,
/// may end with $ (for Samba machine accounts)
static USERNAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z_][a-z0-9_-]{0,31}\$?$").expect("Invalid username regex"));

/// Cached regex for safe path characters
static SAFE_PATH_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z0-9/_.-]+$").expect("Invalid path regex"));

/// Cached regex for environment variable names
static ENV_NAME_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").expect("Invalid env name regex"));

/// Shell metacharacters that indicate potential injection
const SHELL_METACHARACTERS: &[char] = &[
    ';', '|', '&', '$', '`', '(', ')', '{', '}', '[', ']', '<', '>', '!', '\n', '\r', '\0', '"',
    '\'', '\\', '*', '?', '~', '#',
];

/// Result of a validation operation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether validation passed
    pub valid: bool,
    /// Validation message
    pub message: String,
    /// Sanitized value (if applicable)
    pub sanitized: Option<String>,
}

impl ValidationResult {
    /// Create a successful validation result
    pub fn ok() -> Self {
        Self {
            valid: true,
            message: "Validation passed".to_string(),
            sanitized: None,
        }
    }

    /// Create a successful validation result with sanitized value
    pub fn ok_with_sanitized(sanitized: String) -> Self {
        Self {
            valid: true,
            message: "Validation passed".to_string(),
            sanitized: Some(sanitized),
        }
    }

    /// Create a failed validation result
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            valid: false,
            message: message.into(),
            sanitized: None,
        }
    }
}

/// Validator for privilege escalation inputs
#[derive(Debug, Clone)]
pub struct BecomeValidator {
    /// Maximum username length
    max_username_length: usize,
    /// Maximum path length
    max_path_length: usize,
    /// Allow uppercase usernames (non-POSIX compliant)
    allow_uppercase_usernames: bool,
    /// Strict mode - reject any suspicious input
    strict_mode: bool,
}

impl Default for BecomeValidator {
    fn default() -> Self {
        Self {
            max_username_length: 32,
            max_path_length: 4096,
            allow_uppercase_usernames: false,
            strict_mode: true,
        }
    }
}

impl BecomeValidator {
    /// Create a new validator with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a strict validator
    pub fn strict() -> Self {
        Self {
            strict_mode: true,
            ..Self::default()
        }
    }

    /// Create a permissive validator (use with caution)
    pub fn permissive() -> Self {
        Self {
            strict_mode: false,
            allow_uppercase_usernames: true,
            ..Self::default()
        }
    }

    /// Set maximum username length
    pub fn with_max_username_length(mut self, len: usize) -> Self {
        self.max_username_length = len;
        self
    }

    /// Allow uppercase usernames
    pub fn with_uppercase_usernames(mut self, allow: bool) -> Self {
        self.allow_uppercase_usernames = allow;
        self
    }

    /// Validate a username for privilege escalation
    ///
    /// POSIX usernames must:
    /// - Start with a lowercase letter or underscore
    /// - Contain only lowercase letters, digits, underscores, hyphens
    /// - Be at most 32 characters (configurable)
    pub fn validate_username(&self, username: &str) -> SecurityResult<()> {
        // Empty check
        if username.is_empty() {
            return Err(SecurityError::InvalidUsername(
                "Username cannot be empty".to_string(),
            ));
        }

        // Length check
        if username.len() > self.max_username_length {
            return Err(SecurityError::InvalidUsername(format!(
                "Username exceeds maximum length of {} characters",
                self.max_username_length
            )));
        }

        // Check for shell metacharacters first (critical security check)
        if self.contains_shell_metacharacters(username) {
            return Err(SecurityError::CommandInjection(format!(
                "Username contains shell metacharacters: {}",
                username
            )));
        }

        // POSIX compliance check
        let username_to_check = if self.allow_uppercase_usernames {
            username.to_lowercase()
        } else {
            username.to_string()
        };

        if !USERNAME_REGEX.is_match(&username_to_check) {
            return Err(SecurityError::InvalidUsername(format!(
                "Username '{}' does not match POSIX pattern (must start with letter/underscore, \
                 contain only lowercase letters, digits, underscores, hyphens)",
                username
            )));
        }

        // Additional strict checks
        if self.strict_mode {
            // Reject usernames that look like command injection attempts
            let suspicious_patterns = ["root;", "root&&", "root||", "root|", "$(", "`", "${"];
            for pattern in suspicious_patterns {
                if username.contains(pattern) {
                    return Err(SecurityError::CommandInjection(format!(
                        "Username contains suspicious pattern: {}",
                        pattern
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validate an escalation method
    pub fn validate_method(&self, method: &str) -> SecurityResult<()> {
        // Empty check
        if method.is_empty() {
            return Err(SecurityError::UnsupportedMethod(
                "Escalation method cannot be empty".to_string(),
            ));
        }

        // Check for injection in method name
        if self.contains_shell_metacharacters(method) {
            return Err(SecurityError::CommandInjection(format!(
                "Escalation method contains shell metacharacters: {}",
                method
            )));
        }

        // Validate against allowlist
        if EscalationMethod::from_str(method).is_none() {
            return Err(SecurityError::UnsupportedMethod(format!(
                "Unsupported escalation method '{}'. Supported methods: {}",
                method,
                EscalationMethod::ALL
                    .iter()
                    .map(|m| m.command())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        Ok(())
    }

    /// Validate a path for shell safety
    pub fn validate_path(&self, path: &str) -> SecurityResult<()> {
        // Empty check
        if path.is_empty() {
            return Err(SecurityError::InvalidPath(
                "Path cannot be empty".to_string(),
            ));
        }

        // Length check
        if path.len() > self.max_path_length {
            return Err(SecurityError::InvalidPath(format!(
                "Path exceeds maximum length of {} characters",
                self.max_path_length
            )));
        }

        // Check for null bytes (critical - can truncate paths)
        if path.contains('\0') {
            return Err(SecurityError::CommandInjection(
                "Path contains null byte".to_string(),
            ));
        }

        // Check for shell metacharacters
        if self.contains_shell_metacharacters(path) {
            // In strict mode, reject outright
            if self.strict_mode {
                return Err(SecurityError::InvalidPath(format!(
                    "Path contains shell metacharacters: {}",
                    path
                )));
            }
            // Otherwise, we'll need to escape it (handled by caller)
        }

        // Check for directory traversal attempts
        if path.contains("..") && self.strict_mode {
            // Note: This is a heuristic - ".." in paths can be legitimate
            // but in privilege escalation context, it's suspicious
            tracing::warn!(
                path = %path,
                "Path contains parent directory reference (..)"
            );
        }

        Ok(())
    }

    /// Validate an environment variable name
    pub fn validate_env_name(&self, name: &str) -> SecurityResult<()> {
        if name.is_empty() {
            return Err(SecurityError::InvalidEnvName(
                "Environment variable name cannot be empty".to_string(),
            ));
        }

        if !ENV_NAME_REGEX.is_match(name) {
            return Err(SecurityError::InvalidEnvName(format!(
                "Invalid environment variable name '{}' (must start with letter/underscore, \
                 contain only alphanumeric and underscores)",
                name
            )));
        }

        Ok(())
    }

    /// Validate environment variable value (check for injection)
    pub fn validate_env_value(&self, value: &str) -> SecurityResult<()> {
        // Check for newline injection
        if value.contains('\n') || value.contains('\r') {
            return Err(SecurityError::CommandInjection(
                "Environment variable value contains newline".to_string(),
            ));
        }

        // Check for null byte
        if value.contains('\0') {
            return Err(SecurityError::CommandInjection(
                "Environment variable value contains null byte".to_string(),
            ));
        }

        Ok(())
    }

    /// Check if a string contains shell metacharacters
    fn contains_shell_metacharacters(&self, s: &str) -> bool {
        s.chars().any(|c| SHELL_METACHARACTERS.contains(&c))
    }

    /// Escape a path for safe shell usage
    pub fn escape_path(&self, path: &str) -> String {
        // If path contains only safe characters, return as-is
        if SAFE_PATH_REGEX.is_match(path) {
            return path.to_string();
        }

        // Otherwise, wrap in single quotes and escape internal single quotes
        format!("'{}'", path.replace('\'', "'\\''"))
    }

    /// Escape a string for safe shell usage
    pub fn shell_escape(&self, s: &str) -> String {
        shell_escape(s).into_owned()
    }

    /// Validate become flags
    pub fn validate_flags(&self, flags: &str) -> SecurityResult<()> {
        // Check for dangerous patterns in flags
        let dangerous_patterns = [
            "$(", "`", "${", // Command substitution
            ";", "&&", "||", // Command chaining
            "|", ">", "<", // Pipes and redirects
            "\n", "\r", "\0", // Control characters
        ];

        for pattern in dangerous_patterns {
            if flags.contains(pattern) {
                return Err(SecurityError::CommandInjection(format!(
                    "Become flags contain dangerous pattern: {}",
                    pattern
                )));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_usernames() {
        let validator = BecomeValidator::new();

        let valid_names = vec![
            "root",
            "admin",
            "www-data",
            "nginx",
            "postgres",
            "mysql",
            "nobody",
            "user123",
            "test_user",
            "test-user",
            "_apt",
            "systemd-network",
        ];

        for name in valid_names {
            assert!(
                validator.validate_username(name).is_ok(),
                "Username should be valid: {}",
                name
            );
        }
    }

    #[test]
    fn test_invalid_usernames() {
        let validator = BecomeValidator::new();

        let long_name = "a".repeat(256);
        let invalid_names = vec![
            ("", "empty"),
            ("root; rm -rf /", "semicolon injection"),
            ("root$(whoami)", "command substitution dollar"),
            ("root`id`", "command substitution backtick"),
            ("root|cat /etc/shadow", "pipe injection"),
            ("root||malicious", "or-chain injection"),
            ("root&&malicious", "and-chain injection"),
            ("root\nrm -rf /", "newline injection"),
            ("root\x00null", "null byte injection"),
            ("root'malicious", "single quote escape"),
            ("root\"malicious", "double quote escape"),
            ("123user", "starts with number"),
            ("-user", "starts with hyphen"),
            ("user$", "shell metacharacter"),
            (long_name.as_str(), "too long"),
        ];

        for (name, description) in invalid_names {
            assert!(
                validator.validate_username(name).is_err(),
                "Username should be invalid: {} ({})",
                name,
                description
            );
        }
    }

    #[test]
    fn test_valid_methods() {
        let validator = BecomeValidator::new();

        for method in EscalationMethod::ALL {
            assert!(
                validator.validate_method(method.command()).is_ok(),
                "Method should be valid: {}",
                method.command()
            );
        }
    }

    #[test]
    fn test_invalid_methods() {
        let validator = BecomeValidator::new();

        let invalid_methods = vec![
            "unknown",
            "sudo2",
            "my_escalator",
            "",
            "sudo; rm -rf /",
            "$(whoami)",
        ];

        for method in invalid_methods {
            assert!(
                validator.validate_method(method).is_err(),
                "Method should be invalid: {}",
                method
            );
        }
    }

    #[test]
    fn test_path_validation() {
        let validator = BecomeValidator::new();

        // Valid paths
        assert!(validator.validate_path("/tmp").is_ok());
        assert!(validator.validate_path("/var/log").is_ok());
        assert!(validator.validate_path("/home/user").is_ok());

        // Invalid paths (shell metacharacters in strict mode)
        assert!(validator.validate_path("/tmp; rm -rf /").is_err());
        assert!(validator.validate_path("/tmp$(whoami)").is_err());
        assert!(validator.validate_path("/tmp`id`").is_err());
        assert!(validator.validate_path("/tmp\0null").is_err());
    }

    #[test]
    fn test_path_escaping() {
        let validator = BecomeValidator::new();

        // Safe paths remain unchanged
        assert_eq!(validator.escape_path("/tmp"), "/tmp");
        assert_eq!(validator.escape_path("/var/log"), "/var/log");

        // Paths with special chars get quoted
        assert_eq!(validator.escape_path("/tmp/file name"), "'/tmp/file name'");
        assert_eq!(
            validator.escape_path("/tmp/file'with'quotes"),
            "'/tmp/file'\\''with'\\''quotes'"
        );
    }

    #[test]
    fn test_env_validation() {
        let validator = BecomeValidator::new();

        // Valid env names
        assert!(validator.validate_env_name("PATH").is_ok());
        assert!(validator.validate_env_name("HOME").is_ok());
        assert!(validator.validate_env_name("MY_VAR").is_ok());
        assert!(validator.validate_env_name("_PRIVATE").is_ok());

        // Invalid env names
        assert!(validator.validate_env_name("").is_err());
        assert!(validator.validate_env_name("123VAR").is_err());
        assert!(validator.validate_env_name("VAR-NAME").is_err());
        assert!(validator.validate_env_name("VAR; rm").is_err());
    }

    #[test]
    fn test_flags_validation() {
        let validator = BecomeValidator::new();

        // Valid flags
        assert!(validator.validate_flags("-H").is_ok());
        assert!(validator.validate_flags("-S -n").is_ok());
        assert!(validator.validate_flags("--preserve-env").is_ok());

        // Invalid flags (injection attempts)
        assert!(validator.validate_flags("-H; rm -rf /").is_err());
        assert!(validator.validate_flags("-H$(whoami)").is_err());
        assert!(validator.validate_flags("-H`id`").is_err());
    }
}
