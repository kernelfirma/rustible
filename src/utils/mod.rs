//! Shared utility functions for Rustible.

pub mod regex_cache;
pub use regex_cache::get_regex;

pub mod fs;
pub use fs::secure_write_file;

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::Hasher;
use std::io::{self, Read};
use std::path::Path;

/// Compute a checksum for a file using streaming to avoid loading it entirely into memory.
///
/// This function uses `DefaultHasher` which is not cryptographically secure and
/// may vary across Rust versions/runs, but is sufficient for internal change detection
/// within the same process execution.
pub fn get_file_checksum(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = DefaultHasher::new();
    let mut buffer = [0; 8192];

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.write(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finish()))
}

/// Compute a checksum for a byte slice.
///
/// Matches the behavior of `get_file_checksum` by writing bytes directly to the hasher.
pub fn compute_checksum(data: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(data);
    format!("{:x}", hasher.finish())
}

/// Determine if unsafe template helpers are allowed.
///
/// This gates filters/functions that can expose host details (e.g. realpath,
/// expanduser, env lookups). Set `RUSTIBLE_ALLOW_UNSAFE_TEMPLATES` to a truthy
/// value to enable them.
pub fn unsafe_template_access_allowed() -> bool {
    match std::env::var("RUSTIBLE_ALLOW_UNSAFE_TEMPLATES") {
        Ok(value) => matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

/// Parse a boolean string case-insensitively without allocation.
///
/// Handles common boolean representations:
/// - True: "true", "yes", "on", "1", "y", "t"
/// - False: "false", "no", "off", "0", "n", "f"
///
/// Returns `None` if the string is not a recognized boolean value.
pub fn parse_bool(s: &str) -> Option<bool> {
    match s {
        "1" | "t" | "T" | "y" | "Y" => Some(true),
        "0" | "f" | "F" | "n" | "N" => Some(false),
        _ => {
            if s.eq_ignore_ascii_case("true")
                || s.eq_ignore_ascii_case("yes")
                || s.eq_ignore_ascii_case("on")
            {
                Some(true)
            } else if s.eq_ignore_ascii_case("false")
                || s.eq_ignore_ascii_case("no")
                || s.eq_ignore_ascii_case("off")
            {
                Some(false)
            } else {
                None
            }
        }
    }
}

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
pub fn shell_escape(s: &str) -> Cow<'_, str> {
    let mut safe = true;
    // Iterate over bytes to avoid UTF-8 decoding overhead.
    // Safe characters are all ASCII, so any multi-byte char (>= 128) is unsafe.
    for b in s.bytes() {
        if !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b':' | b'+')) {
            safe = false;
            break;
        }
    }

    if safe {
        if s.is_empty() {
            return Cow::Borrowed("''");
        }
        return Cow::Borrowed(s);
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
    Cow::Owned(escaped)
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
pub fn cmd_escape(s: &str) -> Cow<'_, str> {
    let mut escaped = String::with_capacity(s.len() + 16);
    escaped.push('"');

    for c in s.chars() {
        if c == '"' {
            escaped.push_str("\"\"");
        } else {
            escaped.push(c);
        }
    }

    // Optimization: If no quotes were found and string is otherwise safe (though cmd rules are complex),
    // we might be able to return borrowed. But cmd escaping typically always quotes for safety unless trivial.
    // The previous implementation ALWAYS quoted and allocated.
    // Given cmd weirdness, always quoting is safer, but we still allocate.
    // However, the previous implementation did:
    // let mut escaped = String::with_capacity(s.len() + 16);
    // escaped.push('"');
    // ...
    // escaped.push('"');
    //
    // So it always allocated.
    // If we want to return Cow, we can check if it needs escaping.
    // But cmd_escape contract seems to be "wrap in quotes".
    // So we'll keep it allocating but return Cow::Owned to match signature.

    escaped.push('"');
    Cow::Owned(escaped)
}

/// Escapes a string for safe use in PowerShell commands.
///
/// This function handles special characters that could cause issues
/// in PowerShell string literals.
///
/// PowerShell escaping rules are tricky because of "expression mode".
/// Even simple strings like `1+1` or `-flag` can be interpreted as expressions or parameters
/// if not quoted. Therefore, we conservatively always wrap in single quotes to ensure
/// the value is treated as a string literal.
pub fn powershell_escape(s: &str) -> Cow<'_, str> {
    // Always escape for safety in PowerShell expression mode
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
    Cow::Owned(escaped)
}

/// Escapes a string for use in PowerShell double-quoted strings.
///
/// This handles backticks, dollar signs, and double quotes.
pub fn powershell_escape_double_quoted(s: &str) -> Cow<'_, str> {
    // This function wraps in double quotes.
    // Unlike the others, this seems to be for partial string interpolation or specific use cases.
    // Original always allocated.
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
    Cow::Owned(escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_bool() {
        // True values
        assert_eq!(parse_bool("true"), Some(true));
        assert_eq!(parse_bool("True"), Some(true));
        assert_eq!(parse_bool("TRUE"), Some(true));
        assert_eq!(parse_bool("yes"), Some(true));
        assert_eq!(parse_bool("Yes"), Some(true));
        assert_eq!(parse_bool("on"), Some(true));
        assert_eq!(parse_bool("On"), Some(true));
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("y"), Some(true));
        assert_eq!(parse_bool("t"), Some(true));

        // False values
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("False"), Some(false));
        assert_eq!(parse_bool("FALSE"), Some(false));
        assert_eq!(parse_bool("no"), Some(false));
        assert_eq!(parse_bool("No"), Some(false));
        assert_eq!(parse_bool("off"), Some(false));
        assert_eq!(parse_bool("Off"), Some(false));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("n"), Some(false));
        assert_eq!(parse_bool("f"), Some(false));

        // Invalid values
        assert_eq!(parse_bool(""), None);
        assert_eq!(parse_bool("foo"), None);
        assert_eq!(parse_bool("2"), None);
    }

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
    fn test_shell_escape_unicode() {
        // Unicode characters should be quoted for safety
        assert_eq!(shell_escape("café"), "'café'");
        assert_eq!(shell_escape("こんにちは"), "'こんにちは'");
        assert_eq!(shell_escape("emoji🔐"), "'emoji🔐'");
    }

    #[test]
    fn test_shell_escape_cow() {
        // Verify we are actually avoiding allocation
        match shell_escape("simple") {
            Cow::Borrowed(_) => {}
            Cow::Owned(_) => panic!("Should be borrowed"),
        }

        if let Cow::Borrowed(_) = shell_escape("with space") { panic!("Should be owned") }
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

    #[test]
    fn test_checksum_consistency() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let content = b"hello world";
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content).unwrap();

        let file_sum = get_file_checksum(file.path()).unwrap();
        let mem_sum = compute_checksum(content);

        assert_eq!(file_sum, mem_sum);
    }
}
