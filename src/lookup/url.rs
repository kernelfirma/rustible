//! URL Lookup Plugin
//!
//! Fetches content from HTTP/HTTPS URLs. Similar to Ansible's `url` lookup plugin.
//!
//! # Usage
//!
//! ```yaml
//! # Fetch a URL
//! content: "{{ lookup('url', 'https://example.com/api/data') }}"
//!
//! # With headers
//! content: "{{ lookup('url', 'https://api.example.com/endpoint', 'headers=Authorization:Bearer token') }}"
//!
//! # With validation options
//! content: "{{ lookup('url', 'https://example.com', 'validate_certs=true') }}"
//! ```
//!
//! # Options
//!
//! - `headers` (string): HTTP headers in format "Key:Value" (comma-separated for multiple)
//! - `validate_certs` (bool): Whether to validate SSL certificates (default: true)
//! - `timeout` (int): Request timeout in seconds (default: 30)
//! - `username` (string): Username for basic auth
//! - `password` (string): Password for basic auth
//! - `split_lines` (bool): Return each line as a separate result (default: false)

use super::{Lookup, LookupContext, LookupError, LookupResult};
use std::time::Duration;

/// URL lookup plugin for fetching HTTP content
#[derive(Debug, Clone, Default)]
pub struct UrlLookup;

impl UrlLookup {
    /// Create a new UrlLookup instance
    pub fn new() -> Self {
        Self
    }

    /// Validate a URL
    fn validate_url(&self, url: &str) -> LookupResult<()> {
        if url.is_empty() {
            return Err(LookupError::InvalidArguments("URL cannot be empty".to_string()));
        }

        // Check for null bytes
        if url.contains('\0') {
            return Err(LookupError::InvalidArguments(
                "URL contains null byte".to_string(),
            ));
        }

        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(LookupError::InvalidArguments(
                "URL must start with http:// or https://".to_string(),
            ));
        }

        // Parse the URL to validate format
        url::Url::parse(url).map_err(|e| {
            LookupError::InvalidArguments(format!("Invalid URL format: {}", e))
        })?;

        Ok(())
    }

    /// Parse headers from a comma-separated string of "Key:Value" pairs
    fn parse_headers(&self, headers_str: &str) -> LookupResult<Vec<(String, String)>> {
        let mut headers = Vec::new();

        for header in headers_str.split(',') {
            let header = header.trim();
            if header.is_empty() {
                continue;
            }

            let (key, value) = header.split_once(':').ok_or_else(|| {
                LookupError::InvalidArguments(format!(
                    "Invalid header format '{}', expected 'Key:Value'",
                    header
                ))
            })?;

            headers.push((key.trim().to_string(), value.trim().to_string()));
        }

        Ok(headers)
    }

    /// Fetch content from a URL (blocking, for non-async context)
    fn fetch_url(
        &self,
        url: &str,
        headers: Vec<(String, String)>,
        validate_certs: bool,
        timeout: Duration,
        auth: Option<(String, String)>,
    ) -> LookupResult<String> {
        // Build the client
        let client = reqwest::blocking::Client::builder()
            .danger_accept_invalid_certs(!validate_certs)
            .timeout(timeout)
            .build()
            .map_err(|e| LookupError::Http(format!("Failed to create HTTP client: {}", e)))?;

        // Build the request
        let mut request = client.get(url);

        // Add headers
        for (key, value) in headers {
            request = request.header(&key, &value);
        }

        // Add basic auth if provided
        if let Some((username, password)) = auth {
            request = request.basic_auth(username, Some(password));
        }

        // Execute the request
        let response = request.send().map_err(|e| {
            if e.is_timeout() {
                LookupError::Timeout(timeout.as_secs())
            } else if e.is_connect() {
                LookupError::Http(format!("Connection failed: {}", e))
            } else {
                LookupError::Http(format!("HTTP request failed: {}", e))
            }
        })?;

        // Check status code
        let status = response.status();
        if !status.is_success() {
            return Err(LookupError::Http(format!(
                "HTTP {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        // Get the response body
        let body = response.text().map_err(|e| {
            LookupError::Http(format!("Failed to read response body: {}", e))
        })?;

        Ok(body)
    }
}

impl Lookup for UrlLookup {
    fn name(&self) -> &'static str {
        "url"
    }

    fn description(&self) -> &'static str {
        "Fetches content from HTTP/HTTPS URLs"
    }

    fn lookup(&self, args: &[&str], context: &LookupContext) -> LookupResult<Vec<String>> {
        // Find the URL (first non-option argument)
        let url = args
            .iter()
            .find(|arg| !arg.contains('=') && (arg.starts_with("http://") || arg.starts_with("https://")))
            .ok_or_else(|| {
                LookupError::MissingArgument("URL required (must start with http:// or https://)".to_string())
            })?;

        // Validate the URL
        self.validate_url(url)?;

        // Parse options
        let options = self.parse_options(args);

        // Parse headers
        let headers = if let Some(headers_str) = options.get("headers") {
            self.parse_headers(headers_str)?
        } else {
            Vec::new()
        };

        // Parse validate_certs option
        let validate_certs = options
            .get("validate_certs")
            .map(|v| {
                v.eq_ignore_ascii_case("true")
                    || v == "1"
                    || v.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(true);

        // Parse timeout option
        let timeout_secs: u64 = options
            .get("timeout")
            .map(|s| {
                s.parse().map_err(|_| {
                    LookupError::InvalidArguments(format!("Invalid timeout value: {}", s))
                })
            })
            .transpose()?
            .unwrap_or(context.timeout_secs);
        let timeout = Duration::from_secs(timeout_secs);

        // Parse auth options
        let auth = match (options.get("username"), options.get("password")) {
            (Some(u), Some(p)) => Some((u.clone(), p.clone())),
            (Some(u), None) => Some((u.clone(), String::new())),
            _ => None,
        };

        // Parse split_lines option
        let split_lines = options
            .get("split_lines")
            .map(|v| {
                v.eq_ignore_ascii_case("true")
                    || v == "1"
                    || v.eq_ignore_ascii_case("yes")
            })
            .unwrap_or(false);

        // Fetch the URL
        let response_body = self.fetch_url(url, headers, validate_certs, timeout, auth)?;

        // Return results
        if split_lines {
            Ok(response_body.lines().map(|s| s.to_string()).collect())
        } else {
            Ok(vec![response_body])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_lookup_validate_url() {
        let lookup = UrlLookup::new();

        // Valid URLs
        assert!(lookup.validate_url("https://example.com").is_ok());
        assert!(lookup.validate_url("http://example.com/path").is_ok());
        assert!(lookup.validate_url("https://api.example.com/v1/data?key=value").is_ok());

        // Invalid URLs
        assert!(lookup.validate_url("").is_err());
        assert!(lookup.validate_url("ftp://example.com").is_err());
        assert!(lookup.validate_url("example.com").is_err());
        assert!(lookup.validate_url("/path/to/file").is_err());
    }

    #[test]
    fn test_url_lookup_parse_headers() {
        let lookup = UrlLookup::new();

        // Single header
        let headers = lookup.parse_headers("Content-Type:application/json").unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0], ("Content-Type".to_string(), "application/json".to_string()));

        // Multiple headers
        let headers = lookup.parse_headers("Content-Type:application/json,Authorization:Bearer token").unwrap();
        assert_eq!(headers.len(), 2);

        // Empty string
        let headers = lookup.parse_headers("").unwrap();
        assert!(headers.is_empty());

        // Invalid format
        let result = lookup.parse_headers("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_url_lookup_missing_url() {
        let lookup = UrlLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));

        // Only options, no URL
        let result = lookup.lookup(&["timeout=30"], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    #[test]
    fn test_url_lookup_invalid_url() {
        let lookup = UrlLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["not-a-url"], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));

        let result = lookup.lookup(&["ftp://example.com"], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    // Note: The following tests require network access and are marked as ignored
    // They can be run manually with `cargo test -- --ignored`

    #[test]
    #[ignore = "requires network access"]
    fn test_url_lookup_fetch_http() {
        let lookup = UrlLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["https://httpbin.org/get"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert!(values[0].contains("httpbin"));
    }

    #[test]
    #[ignore = "requires network access"]
    fn test_url_lookup_with_headers() {
        let lookup = UrlLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(
            &[
                "https://httpbin.org/headers",
                "headers=X-Custom-Header:TestValue",
            ],
            &context,
        );
        assert!(result.is_ok());
        let values = result.unwrap();
        assert!(values[0].contains("X-Custom-Header"));
    }

    #[test]
    #[ignore = "requires network access"]
    fn test_url_lookup_with_auth() {
        let lookup = UrlLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(
            &[
                "https://httpbin.org/basic-auth/user/passwd",
                "username=user",
                "password=passwd",
            ],
            &context,
        );
        assert!(result.is_ok());
        let values = result.unwrap();
        assert!(values[0].contains("authenticated"));
    }

    #[test]
    #[ignore = "requires network access"]
    fn test_url_lookup_404() {
        let lookup = UrlLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["https://httpbin.org/status/404"], &context);
        assert!(matches!(result, Err(LookupError::Http(_))));
    }

    #[test]
    #[ignore = "requires network access"]
    fn test_url_lookup_timeout() {
        let lookup = UrlLookup::new();
        let context = LookupContext::new().with_timeout(1);

        // This endpoint delays for 10 seconds, but we set timeout to 1 second
        let result = lookup.lookup(&["https://httpbin.org/delay/10"], &context);
        assert!(matches!(result, Err(LookupError::Timeout(_))));
    }

    #[test]
    #[ignore = "requires network access"]
    fn test_url_lookup_split_lines() {
        let lookup = UrlLookup::new();
        let context = LookupContext::default();

        // This returns multiple lines of data
        let result = lookup.lookup(
            &["https://httpbin.org/robots.txt", "split_lines=true"],
            &context,
        );
        assert!(result.is_ok());
        let values = result.unwrap();
        assert!(values.len() >= 1);
    }
}
