//! Template security utilities
//!
//! Provides functions to:
//! - Sanitize template variables
//! - Implement content security policies
//! - Prevent template injection attacks

use super::{SecurityError, SecurityResult};
use std::collections::HashSet;

/// Content Security Policy for templates
///
/// Controls what operations and content are allowed in templates.
#[derive(Debug, Clone)]
pub struct TemplateSecurityPolicy {
    /// Allow execution of arbitrary code/commands
    pub allow_exec: bool,
    /// Allow file system access
    pub allow_fs_access: bool,
    /// Allow network access
    pub allow_network: bool,
    /// Allow environment variable access
    pub allow_env: bool,
    /// Maximum template recursion depth
    pub max_recursion: u32,
    /// Maximum output size in bytes
    pub max_output_size: usize,
    /// Blocked variable patterns
    pub blocked_patterns: HashSet<String>,
    /// Allowed functions (if empty, all safe functions allowed)
    pub allowed_functions: HashSet<String>,
}

impl Default for TemplateSecurityPolicy {
    fn default() -> Self {
        Self::standard()
    }
}

impl TemplateSecurityPolicy {
    /// Create a standard security policy
    pub fn standard() -> Self {
        let mut blocked = HashSet::new();
        // Block patterns that could leak sensitive info
        blocked.insert("password".to_string());
        blocked.insert("secret".to_string());
        blocked.insert("token".to_string());
        blocked.insert("key".to_string());
        blocked.insert("credential".to_string());
        blocked.insert("private".to_string());

        Self {
            allow_exec: false,
            allow_fs_access: false,
            allow_network: false,
            allow_env: true, // Allow env access by default for Ansible compat
            max_recursion: 10,
            max_output_size: 10 * 1024 * 1024, // 10MB
            blocked_patterns: blocked,
            allowed_functions: HashSet::new(), // Empty = all safe functions allowed
        }
    }

    /// Create a restrictive policy for untrusted templates
    pub fn restrictive() -> Self {
        Self {
            allow_exec: false,
            allow_fs_access: false,
            allow_network: false,
            allow_env: false,
            max_recursion: 3,
            max_output_size: 1024 * 1024, // 1MB
            blocked_patterns: HashSet::new(),
            allowed_functions: HashSet::new(),
        }
    }

    /// Create a permissive policy for trusted templates
    pub fn permissive() -> Self {
        Self {
            allow_exec: false, // Still don't allow exec
            allow_fs_access: true,
            allow_network: false,
            allow_env: true,
            max_recursion: 50,
            max_output_size: 100 * 1024 * 1024, // 100MB
            blocked_patterns: HashSet::new(),
            allowed_functions: HashSet::new(),
        }
    }

    /// Check if a variable name is blocked by the policy
    pub fn is_variable_blocked(&self, name: &str) -> bool {
        let lower = name.to_lowercase();
        self.blocked_patterns
            .iter()
            .any(|pattern| lower.contains(pattern))
    }

    /// Check if a function is allowed by the policy
    pub fn is_function_allowed(&self, name: &str) -> bool {
        if self.allowed_functions.is_empty() {
            // Check against builtin dangerous functions
            !is_dangerous_function(name)
        } else {
            self.allowed_functions.contains(name)
        }
    }
}

/// Check if a function name is considered dangerous
fn is_dangerous_function(name: &str) -> bool {
    const DANGEROUS_FUNCTIONS: &[&str] = &[
        "eval",
        "exec",
        "system",
        "shell",
        "popen",
        "subprocess",
        "import",
        "require",
        "include",
        "read_file",
        "write_file",
        "delete",
        "remove",
        "unlink",
        "rmdir",
        "chown",
        "chmod",
        "setuid",
        "setgid",
    ];

    DANGEROUS_FUNCTIONS.contains(&name.to_lowercase().as_str())
}

/// Template variable sanitizer
#[derive(Debug, Clone)]
pub struct TemplateSanitizer {
    /// Security policy to apply
    policy: TemplateSecurityPolicy,
}

impl Default for TemplateSanitizer {
    fn default() -> Self {
        Self::new(TemplateSecurityPolicy::standard())
    }
}

impl TemplateSanitizer {
    /// Create a new sanitizer with the given policy
    pub fn new(policy: TemplateSecurityPolicy) -> Self {
        Self { policy }
    }

    /// Sanitize a template variable value
    ///
    /// # Arguments
    ///
    /// * `name` - Variable name
    /// * `value` - Variable value to sanitize
    ///
    /// # Returns
    ///
    /// * `Ok(String)` - Sanitized value
    /// * `Err(SecurityError)` - If the variable is blocked or dangerous
    pub fn sanitize_variable(&self, name: &str, value: &str) -> SecurityResult<String> {
        // Check if variable name is blocked
        if self.policy.is_variable_blocked(name) {
            return Err(SecurityError::TemplateViolation(format!(
                "Variable '{}' matches blocked pattern",
                name
            )));
        }

        // Check output size
        if value.len() > self.policy.max_output_size {
            return Err(SecurityError::TemplateViolation(format!(
                "Variable '{}' value exceeds maximum size",
                name
            )));
        }

        // Sanitize the value
        let sanitized = self.sanitize_value(value);

        Ok(sanitized)
    }

    /// Sanitize a template variable value (internal)
    fn sanitize_value(&self, value: &str) -> String {
        // Remove null bytes
        let value = value.replace('\0', "");

        // Escape HTML-like content if it looks dangerous
        if value.contains('<') && value.contains('>') {
            return html_escape(&value);
        }

        value
    }

    /// Check if a template string contains dangerous patterns
    pub fn check_template(&self, template: &str) -> SecurityResult<()> {
        // Check for maximum length
        if template.len() > self.policy.max_output_size {
            return Err(SecurityError::TemplateViolation(
                "Template exceeds maximum size".to_string(),
            ));
        }

        // Check for dangerous patterns
        let dangerous_patterns = [
            // Jinja2 code execution attempts
            "{{ config",
            "{{ self",
            "{{ request",
            "{{ ''.__class__",
            "{{ \"\".__class__",
            "{% raw %}",
            // File access attempts
            "{% include '/",
            "{% include \"c:",
            "{% include \"C:",
            // Command execution
            "| shell",
            "| bash",
            "| sh",
            "| cmd",
            "| powershell",
        ];

        for pattern in dangerous_patterns {
            if template.contains(pattern) {
                return Err(SecurityError::TemplateViolation(format!(
                    "Template contains dangerous pattern: '{}'",
                    pattern
                )));
            }
        }

        // Check recursion depth (count nested template tags)
        let mut depth: usize = 0;
        let mut max_depth: usize = 0;
        for c in template.chars() {
            if c == '{' {
                depth += 1;
                max_depth = max_depth.max(depth);
            } else if c == '}' {
                depth = depth.saturating_sub(1);
            }
        }

        if max_depth > self.policy.max_recursion as usize {
            return Err(SecurityError::TemplateViolation(format!(
                "Template nesting depth {} exceeds maximum {}",
                max_depth, self.policy.max_recursion
            )));
        }

        Ok(())
    }

    /// Validate that template output doesn't contain sensitive data
    pub fn validate_output(&self, output: &str, sensitive_patterns: &[&str]) -> SecurityResult<()> {
        for pattern in sensitive_patterns {
            if output.contains(pattern) {
                return Err(SecurityError::TemplateViolation(format!(
                    "Template output contains sensitive data matching pattern: '{}'",
                    pattern
                )));
            }
        }
        Ok(())
    }
}

/// Escape HTML special characters
fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Check if a string could be a template injection attempt
pub fn is_potential_injection(input: &str) -> bool {
    // Check for Jinja2-style template syntax
    let has_template_syntax = input.contains("{{") || input.contains("{%") || input.contains("{#");

    // Check for nested template attempts
    let has_nested = input.contains("{{{{") || input.contains("}}}}");

    // Check for code execution attempts
    let code_patterns = [
        "__class__",
        "__mro__",
        "__subclasses__",
        "__globals__",
        "__builtins__",
        "config.items",
        "self._",
    ];

    let has_code_pattern = code_patterns.iter().any(|p| input.contains(p));

    has_template_syntax && (has_nested || has_code_pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_security_policy_default() {
        let policy = TemplateSecurityPolicy::standard();

        assert!(!policy.allow_exec);
        assert!(!policy.allow_fs_access);
        assert!(!policy.allow_network);
        assert!(policy.allow_env);
    }

    #[test]
    fn test_variable_blocking() {
        let policy = TemplateSecurityPolicy::standard();

        assert!(policy.is_variable_blocked("db_password"));
        assert!(policy.is_variable_blocked("api_secret"));
        assert!(policy.is_variable_blocked("auth_token"));
        assert!(policy.is_variable_blocked("private_key"));

        assert!(!policy.is_variable_blocked("hostname"));
        assert!(!policy.is_variable_blocked("port"));
        assert!(!policy.is_variable_blocked("username"));
    }

    #[test]
    fn test_dangerous_function_check() {
        assert!(is_dangerous_function("eval"));
        assert!(is_dangerous_function("exec"));
        assert!(is_dangerous_function("system"));
        assert!(is_dangerous_function("EVAL")); // Case insensitive

        assert!(!is_dangerous_function("upper"));
        assert!(!is_dangerous_function("lower"));
        assert!(!is_dangerous_function("join"));
    }

    #[test]
    fn test_sanitizer_variable() {
        let sanitizer = TemplateSanitizer::default();

        // Normal variables pass through
        assert!(sanitizer.sanitize_variable("name", "hello").is_ok());
        assert_eq!(
            sanitizer.sanitize_variable("name", "hello").unwrap(),
            "hello"
        );

        // Blocked variables fail
        assert!(sanitizer
            .sanitize_variable("db_password", "secret123")
            .is_err());
        assert!(sanitizer.sanitize_variable("api_token", "abc").is_err());
    }

    #[test]
    fn test_sanitizer_template_check() {
        let sanitizer = TemplateSanitizer::default();

        // Normal templates pass
        assert!(sanitizer.check_template("Hello {{ name }}!").is_ok());
        assert!(sanitizer
            .check_template("{% for item in items %}{{ item }}{% endfor %}")
            .is_ok());

        // Dangerous templates fail
        assert!(sanitizer.check_template("{{ config.items() }}").is_err());
        assert!(sanitizer.check_template("{{ ''.__class__ }}").is_err());
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
        assert_eq!(html_escape("\"test\""), "&quot;test&quot;");
    }

    #[test]
    fn test_is_potential_injection() {
        // Normal text is not injection
        assert!(!is_potential_injection("hello world"));
        assert!(!is_potential_injection("{{ name }}")); // Valid template

        // Injection attempts
        assert!(is_potential_injection("{{ ''.__class__ }}"));
        assert!(is_potential_injection("{{ config.items() }}"));
        assert!(is_potential_injection("{{{{ nested }}}}"));
    }
}
