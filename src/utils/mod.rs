//! Shared utility functions for Rustible.

/// Escape a string for safe use in shell commands.
///
/// This function is essential for preventing command injection vulnerabilities.
/// It wraps the string in single quotes and escapes any single quotes within it.
/// Alphanumeric characters and a few safe symbols are returned as-is to improve readability.
///
/// # Arguments
///
/// * `s` - The string to escape
///
/// # Returns
///
/// * The escaped string safe for shell execution
///
/// # Examples
///
/// ```
/// use rustible::utils::shell_escape;
///
/// assert_eq!(shell_escape("simple"), "simple");
/// assert_eq!(shell_escape("with space"), "'with space'");
/// assert_eq!(shell_escape("don't"), "'don'\\''t'");
/// assert_eq!(shell_escape("rm -rf /"), "'rm -rf /'");
/// ```
pub fn shell_escape(s: &str) -> String {
    // If the string contains no special characters, return as-is
    // This makes the output more readable for simple cases
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')
    {
        return s.to_string();
    }
    // Otherwise, wrap in single quotes and escape any single quotes within
    // 'string' -> 'string'
    // 'can't' -> 'can'\''t'
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("nginx"), "nginx");
        assert_eq!(shell_escape("nginx-1.2.3"), "nginx-1.2.3");
        assert_eq!(shell_escape("/usr/bin/python"), "/usr/bin/python");

        assert_eq!(shell_escape("with space"), "'with space'");
        assert_eq!(shell_escape("with'quote"), "'with'\\''quote'");
        assert_eq!(shell_escape("don't"), "'don'\\''t'");

        // Command injection attempts
        assert_eq!(shell_escape("pkg; rm -rf /"), "'pkg; rm -rf /'");
        assert_eq!(shell_escape("$(whoami)"), "'$(whoami)'");
        assert_eq!(shell_escape("`id`"), "'`id`'");
        assert_eq!(shell_escape("cat /etc/passwd"), "'cat /etc/passwd'");
        assert_eq!(shell_escape("foo|bar"), "'foo|bar'");
        assert_eq!(shell_escape("foo>bar"), "'foo>bar'");
        assert_eq!(shell_escape("foo&bar"), "'foo&bar'");
    }
}
