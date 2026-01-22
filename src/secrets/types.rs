//! Core types for secret management.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use super::error::{SecretError, SecretResult};
use super::no_log::SensitiveString;

/// A secret containing key-value data.
///
/// Secrets can contain multiple key-value pairs, such as a database
/// connection secret containing `username`, `password`, and `host`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Secret {
    /// The secret path/name
    path: String,

    /// The secret data (key-value pairs)
    data: HashMap<String, SecretValue>,

    /// Metadata about the secret
    metadata: SecretMetadata,
}

impl Secret {
    /// Create a new secret with the given path and data.
    pub fn new(path: impl Into<String>, data: HashMap<String, SecretValue>) -> Self {
        Self {
            path: path.into(),
            data,
            metadata: SecretMetadata::default(),
        }
    }

    /// Create a new secret with metadata.
    pub fn with_metadata(
        path: impl Into<String>,
        data: HashMap<String, SecretValue>,
        metadata: SecretMetadata,
    ) -> Self {
        Self {
            path: path.into(),
            data,
            metadata,
        }
    }

    /// Get the secret path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get all secret data.
    pub fn data(&self) -> &HashMap<String, SecretValue> {
        &self.data
    }

    /// Get mutable access to secret data.
    pub fn data_mut(&mut self) -> &mut HashMap<String, SecretValue> {
        &mut self.data
    }

    /// Get the secret metadata.
    pub fn metadata(&self) -> &SecretMetadata {
        &self.metadata
    }

    /// Get a specific value by key.
    pub fn get(&self, key: &str) -> Option<&SecretValue> {
        self.data.get(key)
    }

    /// Get a string value by key.
    pub fn get_string(&self, key: &str) -> SecretResult<String> {
        match self.data.get(key) {
            Some(SecretValue::String(s)) => Ok(s.clone()),
            Some(_) => Err(SecretError::TypeMismatch {
                key: key.to_string(),
                expected: "string".to_string(),
            }),
            None => Err(SecretError::KeyNotFound(key.to_string())),
        }
    }

    /// Get a value as a SensitiveString (for no_log protection).
    pub fn get_sensitive(&self, key: &str) -> SecretResult<SensitiveString> {
        let value = self.get_string(key)?;
        Ok(SensitiveString::new(value))
    }

    /// Get an integer value by key.
    pub fn get_int(&self, key: &str) -> SecretResult<i64> {
        match self.data.get(key) {
            Some(SecretValue::Integer(i)) => Ok(*i),
            Some(SecretValue::String(s)) => s.parse().map_err(|_| SecretError::TypeMismatch {
                key: key.to_string(),
                expected: "integer".to_string(),
            }),
            Some(_) => Err(SecretError::TypeMismatch {
                key: key.to_string(),
                expected: "integer".to_string(),
            }),
            None => Err(SecretError::KeyNotFound(key.to_string())),
        }
    }

    /// Get a boolean value by key.
    pub fn get_bool(&self, key: &str) -> SecretResult<bool> {
        match self.data.get(key) {
            Some(SecretValue::Boolean(b)) => Ok(*b),
            Some(SecretValue::String(s)) => match s.to_lowercase().as_str() {
                "true" | "yes" | "1" => Ok(true),
                "false" | "no" | "0" => Ok(false),
                _ => Err(SecretError::TypeMismatch {
                    key: key.to_string(),
                    expected: "boolean".to_string(),
                }),
            },
            Some(_) => Err(SecretError::TypeMismatch {
                key: key.to_string(),
                expected: "boolean".to_string(),
            }),
            None => Err(SecretError::KeyNotFound(key.to_string())),
        }
    }

    /// Get binary data by key.
    pub fn get_binary(&self, key: &str) -> SecretResult<Vec<u8>> {
        match self.data.get(key) {
            Some(SecretValue::Binary(b)) => Ok(b.clone()),
            Some(_) => Err(SecretError::TypeMismatch {
                key: key.to_string(),
                expected: "binary".to_string(),
            }),
            None => Err(SecretError::KeyNotFound(key.to_string())),
        }
    }

    /// Insert a value.
    pub fn insert(&mut self, key: impl Into<String>, value: SecretValue) {
        self.data.insert(key.into(), value);
    }

    /// Remove a value.
    pub fn remove(&mut self, key: &str) -> Option<SecretValue> {
        self.data.remove(key)
    }

    /// Check if the secret contains a key.
    pub fn contains_key(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// Get the number of key-value pairs.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get all keys.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.data.keys()
    }

    /// Convert to a simple string map (for Vault compatibility).
    pub fn to_string_map(&self) -> HashMap<String, String> {
        self.data
            .iter()
            .filter_map(|(k, v)| match v {
                SecretValue::String(s) => Some((k.clone(), s.clone())),
                SecretValue::Integer(i) => Some((k.clone(), i.to_string())),
                SecretValue::Boolean(b) => Some((k.clone(), b.to_string())),
                SecretValue::Binary(b) => Some((
                    k.clone(),
                    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b),
                )),
                SecretValue::Null => None,
            })
            .collect()
    }

    /// Create from a simple string map.
    pub fn from_string_map(path: impl Into<String>, map: HashMap<String, String>) -> Self {
        let data = map
            .into_iter()
            .map(|(k, v)| (k, SecretValue::String(v)))
            .collect();
        Self::new(path, data)
    }
}

// Implement Debug to hide sensitive data
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret")
            .field("path", &self.path)
            .field("keys", &self.data.keys().collect::<Vec<_>>())
            .field("metadata", &self.metadata)
            .finish()
    }
}

/// A value stored in a secret.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum SecretValue {
    /// A string value
    String(String),
    /// An integer value
    Integer(i64),
    /// A boolean value
    Boolean(bool),
    /// Binary data (stored as base64 in JSON)
    #[serde(with = "base64_serde")]
    Binary(Vec<u8>),
    /// Null value
    Null,
}

impl SecretValue {
    /// Get as string if possible.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            SecretValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get as integer if possible.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            SecretValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    /// Get as boolean if possible.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            SecretValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Get as binary if possible.
    pub fn as_binary(&self) -> Option<&[u8]> {
        match self {
            SecretValue::Binary(b) => Some(b),
            _ => None,
        }
    }

    /// Check if null.
    pub fn is_null(&self) -> bool {
        matches!(self, SecretValue::Null)
    }
}

// Implement Debug to hide sensitive data
impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretValue::String(_) => write!(f, "String([REDACTED])"),
            SecretValue::Integer(_) => write!(f, "Integer([REDACTED])"),
            SecretValue::Boolean(b) => write!(f, "Boolean({})", b),
            SecretValue::Binary(b) => write!(f, "Binary({} bytes)", b.len()),
            SecretValue::Null => write!(f, "Null"),
        }
    }
}

impl From<String> for SecretValue {
    fn from(s: String) -> Self {
        SecretValue::String(s)
    }
}

impl From<&str> for SecretValue {
    fn from(s: &str) -> Self {
        SecretValue::String(s.to_string())
    }
}

impl From<i64> for SecretValue {
    fn from(i: i64) -> Self {
        SecretValue::Integer(i)
    }
}

impl From<i32> for SecretValue {
    fn from(i: i32) -> Self {
        SecretValue::Integer(i64::from(i))
    }
}

impl From<bool> for SecretValue {
    fn from(b: bool) -> Self {
        SecretValue::Boolean(b)
    }
}

impl From<Vec<u8>> for SecretValue {
    fn from(b: Vec<u8>) -> Self {
        SecretValue::Binary(b)
    }
}

/// Metadata about a secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecretMetadata {
    /// Secret version
    pub version: Option<SecretVersion>,

    /// Creation timestamp (Unix seconds)
    pub created_time: Option<i64>,

    /// Last update timestamp (Unix seconds)
    pub updated_time: Option<i64>,

    /// Custom metadata
    #[serde(default)]
    pub custom: HashMap<String, String>,

    /// Whether the secret is marked for deletion
    pub deletion_time: Option<i64>,

    /// Whether the secret is destroyed (permanently deleted)
    pub destroyed: bool,
}

impl SecretMetadata {
    /// Create new metadata with a version.
    pub fn with_version(version: impl Into<SecretVersion>) -> Self {
        Self {
            version: Some(version.into()),
            ..Default::default()
        }
    }

    /// Set the version.
    pub fn set_version(&mut self, version: impl Into<SecretVersion>) {
        self.version = Some(version.into());
    }

    /// Add custom metadata.
    pub fn add_custom(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.custom.insert(key.into(), value.into());
    }
}

/// Secret version identifier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SecretVersion {
    /// Numeric version (Vault KV v2)
    Numeric(u64),
    /// String version ID (AWS Secrets Manager)
    String(String),
    /// Latest version (no specific version)
    Latest,
}

impl SecretVersion {
    /// Get as numeric version if possible.
    pub fn as_numeric(&self) -> Option<u64> {
        match self {
            SecretVersion::Numeric(n) => Some(*n),
            _ => None,
        }
    }

    /// Get as string version if possible.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            SecretVersion::String(s) => Some(s),
            _ => None,
        }
    }
}

impl From<u64> for SecretVersion {
    fn from(n: u64) -> Self {
        SecretVersion::Numeric(n)
    }
}

impl From<String> for SecretVersion {
    fn from(s: String) -> Self {
        SecretVersion::String(s)
    }
}

impl From<&str> for SecretVersion {
    fn from(s: &str) -> Self {
        SecretVersion::String(s.to_string())
    }
}

impl fmt::Display for SecretVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecretVersion::Numeric(n) => write!(f, "{}", n),
            SecretVersion::String(s) => write!(f, "{}", s),
            SecretVersion::Latest => write!(f, "latest"),
        }
    }
}

/// Base64 serialization for binary data.
mod base64_serde {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        STANDARD.decode(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_creation() {
        let mut data = HashMap::new();
        data.insert(
            "username".to_string(),
            SecretValue::String("admin".to_string()),
        );
        data.insert(
            "password".to_string(),
            SecretValue::String("secret123".to_string()),
        );

        let secret = Secret::new("secret/database", data);

        assert_eq!(secret.path(), "secret/database");
        assert_eq!(secret.len(), 2);
        assert!(secret.contains_key("username"));
        assert!(secret.contains_key("password"));
    }

    #[test]
    fn test_secret_get_sensitive() {
        let mut data = HashMap::new();
        data.insert(
            "password".to_string(),
            SecretValue::String("secret123".to_string()),
        );
        let secret = Secret::new("test", data);

        let sensitive = secret.get_sensitive("password").unwrap();
        // Debug should show redacted
        assert!(format!("{:?}", sensitive).contains("REDACTED"));
        // expose() should return the actual value
        assert_eq!(sensitive.expose(), "secret123");
    }

    #[test]
    fn test_secret_debug_hides_values() {
        let mut data = HashMap::new();
        data.insert(
            "password".to_string(),
            SecretValue::String("secret123".to_string()),
        );
        let secret = Secret::new("test", data);

        let debug = format!("{:?}", secret);
        assert!(!debug.contains("secret123"));
        assert!(debug.contains("password")); // key is visible
    }

    #[test]
    fn test_secret_value_types() {
        assert!(SecretValue::String("test".to_string())
            .as_string()
            .is_some());
        assert!(SecretValue::Integer(42).as_int().is_some());
        assert!(SecretValue::Boolean(true).as_bool().is_some());
        assert!(SecretValue::Binary(vec![1, 2, 3]).as_binary().is_some());
        assert!(SecretValue::Null.is_null());
    }

    #[test]
    fn test_secret_version() {
        let numeric = SecretVersion::Numeric(5);
        assert_eq!(numeric.as_numeric(), Some(5));
        assert_eq!(format!("{}", numeric), "5");

        let string = SecretVersion::String("v1.0.0".to_string());
        assert_eq!(string.as_string(), Some("v1.0.0"));
        assert_eq!(format!("{}", string), "v1.0.0");
    }

    #[test]
    fn test_secret_to_string_map() {
        let mut data = HashMap::new();
        data.insert("str".to_string(), SecretValue::String("value".to_string()));
        data.insert("int".to_string(), SecretValue::Integer(42));
        data.insert("bool".to_string(), SecretValue::Boolean(true));

        let secret = Secret::new("test", data);
        let map = secret.to_string_map();

        assert_eq!(map.get("str"), Some(&"value".to_string()));
        assert_eq!(map.get("int"), Some(&"42".to_string()));
        assert_eq!(map.get("bool"), Some(&"true".to_string()));
    }
}
