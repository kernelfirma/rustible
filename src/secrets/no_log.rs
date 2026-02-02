//! No-log enforcement for sensitive data.
//!
//! This module provides mechanisms to prevent sensitive data from appearing
//! in logs, output, and error messages.

use parking_lot::RwLock;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

/// A string wrapper that prevents the value from being logged.
///
/// When used in format strings or logging, this type will display
/// `[REDACTED]` instead of the actual value. Use `expose()` to
/// access the underlying value when needed.
///
/// # Example
///
/// ```rust,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::secrets::SensitiveString;
///
/// let password = SensitiveString::new("secret123");
///
/// // This logs "[REDACTED]" instead of "secret123"
/// tracing::info!("Password is: {:?}", password);
///
/// // Access the actual value
/// let actual_value = password.expose();
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct SensitiveString {
    value: String,
}

impl SensitiveString {
    /// Create a new sensitive string.
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    /// Expose the underlying value.
    ///
    /// Use this method when you need to access the actual secret value,
    /// such as when passing it to an API or storing it.
    pub fn expose(&self) -> &str {
        &self.value
    }

    /// Consume and return the underlying value.
    pub fn into_inner(self) -> String {
        self.value
    }

    /// Get the length of the value.
    pub fn len(&self) -> usize {
        self.value.len()
    }

    /// Check if the value is empty.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Create an empty sensitive string.
    pub fn empty() -> Self {
        Self {
            value: String::new(),
        }
    }
}

// Display shows redacted value
impl fmt::Display for SensitiveString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[REDACTED]")
    }
}

// Debug shows redacted value
impl fmt::Debug for SensitiveString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SensitiveString([REDACTED])")
    }
}

// Don't implement Deref to String - force use of expose()
// This prevents accidental logging of the value

impl From<String> for SensitiveString {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for SensitiveString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl PartialEq for SensitiveString {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for SensitiveString {}

impl std::hash::Hash for SensitiveString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

// Serialize without exposing the value (serializes as redacted)
impl serde::Serialize for SensitiveString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Always serialize as [REDACTED] to prevent accidental exposure
        serializer.serialize_str("[REDACTED]")
    }
}

// Deserialize normally
impl<'de> serde::Deserialize<'de> for SensitiveString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::new(value))
    }
}

/// Registry for sensitive values that should be redacted.
///
/// This registry keeps track of all sensitive values that have been
/// loaded, allowing automatic redaction of these values in output.
pub struct NoLogRegistry {
    /// Set of sensitive values (hashed for security)
    values: RwLock<HashSet<String>>,
    /// Maximum number of values to track
    max_values: usize,
}

impl NoLogRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self {
            values: RwLock::new(HashSet::new()),
            max_values: 10000,
        }
    }

    /// Create a registry with a custom max size.
    pub fn with_max_values(max_values: usize) -> Self {
        Self {
            values: RwLock::new(HashSet::new()),
            max_values,
        }
    }

    /// Register a sensitive value.
    pub fn register(&self, value: impl Into<String>) {
        let value = value.into();
        if value.is_empty() {
            return;
        }

        let mut values = self.values.write();
        if values.len() < self.max_values {
            values.insert(value);
        }
    }

    /// Unregister a sensitive value.
    pub fn unregister(&self, value: &str) {
        let mut values = self.values.write();
        values.remove(value);
    }

    /// Check if a text contains any registered sensitive values.
    pub fn contains_sensitive(&self, text: &str) -> bool {
        let values = self.values.read();
        values.iter().any(|v| text.contains(v))
    }

    /// Redact all registered sensitive values from text.
    pub fn redact(&self, text: &str) -> String {
        let values = self.values.read();
        let mut result = text.to_string();

        for value in values.iter() {
            if !value.is_empty() && result.contains(value) {
                result = result.replace(value, "[REDACTED]");
            }
        }

        result
    }

    /// Redact sensitive values and return whether any redaction occurred.
    pub fn redact_with_flag(&self, text: &str) -> (String, bool) {
        let values = self.values.read();
        let mut result = text.to_string();
        let mut redacted = false;

        for value in values.iter() {
            if !value.is_empty() && result.contains(value) {
                result = result.replace(value, "[REDACTED]");
                redacted = true;
            }
        }

        (result, redacted)
    }

    /// Get the number of registered values.
    pub fn len(&self) -> usize {
        self.values.read().len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.values.read().is_empty()
    }

    /// Clear all registered values.
    pub fn clear(&self) {
        self.values.write().clear();
    }
}

impl Default for NoLogRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for NoLogRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NoLogRegistry")
            .field("registered_values", &self.len())
            .field("max_values", &self.max_values)
            .finish()
    }
}

/// Guard that ensures no_log enforcement within a scope.
///
/// When this guard is active, it registers values for redaction and
/// automatically unregisters them when dropped.
pub struct NoLogGuard {
    registry: Arc<NoLogRegistry>,
    values: Vec<String>,
}

impl NoLogGuard {
    /// Create a new guard with the given registry.
    pub fn new(registry: Arc<NoLogRegistry>) -> Self {
        Self {
            registry,
            values: Vec::new(),
        }
    }

    /// Register a value to be redacted.
    pub fn protect(&mut self, value: impl Into<String>) {
        let value = value.into();
        self.registry.register(value.clone());
        self.values.push(value);
    }

    /// Protect multiple values.
    pub fn protect_all(&mut self, values: impl IntoIterator<Item = impl Into<String>>) {
        for value in values {
            self.protect(value);
        }
    }
}

impl Drop for NoLogGuard {
    fn drop(&mut self) {
        // Unregister all protected values
        for value in &self.values {
            self.registry.unregister(value);
        }
    }
}

/// Trait for types that can be redacted.
pub trait Redactable {
    /// Return a redacted version of this value.
    fn redact(&self) -> String;

    /// Check if this value should be redacted.
    fn is_sensitive(&self) -> bool {
        true
    }
}

impl Redactable for SensitiveString {
    fn redact(&self) -> String {
        "[REDACTED]".to_string()
    }
}

impl Redactable for String {
    fn redact(&self) -> String {
        "[REDACTED]".to_string()
    }

    fn is_sensitive(&self) -> bool {
        false // Regular strings are not sensitive by default
    }
}

/// Helper function to check if a field name suggests sensitive data.
pub fn is_sensitive_field_name(name: &str) -> bool {
    let sensitive_patterns = [
        "password",
        "passwd",
        "pwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "api-key",
        "private_key",
        "privatekey",
        "private-key",
        "credential",
        "auth",
        "bearer",
        "access_key",
        "accesskey",
        "secret_key",
        "secretkey",
        "encryption_key",
        "encryptionkey",
        "ssh_key",
        "sshkey",
        "cert",
        "certificate",
    ];

    let lower = name.to_lowercase();
    sensitive_patterns
        .iter()
        .any(|pattern| lower.contains(pattern))
}

/// Macro to ensure no_log is respected in logging statements.
///
/// This macro wraps values and ensures they are redacted if sensitive.
#[macro_export]
macro_rules! no_log {
    ($value:expr) => {
        if $crate::secrets::is_sensitive_field_name(stringify!($value)) {
            "[REDACTED]".to_string()
        } else {
            format!("{:?}", $value)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensitive_string_display() {
        let secret = SensitiveString::new("my_secret_password");
        assert_eq!(format!("{}", secret), "[REDACTED]");
    }

    #[test]
    fn test_sensitive_string_debug() {
        let secret = SensitiveString::new("my_secret_password");
        assert!(format!("{:?}", secret).contains("REDACTED"));
        assert!(!format!("{:?}", secret).contains("my_secret_password"));
    }

    #[test]
    fn test_sensitive_string_expose() {
        let secret = SensitiveString::new("my_secret_password");
        assert_eq!(secret.expose(), "my_secret_password");
    }

    #[test]
    fn test_sensitive_string_into_inner() {
        let secret = SensitiveString::new("my_secret_password");
        assert_eq!(secret.into_inner(), "my_secret_password");
    }

    #[test]
    fn test_registry_register_and_redact() {
        let registry = NoLogRegistry::new();
        registry.register("secret_value");

        let text = "The password is secret_value, please keep it safe.";
        let redacted = registry.redact(text);

        assert!(redacted.contains("[REDACTED]"));
        assert!(!redacted.contains("secret_value"));
    }

    #[test]
    fn test_registry_contains_sensitive() {
        let registry = NoLogRegistry::new();
        registry.register("api_key_123");

        assert!(registry.contains_sensitive("My api_key_123 is here"));
        assert!(!registry.contains_sensitive("No secrets here"));
    }

    #[test]
    fn test_registry_multiple_values() {
        let registry = NoLogRegistry::new();
        registry.register("password123");
        registry.register("token456");

        let text = "password123 and token456 should be hidden";
        let redacted = registry.redact(text);

        assert!(!redacted.contains("password123"));
        assert!(!redacted.contains("token456"));
        assert_eq!(redacted.matches("[REDACTED]").count(), 2);
    }

    #[test]
    fn test_no_log_guard() {
        let registry = Arc::new(NoLogRegistry::new());

        {
            let mut guard = NoLogGuard::new(registry.clone());
            guard.protect("temp_secret");

            assert!(registry.contains_sensitive("Contains temp_secret"));
        }

        // After guard is dropped, value should be unregistered
        assert!(!registry.contains_sensitive("Contains temp_secret"));
    }

    #[test]
    fn test_sensitive_field_names() {
        assert!(is_sensitive_field_name("password"));
        assert!(is_sensitive_field_name("db_password"));
        assert!(is_sensitive_field_name("API_KEY"));
        assert!(is_sensitive_field_name("secret_token"));
        assert!(is_sensitive_field_name("private_key"));
        assert!(is_sensitive_field_name("ssh_key"));

        assert!(!is_sensitive_field_name("username"));
        assert!(!is_sensitive_field_name("hostname"));
        assert!(!is_sensitive_field_name("port"));
    }

    #[test]
    fn test_sensitive_string_serialization() {
        let secret = SensitiveString::new("actual_secret");
        let json = serde_json::to_string(&secret).unwrap();
        assert!(json.contains("REDACTED"));
        assert!(!json.contains("actual_secret"));
    }

    #[test]
    fn test_sensitive_string_equality() {
        let s1 = SensitiveString::new("same");
        let s2 = SensitiveString::new("same");
        let s3 = SensitiveString::new("different");

        assert_eq!(s1, s2);
        assert_ne!(s1, s3);
    }
}
