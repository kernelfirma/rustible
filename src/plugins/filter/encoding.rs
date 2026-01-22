//! Encoding filters for Jinja2 templates.
//!
//! This module provides filters for encoding and decoding data in various
//! formats, compatible with Ansible's Jinja2 encoding filters.
//!
//! # Available Filters
//!
//! - `b64encode`: Encode data to Base64
//! - `b64decode`: Decode Base64 data
//! - `urlsplit`: Parse a URL into components
//! - `urlencode`: URL-encode a string
//! - `urldecode`: URL-decode a string
//! - `quote`: Shell-quote a string
//! - `unquote`: Remove shell quotes from a string
//!
//! # Examples
//!
//! ```jinja2
//! {{ 'hello world' | b64encode }}
//! {{ 'aGVsbG8gd29ybGQ=' | b64decode }}
//! {{ 'hello world' | urlencode }}
//! {{ 'https://example.com/path?q=1' | urlsplit('query') }}
//! ```

use base64::Engine;
use minijinja::{Environment, Value};

/// Register all encoding filters with the given environment.
pub fn register_filters(env: &mut Environment<'static>) {
    env.add_filter("b64encode", b64encode);
    env.add_filter("b64decode", b64decode);
    env.add_filter("urlsplit", urlsplit);
    env.add_filter("urlencode", urlencode_filter);
    env.add_filter("urldecode", urldecode_filter);
    env.add_filter("quote", quote_filter);
    env.add_filter("unquote", unquote_filter);
}

/// Encode a string to Base64.
///
/// # Arguments
///
/// * `input` - The string to encode
///
/// # Returns
///
/// The Base64-encoded string.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `b64encode` filter.
fn b64encode(input: String) -> String {
    base64::engine::general_purpose::STANDARD.encode(input.as_bytes())
}

/// Decode a Base64 string.
///
/// # Arguments
///
/// * `input` - The Base64-encoded string to decode
///
/// # Returns
///
/// The decoded string, or an empty string if decoding fails.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `b64decode` filter.
fn b64decode(input: String) -> String {
    base64::engine::general_purpose::STANDARD
        .decode(&input)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}

/// Parse a URL and optionally extract a component.
///
/// # Arguments
///
/// * `url` - The URL to parse
/// * `component` - Optional: specific component to extract
///
/// # Components
///
/// - `scheme`: The URL scheme (e.g., "https")
/// - `netloc`: The network location (e.g., "example.com:8080")
/// - `hostname`: The hostname (e.g., "example.com")
/// - `port`: The port number
/// - `path`: The URL path
/// - `query`: The query string
/// - `fragment`: The URL fragment
/// - `username`: The username (if present)
/// - `password`: The password (if present)
///
/// # Returns
///
/// If component is specified, returns that component as a string.
/// Otherwise, returns an object with all components.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `urlsplit` filter.
fn urlsplit(url_str: String, component: Option<String>) -> Value {
    match url::Url::parse(&url_str) {
        Ok(url) => {
            if let Some(comp) = component {
                match comp.as_str() {
                    "scheme" => Value::from(url.scheme().to_string()),
                    "netloc" => {
                        let netloc = match url.port() {
                            Some(port) => format!("{}:{}", url.host_str().unwrap_or(""), port),
                            None => url.host_str().unwrap_or("").to_string(),
                        };
                        Value::from(netloc)
                    }
                    "hostname" => Value::from(url.host_str().unwrap_or("").to_string()),
                    "port" => url
                        .port()
                        .map(|p| Value::from(p as i64))
                        .unwrap_or(Value::from("")),
                    "path" => Value::from(url.path().to_string()),
                    "query" => Value::from(url.query().unwrap_or("").to_string()),
                    "fragment" => Value::from(url.fragment().unwrap_or("").to_string()),
                    "username" => Value::from(url.username().to_string()),
                    "password" => Value::from(url.password().unwrap_or("").to_string()),
                    _ => Value::UNDEFINED,
                }
            } else {
                // Return all components as an object
                Value::from_iter([
                    ("scheme".to_string(), Value::from(url.scheme().to_string())),
                    (
                        "netloc".to_string(),
                        Value::from(match url.port() {
                            Some(port) => format!("{}:{}", url.host_str().unwrap_or(""), port),
                            None => url.host_str().unwrap_or("").to_string(),
                        }),
                    ),
                    (
                        "hostname".to_string(),
                        Value::from(url.host_str().unwrap_or("").to_string()),
                    ),
                    (
                        "port".to_string(),
                        url.port()
                            .map(|p| Value::from(p as i64))
                            .unwrap_or(Value::from("")),
                    ),
                    ("path".to_string(), Value::from(url.path().to_string())),
                    (
                        "query".to_string(),
                        Value::from(url.query().unwrap_or("").to_string()),
                    ),
                    (
                        "fragment".to_string(),
                        Value::from(url.fragment().unwrap_or("").to_string()),
                    ),
                    (
                        "username".to_string(),
                        Value::from(url.username().to_string()),
                    ),
                    (
                        "password".to_string(),
                        Value::from(url.password().unwrap_or("").to_string()),
                    ),
                ])
            }
        }
        Err(_) => Value::UNDEFINED,
    }
}

/// URL-encode a string.
///
/// # Arguments
///
/// * `input` - The string to encode
///
/// # Returns
///
/// The URL-encoded string.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `urlencode` filter.
fn urlencode_filter(input: String) -> String {
    // URL encoding for query strings
    form_urlencoded::byte_serialize(input.as_bytes())
        .collect::<Vec<_>>()
        .join("")
}

/// URL-decode a string.
///
/// # Arguments
///
/// * `input` - The URL-encoded string to decode
///
/// # Returns
///
/// The decoded string.
fn urldecode_filter(input: String) -> String {
    form_urlencoded::parse(input.as_bytes())
        .map(|(k, v)| {
            if v.is_empty() {
                k.to_string()
            } else {
                format!("{}={}", k, v)
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Shell-quote a string.
///
/// # Arguments
///
/// * `input` - The string to quote
///
/// # Returns
///
/// A shell-safe quoted string.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `quote` filter.
fn quote_filter(input: String) -> String {
    // Use single quotes and escape any single quotes within
    format!("'{}'", input.replace('\'', "'\"'\"'"))
}

/// Remove shell quotes from a string.
///
/// # Arguments
///
/// * `input` - The quoted string
///
/// # Returns
///
/// The unquoted string.
fn unquote_filter(input: String) -> String {
    let trimmed = input.trim();

    // Handle single-quoted strings
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        return inner.replace("'\"'\"'", "'");
    }

    // Handle double-quoted strings
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        return inner
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
            .replace("\\$", "$")
            .replace("\\`", "`");
    }

    input
}

// Re-export url-encoding utilities
mod form_urlencoded {
    pub fn byte_serialize(bytes: &[u8]) -> impl Iterator<Item = String> + '_ {
        bytes.iter().map(|&b| {
            if matches!(
                b,
                b'A'..=b'Z'
                    | b'a'..=b'z'
                    | b'0'..=b'9'
                    | b'-'
                    | b'_'
                    | b'.'
                    | b'~'
            ) {
                (b as char).to_string()
            } else {
                format!("%{:02X}", b)
            }
        })
    }

    pub fn parse(
        bytes: &[u8],
    ) -> impl Iterator<Item = (std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)> {
        let s = String::from_utf8_lossy(bytes);
        let decoded = percent_decode(&s);
        vec![(
            std::borrow::Cow::Owned(decoded),
            std::borrow::Cow::Borrowed(""),
        )]
        .into_iter()
    }

    fn percent_decode(s: &str) -> String {
        let mut result = Vec::new();
        let mut chars = s.bytes().peekable();

        while let Some(b) = chars.next() {
            if b == b'%' {
                let h1 = chars.next().and_then(|c| char::from(c).to_digit(16));
                let h2 = chars.next().and_then(|c| char::from(c).to_digit(16));
                if let (Some(d1), Some(d2)) = (h1, h2) {
                    result.push((d1 * 16 + d2) as u8);
                } else {
                    result.push(b);
                }
            } else if b == b'+' {
                result.push(b' ');
            } else {
                result.push(b);
            }
        }

        String::from_utf8_lossy(&result).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_b64encode() {
        let result = b64encode("hello world".to_string());
        assert_eq!(result, "aGVsbG8gd29ybGQ=");
    }

    #[test]
    fn test_b64decode() {
        let result = b64decode("aGVsbG8gd29ybGQ=".to_string());
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_b64decode_invalid() {
        let result = b64decode("not valid base64!!!".to_string());
        assert!(result.is_empty());
    }

    #[test]
    fn test_b64_roundtrip() {
        let original = "The quick brown fox jumps over the lazy dog!";
        let encoded = b64encode(original.to_string());
        let decoded = b64decode(encoded);
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_urlsplit_all() {
        let result = urlsplit(
            "https://user:pass@example.com:8080/path?q=1#frag".to_string(),
            None,
        );
        assert!(!result.is_undefined());
    }

    #[test]
    fn test_urlsplit_scheme() {
        let result = urlsplit(
            "https://example.com/path".to_string(),
            Some("scheme".to_string()),
        );
        assert_eq!(result.to_string(), "https");
    }

    #[test]
    fn test_urlsplit_hostname() {
        let result = urlsplit(
            "https://example.com:8080/path".to_string(),
            Some("hostname".to_string()),
        );
        assert_eq!(result.to_string(), "example.com");
    }

    #[test]
    fn test_urlsplit_port() {
        let result = urlsplit(
            "https://example.com:8080/path".to_string(),
            Some("port".to_string()),
        );
        assert_eq!(result.to_string(), "8080");
    }

    #[test]
    fn test_urlsplit_path() {
        let result = urlsplit(
            "https://example.com/some/path".to_string(),
            Some("path".to_string()),
        );
        assert_eq!(result.to_string(), "/some/path");
    }

    #[test]
    fn test_urlsplit_query() {
        let result = urlsplit(
            "https://example.com/path?foo=bar&baz=qux".to_string(),
            Some("query".to_string()),
        );
        assert_eq!(result.to_string(), "foo=bar&baz=qux");
    }

    #[test]
    fn test_urlencode() {
        let result = urlencode_filter("hello world".to_string());
        assert_eq!(result, "hello%20world");
    }

    #[test]
    fn test_urlencode_special_chars() {
        let result = urlencode_filter("foo=bar&baz=qux".to_string());
        assert!(result.contains("%3D")); // = is encoded
        assert!(result.contains("%26")); // & is encoded
    }

    #[test]
    fn test_quote() {
        let result = quote_filter("hello world".to_string());
        assert_eq!(result, "'hello world'");
    }

    #[test]
    fn test_quote_with_single_quote() {
        let result = quote_filter("it's a test".to_string());
        assert_eq!(result, "'it'\"'\"'s a test'");
    }

    #[test]
    fn test_unquote_single() {
        let result = unquote_filter("'hello world'".to_string());
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_unquote_double() {
        let result = unquote_filter("\"hello world\"".to_string());
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_unquote_no_quotes() {
        let result = unquote_filter("hello world".to_string());
        assert_eq!(result, "hello world");
    }
}
