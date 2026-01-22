//! Input validation and sanitization utilities
//!
//! Provides functions to validate and sanitize user input to prevent:
//! - Command injection
//! - Shell metacharacter attacks
//! - Null byte injection
//! - Control character attacks

use super::{SecurityError, SecurityResult};
use once_cell::sync::Lazy;
use regex::Regex;

/// Regex for valid identifiers (variable names, module names, etc.)
static IDENTIFIER_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").expect("Invalid identifier regex"));

/// Regex for valid hostnames (RFC 1123)
static HOSTNAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9\-]{0,61}[a-zA-Z0-9])?(\.[a-zA-Z0-9]([a-zA-Z0-9\-]{0,61}[a-zA-Z0-9])?)*$")
        .expect("Invalid hostname regex")
});

/// Regex for IP addresses (v4 and v6)
static IP_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})|(\[[a-fA-F0-9:]+\])$")
        .expect("Invalid IP regex")
});

/// Dangerous shell metacharacters that could enable command injection
const SHELL_METACHARACTERS: &[char] = &[
    ';', '&', '|', '$', '`', '(', ')', '{', '}', '[', ']', '<', '>', '\n', '\r', '\0', '!', '#',
];

/// Level of sanitization to apply
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SanitizationLevel {
    /// Only escape dangerous characters, preserve most input
    Minimal,
    /// Escape all shell metacharacters
    Standard,
    /// Allow only alphanumeric, underscore, hyphen, dot
    Strict,
}

/// Input validator with configurable security policies
#[derive(Debug, Clone)]
pub struct InputValidator {
    /// Maximum allowed length for inputs
    pub max_length: usize,
    /// Whether to allow unicode characters
    pub allow_unicode: bool,
    /// Sanitization level
    pub sanitization_level: SanitizationLevel,
}

impl Default for InputValidator {
    fn default() -> Self {
        Self {
            max_length: 4096,
            allow_unicode: true,
            sanitization_level: SanitizationLevel::Standard,
        }
    }
}

impl InputValidator {
    /// Create a new input validator with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a strict validator for high-security contexts
    pub fn strict() -> Self {
        Self {
            max_length: 1024,
            allow_unicode: false,
            sanitization_level: SanitizationLevel::Strict,
        }
    }

    /// Validate and sanitize a string input
    pub fn validate(&self, input: &str, field_name: &str) -> SecurityResult<String> {
        // Check length
        if input.len() > self.max_length {
            return Err(SecurityError::InvalidInput(format!(
                "{} exceeds maximum length of {} bytes",
                field_name, self.max_length
            )));
        }

        // Check for null bytes
        if input.contains('\0') {
            return Err(SecurityError::InvalidInput(format!(
                "{} contains null byte",
                field_name
            )));
        }

        // Check unicode if not allowed
        if !self.allow_unicode && !input.is_ascii() {
            return Err(SecurityError::InvalidInput(format!(
                "{} contains non-ASCII characters",
                field_name
            )));
        }

        // Apply sanitization
        match self.sanitization_level {
            SanitizationLevel::Minimal => Ok(self.sanitize_minimal(input)),
            SanitizationLevel::Standard => Ok(self.sanitize_standard(input)),
            SanitizationLevel::Strict => self.sanitize_strict(input, field_name),
        }
    }

    /// Minimal sanitization - only escape quotes for shell safety
    fn sanitize_minimal(&self, input: &str) -> String {
        input.replace('\'', "'\\''")
    }

    /// Standard sanitization - escape shell metacharacters
    fn sanitize_standard(&self, input: &str) -> String {
        sanitize_shell_arg(input)
    }

    /// Strict sanitization - only allow safe characters
    fn sanitize_strict(&self, input: &str, field_name: &str) -> SecurityResult<String> {
        // Check for dangerous characters
        for c in input.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '.' && c != '/' {
                return Err(SecurityError::InvalidInput(format!(
                    "{} contains invalid character '{}'",
                    field_name, c
                )));
            }
        }
        Ok(input.to_string())
    }
}

/// Sanitize a string for safe use as a shell argument.
///
/// This function wraps the input in single quotes when needed and escapes any
/// embedded single quotes, making it safe to pass to shell commands. Inputs
/// that only contain safe characters are returned as-is.
///
/// # Examples
///
/// ```
/// use rustible::security::sanitize_shell_arg;
///
/// assert_eq!(sanitize_shell_arg("hello"), "hello");
/// assert_eq!(sanitize_shell_arg("it's"), "'it'\\''s'");
/// assert_eq!(sanitize_shell_arg("$(whoami)"), "'$(whoami)'");
/// ```
pub fn sanitize_shell_arg(input: &str) -> String {
    // If string contains no special characters, return as-is for efficiency
    if input
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
    {
        return input.to_string();
    }

    // Wrap in single quotes and escape embedded single quotes
    format!("'{}'", input.replace('\'', "'\\''"))
}

/// Check if a string contains shell metacharacters that could be dangerous.
///
/// Returns `true` if the string contains any shell metacharacters.
pub fn contains_shell_metacharacters(input: &str) -> bool {
    input.chars().any(|c| SHELL_METACHARACTERS.contains(&c))
}

/// Validate that a string is a valid identifier (for variable names, etc.).
///
/// Valid identifiers start with a letter or underscore, followed by
/// alphanumeric characters or underscores.
///
/// # Examples
///
/// ```
/// use rustible::security::validate_identifier;
///
/// assert!(validate_identifier("my_var").is_ok());
/// assert!(validate_identifier("_private").is_ok());
/// assert!(validate_identifier("123abc").is_err());
/// assert!(validate_identifier("my-var").is_err());
/// ```
pub fn validate_identifier(name: &str) -> SecurityResult<()> {
    if name.is_empty() {
        return Err(SecurityError::InvalidInput(
            "Identifier cannot be empty".to_string(),
        ));
    }

    if name.len() > 255 {
        return Err(SecurityError::InvalidInput(
            "Identifier exceeds maximum length".to_string(),
        ));
    }

    if !IDENTIFIER_REGEX.is_match(name) {
        return Err(SecurityError::InvalidInput(format!(
            "Invalid identifier '{}': must start with letter/underscore and contain only alphanumeric/underscore",
            name
        )));
    }

    Ok(())
}

/// Validate that a string is a valid hostname or IP address.
///
/// # Examples
///
/// ```
/// use rustible::security::validate_hostname;
///
/// assert!(validate_hostname("example.com").is_ok());
/// assert!(validate_hostname("192.168.1.1").is_ok());
/// assert!(validate_hostname("host_name").is_err());
/// assert!(validate_hostname("host;rm -rf /").is_err());
/// ```
pub fn validate_hostname(host: &str) -> SecurityResult<()> {
    if host.is_empty() {
        return Err(SecurityError::InvalidInput(
            "Hostname cannot be empty".to_string(),
        ));
    }

    if host.len() > 253 {
        return Err(SecurityError::InvalidInput(
            "Hostname exceeds maximum length".to_string(),
        ));
    }

    // Check for null bytes and control characters
    if host.chars().any(|c| c.is_control()) {
        return Err(SecurityError::InvalidInput(
            "Hostname contains control characters".to_string(),
        ));
    }

    // Check for command injection attempts
    if contains_shell_metacharacters(host) {
        return Err(SecurityError::CommandInjection(format!(
            "Hostname '{}' contains shell metacharacters",
            host
        )));
    }

    // Validate against hostname or IP patterns
    if !HOSTNAME_REGEX.is_match(host) && !IP_REGEX.is_match(host) {
        return Err(SecurityError::InvalidInput(format!(
            "Invalid hostname or IP address: '{}'",
            host
        )));
    }

    Ok(())
}

/// Validate that a string is a valid URL.
///
/// # Examples
///
/// ```
/// use rustible::security::validate_url;
///
/// assert!(validate_url("https://example.com/path").is_ok());
/// assert!(validate_url("git://github.com/user/repo.git").is_ok());
/// assert!(validate_url("file:///etc/passwd").is_err()); // file:// not allowed
/// assert!(validate_url("javascript:alert(1)").is_err());
/// ```
pub fn validate_url(url: &str) -> SecurityResult<()> {
    if url.is_empty() {
        return Err(SecurityError::InvalidInput(
            "URL cannot be empty".to_string(),
        ));
    }

    if url.len() > 2048 {
        return Err(SecurityError::InvalidInput(
            "URL exceeds maximum length".to_string(),
        ));
    }

    // Check for null bytes
    if url.contains('\0') {
        return Err(SecurityError::InvalidInput(
            "URL contains null byte".to_string(),
        ));
    }

    // Disallow dangerous URL schemes
    let lower_url = url.to_lowercase();
    let dangerous_schemes = [
        "javascript:",
        "data:",
        "vbscript:",
        "file://",
        "about:",
        "blob:",
    ];

    for scheme in dangerous_schemes {
        if lower_url.starts_with(scheme) {
            return Err(SecurityError::InvalidInput(format!(
                "URL scheme '{}' is not allowed",
                scheme.trim_end_matches(':').trim_end_matches('/')
            )));
        }
    }

    // Basic URL structure validation
    let allowed_schemes = [
        "http://", "https://", "git://", "ssh://", "ftp://", "sftp://",
    ];
    let has_valid_scheme = allowed_schemes.iter().any(|s| lower_url.starts_with(s));

    if !has_valid_scheme {
        return Err(SecurityError::InvalidInput(format!(
            "URL '{}' has invalid or unsupported scheme",
            url
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_shell_arg() {
        // Simple strings pass through with quotes
        assert_eq!(sanitize_shell_arg("hello"), "hello");
        assert_eq!(sanitize_shell_arg("hello-world"), "hello-world");
        assert_eq!(sanitize_shell_arg("test_file.txt"), "test_file.txt");

        // Strings with spaces get quoted
        assert_eq!(sanitize_shell_arg("hello world"), "'hello world'");

        // Single quotes are escaped
        assert_eq!(sanitize_shell_arg("it's"), "'it'\\''s'");

        // Command injection attempts are neutralized
        assert_eq!(sanitize_shell_arg("$(whoami)"), "'$(whoami)'");
        assert_eq!(sanitize_shell_arg("`id`"), "'`id`'");
        assert_eq!(sanitize_shell_arg("; rm -rf /"), "'; rm -rf /'");
    }

    #[test]
    fn test_validate_identifier() {
        // Valid identifiers
        assert!(validate_identifier("my_var").is_ok());
        assert!(validate_identifier("_private").is_ok());
        assert!(validate_identifier("MyClass").is_ok());
        assert!(validate_identifier("var123").is_ok());

        // Invalid identifiers
        assert!(validate_identifier("").is_err());
        assert!(validate_identifier("123abc").is_err());
        assert!(validate_identifier("my-var").is_err());
        assert!(validate_identifier("my var").is_err());
        assert!(validate_identifier("my.var").is_err());
    }

    #[test]
    fn test_validate_hostname() {
        // Valid hostnames
        assert!(validate_hostname("example.com").is_ok());
        assert!(validate_hostname("sub.example.com").is_ok());
        assert!(validate_hostname("host-name").is_ok());
        assert!(validate_hostname("192.168.1.1").is_ok());
        assert!(validate_hostname("10.0.0.1").is_ok());

        // Invalid hostnames
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname("host;rm -rf /").is_err());
        assert!(validate_hostname("host$(whoami)").is_err());
        assert!(validate_hostname("host\nname").is_err());
    }

    #[test]
    fn test_validate_url() {
        // Valid URLs
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://example.com/path").is_ok());
        assert!(validate_url("git://github.com/user/repo.git").is_ok());
        assert!(validate_url("ssh://git@github.com/user/repo.git").is_ok());

        // Invalid URLs
        assert!(validate_url("").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("data:text/html,<script>").is_err());
    }

    #[test]
    fn test_contains_shell_metacharacters() {
        assert!(!contains_shell_metacharacters("hello"));
        assert!(!contains_shell_metacharacters("hello-world"));
        assert!(contains_shell_metacharacters("hello;world"));
        assert!(contains_shell_metacharacters("$(whoami)"));
        assert!(contains_shell_metacharacters("a|b"));
        assert!(contains_shell_metacharacters("a&b"));
    }

    #[test]
    fn test_input_validator() {
        let validator = InputValidator::new();

        // Valid inputs
        assert!(validator.validate("hello", "test").is_ok());
        assert!(validator.validate("hello world", "test").is_ok());

        // Null bytes rejected
        assert!(validator.validate("hello\0world", "test").is_err());

        // Strict validator
        let strict = InputValidator::strict();
        assert!(strict.validate("hello", "test").is_ok());
        assert!(strict.validate("hello-world", "test").is_ok());
        assert!(strict.validate("hello world", "test").is_err()); // spaces not allowed
    }
}
