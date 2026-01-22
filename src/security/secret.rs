//! Secure secret storage with automatic zeroization
//!
//! This module provides types that securely store sensitive data in memory
//! and automatically zero it when dropped, preventing secret leakage.

use std::fmt;
use zeroize::{Zeroize, Zeroizing};

/// A string that is automatically zeroed when dropped.
///
/// Use this for storing passwords, tokens, keys, and other sensitive data.
/// Debug output is redacted to prevent accidental logging.
#[derive(Clone, Zeroize)]
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    /// Create a new secret string.
    pub fn new(s: impl Into<String>) -> Self {
        Self(Zeroizing::new(s.into()))
    }

    /// Expose the secret value.
    ///
    /// Use sparingly and only when the value is actually needed.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Get the secret as bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Check if the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

/// A byte vector that is automatically zeroed when dropped.
///
/// Use for storing binary secrets like encryption keys.
#[derive(Clone, Zeroize)]
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
    /// Create new secret bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Create from a string.
    pub fn from_str(s: &str) -> Self {
        Self::new(s.as_bytes().to_vec())
    }

    /// Expose the secret bytes.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Try to convert to a UTF-8 string.
    pub fn to_string(&self) -> Option<String> {
        String::from_utf8(self.0.to_vec()).ok()
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl From<Vec<u8>> for SecretBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self::new(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_string_redacted_debug() {
        let secret = SecretString::new("my_password");
        let debug = format!("{:?}", secret);
        assert_eq!(debug, "[REDACTED]");
        assert!(!debug.contains("my_password"));
    }

    #[test]
    fn test_secret_string_expose() {
        let secret = SecretString::new("my_password");
        assert_eq!(secret.expose(), "my_password");
    }

    #[test]
    fn test_secret_bytes_redacted_debug() {
        let secret = SecretBytes::from_str("secret_key");
        let debug = format!("{:?}", secret);
        assert_eq!(debug, "[REDACTED]");
    }

    #[test]
    fn test_secret_bytes_to_string() {
        let secret = SecretBytes::from_str("hello");
        assert_eq!(secret.to_string(), Some("hello".to_string()));
    }
}
