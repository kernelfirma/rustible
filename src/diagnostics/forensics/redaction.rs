//! Sensitive data redaction
//!
//! Provides pattern-based redaction of passwords, tokens, private keys, and
//! other secrets before forensics data is exported.

use serde::{Deserialize, Serialize};

/// The kind of content a redaction rule applies to.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContentType {
    /// Audit log entries.
    AuditLog,
    /// State snapshot files.
    StateFile,
    /// Drift report files.
    DriftReport,
    /// Matches any content type.
    All,
}

/// Pattern used to locate sensitive data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RedactionPattern {
    /// A regular expression pattern.
    Regex(String),
    /// A literal string match.
    Literal(String),
}

/// A single redaction rule describing what to find and how to replace it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedactionRule {
    /// The pattern that identifies sensitive data.
    pub pattern: RedactionPattern,
    /// The replacement text.
    pub replacement: String,
    /// Which content types this rule applies to.
    pub content_types: Vec<ContentType>,
}

/// Applies redaction rules to text content.
pub struct Redactor;

impl Redactor {
    /// Apply a set of redaction rules to the given content string.
    ///
    /// Rules are applied in order. Regex patterns that fail to compile are
    /// silently skipped.
    pub fn redact(content: &str, rules: &[RedactionRule]) -> String {
        let mut result = content.to_string();

        for rule in rules {
            match &rule.pattern {
                RedactionPattern::Regex(pattern) => {
                    if let Ok(re) = regex::Regex::new(pattern) {
                        result = re.replace_all(&result, rule.replacement.as_str()).to_string();
                    }
                }
                RedactionPattern::Literal(literal) => {
                    result = result.replace(literal, &rule.replacement);
                }
            }
        }

        result
    }

    /// Return a set of built-in redaction rules that cover common secrets.
    pub fn builtin_rules() -> Vec<RedactionRule> {
        vec![
            // Passwords in key=value or YAML/JSON style
            RedactionRule {
                pattern: RedactionPattern::Regex(
                    r#"(?i)(password|passwd|pwd)\s*[:=]\s*\S+"#.to_string(),
                ),
                replacement: "$1=***REDACTED***".to_string(),
                content_types: vec![ContentType::All],
            },
            // Bearer tokens and API keys
            RedactionRule {
                pattern: RedactionPattern::Regex(
                    r#"(?i)(token|api[_-]?key|secret[_-]?key|access[_-]?key)\s*[:=]\s*\S+"#
                        .to_string(),
                ),
                replacement: "$1=***REDACTED***".to_string(),
                content_types: vec![ContentType::All],
            },
            // PEM private keys
            RedactionRule {
                pattern: RedactionPattern::Regex(
                    r#"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"#
                        .to_string(),
                ),
                replacement: "***PRIVATE KEY REDACTED***".to_string(),
                content_types: vec![ContentType::All],
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_password() {
        let content = "db_password=supersecret123";
        let rules = Redactor::builtin_rules();
        let redacted = Redactor::redact(content, &rules);

        assert!(!redacted.contains("supersecret123"), "password should be redacted");
        assert!(redacted.contains("REDACTED"), "replacement marker should be present");
    }

    #[test]
    fn test_redact_private_key() {
        let content = r#"credentials:
-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF
-----END RSA PRIVATE KEY-----
done"#;
        let rules = Redactor::builtin_rules();
        let redacted = Redactor::redact(content, &rules);

        assert!(
            !redacted.contains("MIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn"),
            "private key body should be redacted"
        );
        assert!(redacted.contains("PRIVATE KEY REDACTED"));
    }

    #[test]
    fn test_redact_token() {
        let content = "api_key=abc123secret";
        let rules = Redactor::builtin_rules();
        let redacted = Redactor::redact(content, &rules);

        assert!(!redacted.contains("abc123secret"));
        assert!(redacted.contains("REDACTED"));
    }

    #[test]
    fn test_literal_redaction() {
        let rules = vec![RedactionRule {
            pattern: RedactionPattern::Literal("my-secret-value".to_string()),
            replacement: "***".to_string(),
            content_types: vec![ContentType::All],
        }];
        let result = Redactor::redact("data: my-secret-value here", &rules);
        assert_eq!(result, "data: *** here");
    }
}
