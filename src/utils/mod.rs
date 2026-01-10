//! Shared utility functions for Rustible.

pub mod regex_cache;
pub use regex_cache::get_regex;

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
    let mut safe = true;
    for c in s.chars() {
        if !(c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '+')) {
            safe = false;
            break;
        }
    }

    if safe {
        if s.is_empty() {
             return "''".to_string();
        }
        return s.to_string();
    }

    let mut escaped = String::with_capacity(s.len() + 16);
    escaped.push('\'');

    for c in s.chars() {
        if c == '\'' {
            escaped.push_str("'\\''");
        } else {
            escaped.push(c);
        }
    }

    escaped.push('\'');
    escaped
}

/// Escape a string for safe use in Windows cmd.exe.
///
/// Windows cmd.exe has very specific and complex escaping rules.
/// This function wraps the string in double quotes and escapes any internal double quotes
/// using the CSV-style `""` escaping, which is generally accepted by `cmd /c "command"`.
///
/// # Arguments
///
/// * `s` - The string to escape
///
/// # Returns
///
/// * The escaped string safe for cmd.exe execution
pub fn cmd_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 16);
    escaped.push('"');
    for c in s.chars() {
        if c == '"' {
            escaped.push_str("\"\"");
        } else {
            escaped.push(c);
        }
    }
    escaped.push('"');
    escaped
}

/// Escapes a string for safe use in PowerShell commands.
///
/// This function handles special characters that could cause issues
/// in PowerShell string literals.
pub fn powershell_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 16);
    escaped.push('\'');
    for c in s.chars() {
        if c == '\'' {
            escaped.push_str("''");
        } else {
            escaped.push(c);
        }
    }
    escaped.push('\'');
    escaped
}

/// Escapes a string for use in PowerShell double-quoted strings.
///
/// This handles backticks, dollar signs, and double quotes.
pub fn powershell_escape_double_quoted(s: &str) -> String {
    // let escaped = s.replace('`', "``").replace('$', "`$").replace('"', "`\"");
    // format!("\"{}\"", escaped)
    let mut escaped = String::with_capacity(s.len() + 16);
    escaped.push('"');
    for c in s.chars() {
        match c {
            '`' => escaped.push_str("``"),
            '$' => escaped.push_str("`$"),
            '"' => escaped.push_str("`\""),
            _ => escaped.push(c),
        }
    }
    escaped.push('"');
    escaped
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

        // Empty string
        assert_eq!(shell_escape(""), "''");

        // Command injection attempts
        assert_eq!(shell_escape("pkg; rm -rf /"), "'pkg; rm -rf /'");
        assert_eq!(shell_escape("$(whoami)"), "'$(whoami)'");
        assert_eq!(shell_escape("`id`"), "'`id`'");
        assert_eq!(shell_escape("cat /etc/passwd"), "'cat /etc/passwd'");
        assert_eq!(shell_escape("foo|bar"), "'foo|bar'");
        assert_eq!(shell_escape("foo>bar"), "'foo>bar'");
        assert_eq!(shell_escape("foo&bar"), "'foo&bar'");
    }

    #[test]
    fn test_cmd_escape() {
        assert_eq!(cmd_escape("simple"), "\"simple\"");
        assert_eq!(cmd_escape("with space"), "\"with space\"");
        assert_eq!(cmd_escape("with\"quote"), "\"with\"\"quote\"");
        assert_eq!(cmd_escape("foo&bar"), "\"foo&bar\"");
        assert_eq!(cmd_escape("foo|bar"), "\"foo|bar\"");
        assert_eq!(cmd_escape(""), "\"\"");
    }

    #[test]
    fn test_powershell_escape() {
        assert_eq!(powershell_escape("simple"), "'simple'");
        assert_eq!(powershell_escape("with'quote"), "'with''quote'");
        assert_eq!(powershell_escape(""), "''");
    }

    #[test]
    fn test_powershell_escape_double_quoted() {
        assert_eq!(powershell_escape_double_quoted("simple"), "\"simple\"");
        assert_eq!(powershell_escape_double_quoted("with$var"), "\"with`$var\"");
        assert_eq!(
            powershell_escape_double_quoted("with`backtick"),
            "\"with``backtick\""
        );
        assert_eq!(powershell_escape_double_quoted(""), "\"\"");
    }
}
