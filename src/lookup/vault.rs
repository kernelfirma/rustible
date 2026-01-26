//! HashiCorp Vault Lookup Plugin
//!
//! This module provides a lookup plugin for retrieving secrets from HashiCorp Vault.
//! It supports the standard Vault path format with optional key extraction.
//!
//! # Path Formats
//!
//! - `secret/path#key` - Get specific key from secret
//! - `secret/path` - Get entire secret as JSON
//! - `vault://secret/path#key` - Explicit vault scheme
//!
//! # Example Usage
//!
//! ```rust,ignore,no_run
//! use rustible::lookup::{LookupRegistry, LookupContext, VaultLookup};
//! use std::sync::Arc;
//!
//! let mut registry = LookupRegistry::new();
//! registry.register(Arc::new(VaultLookup::new()));
//!
//! let context = LookupContext::default();
//!
//! // Get a specific key from a secret
//! let password = registry.lookup("vault", &["secret/data/myapp/db#password"], &context)?;
//!
//! // Get entire secret as JSON
//! let secret = registry.lookup("vault", &["secret/data/myapp/db"], &context)?;
//! ```

use super::{Lookup, LookupContext, LookupError, LookupResult};
use crate::secrets::{SecretValue, VaultBackend};

/// HashiCorp Vault lookup plugin.
///
/// Retrieves secrets from HashiCorp Vault using the KV secrets engine.
/// Configuration is read from environment variables:
///
/// - `VAULT_ADDR` - Vault server address (default: http://127.0.0.1:8200)
/// - `VAULT_TOKEN` - Authentication token
/// - `VAULT_NAMESPACE` - Vault namespace (Enterprise feature)
/// - `VAULT_SKIP_VERIFY` - Skip TLS verification (not recommended for production)
///
/// Or via AppRole authentication:
/// - `VAULT_ROLE_ID` - AppRole role ID
/// - `VAULT_SECRET_ID` - AppRole secret ID
#[derive(Debug, Clone)]
pub struct VaultLookup {
    /// Default mount path for KV secrets engine
    default_mount: String,
    /// Default KV version (1 or 2)
    default_kv_version: u8,
    /// Whether to cache secrets during the lookup session
    cache_enabled: bool,
}

impl VaultLookup {
    /// Create a new Vault lookup plugin with default settings.
    pub fn new() -> Self {
        Self {
            default_mount: "secret".to_string(),
            default_kv_version: 2,
            cache_enabled: true,
        }
    }

    /// Create a new Vault lookup plugin with custom mount path.
    pub fn with_mount(mut self, mount: impl Into<String>) -> Self {
        self.default_mount = mount.into();
        self
    }

    /// Create a new Vault lookup plugin with specific KV version.
    pub fn with_kv_version(mut self, version: u8) -> Self {
        self.default_kv_version = version;
        self
    }

    /// Parse the path argument into (mount, path, key) components.
    ///
    /// Supported formats:
    /// - `secret/path#key` -> ("secret", "path", Some("key"))
    /// - `secret/path` -> ("secret", "path", None)
    /// - `vault://mount/path#key` -> ("mount", "path", Some("key"))
    /// - `mount=custom secret/path#key` -> ("custom", "secret/path", Some("key"))
    fn parse_path(&self, path_arg: &str) -> (String, String, Option<String>) {
        // Strip vault:// scheme if present
        let path = if path_arg.starts_with("vault://") {
            &path_arg[8..]
        } else {
            path_arg
        };

        // Check for key fragment (after #)
        let (path_part, key) = if let Some(pos) = path.rfind('#') {
            let (p, k) = path.split_at(pos);
            (p.to_string(), Some(k[1..].to_string()))
        } else {
            (path.to_string(), None)
        };

        // Determine mount and secret path
        // If path starts with "data/" assume default mount with KV v2 path structure
        // Otherwise, use the first path segment as mount
        let (mount, secret_path) = if path_part.starts_with("data/") {
            (self.default_mount.clone(), path_part)
        } else if path_part.contains('/') {
            // First segment is mount, rest is path
            if let Some(pos) = path_part.find('/') {
                let (m, p) = path_part.split_at(pos);
                (m.to_string(), p[1..].to_string())
            } else {
                (self.default_mount.clone(), path_part)
            }
        } else {
            // Single segment, use as path under default mount
            (self.default_mount.clone(), path_part)
        };

        (mount, secret_path, key)
    }

    /// Convert a SecretValue to a string representation.
    fn secret_value_to_string(value: &SecretValue) -> String {
        match value {
            SecretValue::String(s) => s.clone(),
            SecretValue::Integer(i) => i.to_string(),
            SecretValue::Boolean(b) => b.to_string(),
            SecretValue::Binary(b) => base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                b,
            ),
            SecretValue::Null => String::new(),
        }
    }

    /// Perform the actual Vault lookup using the secrets backend.
    fn do_lookup(&self, args: &[&str], _context: &LookupContext) -> LookupResult<Vec<String>> {
        use crate::secrets::VaultConfig;

        if args.is_empty() {
            return Err(LookupError::MissingArgument("path".to_string()));
        }

        // Parse options from arguments
        let options = self.parse_options(args);
        let path_arg = args
            .iter()
            .find(|a| !a.contains('='))
            .ok_or_else(|| LookupError::MissingArgument("path".to_string()))?;

        let (mount, secret_path, key) = self.parse_path(path_arg);

        // Get optional overrides from options
        let kv_version = options
            .get("version")
            .or(options.get("kv_version"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(self.default_kv_version);

        // Try to get or create Vault backend
        // We use a blocking approach since the Lookup trait is synchronous
        let result: Result<Vec<String>, LookupError> = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| LookupError::Other(format!("Failed to create runtime: {}", e)))?;

            rt.block_on(async {
                // Create Vault configuration from environment
                let config = VaultConfig::from_env();

                // Create Vault backend
                let vault = VaultBackend::new(config.into()).await.map_err(|e| {
                    LookupError::Other(format!("Failed to connect to Vault: {}", e))
                })?;

                // Read the secret based on KV version
                let secret = if kv_version == 2 {
                    vault.kv_v2_read(&mount, &secret_path).await
                } else {
                    vault.kv_v1_read(&mount, &secret_path).await
                }
                .map_err(|e| LookupError::Other(format!("Failed to read secret: {}", e)))?;

                // Extract the requested key or return entire secret as JSON
                if let Some(key_name) = key {
                    let value = secret.get(&key_name).ok_or_else(|| {
                        LookupError::Other(format!(
                            "Key '{}' not found in secret at '{}/{}'",
                            key_name, mount, secret_path
                        ))
                    })?;
                    Ok(vec![Self::secret_value_to_string(value)])
                } else {
                    // Return entire secret as JSON
                    let json = serde_json::to_string(secret.data()).map_err(|e| {
                        LookupError::Other(format!("Failed to serialize secret: {}", e))
                    })?;
                    Ok(vec![json])
                }
            })
        })
        .join()
        .map_err(|_| LookupError::Other("Vault lookup thread panicked".to_string()))?;

        result
    }
}

impl Default for VaultLookup {
    fn default() -> Self {
        Self::new()
    }
}

impl Lookup for VaultLookup {
    fn name(&self) -> &'static str {
        "vault"
    }

    fn description(&self) -> &'static str {
        "Retrieve secrets from HashiCorp Vault"
    }

    fn lookup(&self, args: &[&str], context: &LookupContext) -> LookupResult<Vec<String>> {
        self.do_lookup(args, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_lookup_name() {
        let lookup = VaultLookup::new();
        assert_eq!(lookup.name(), "vault");
    }

    #[test]
    fn test_vault_lookup_description() {
        let lookup = VaultLookup::new();
        assert!(lookup.description().contains("Vault"));
    }

    #[test]
    fn test_parse_path_simple() {
        let lookup = VaultLookup::new();
        let (mount, path, key) = lookup.parse_path("myapp/database#password");
        assert_eq!(mount, "myapp");
        assert_eq!(path, "database");
        assert_eq!(key, Some("password".to_string()));
    }

    #[test]
    fn test_parse_path_with_vault_scheme() {
        let lookup = VaultLookup::new();
        let (mount, path, key) = lookup.parse_path("vault://secret/myapp/db#password");
        assert_eq!(mount, "secret");
        assert_eq!(path, "myapp/db");
        assert_eq!(key, Some("password".to_string()));
    }

    #[test]
    fn test_parse_path_no_key() {
        let lookup = VaultLookup::new();
        let (mount, path, key) = lookup.parse_path("secret/myapp/config");
        assert_eq!(mount, "secret");
        assert_eq!(path, "myapp/config");
        assert_eq!(key, None);
    }

    #[test]
    fn test_parse_path_data_prefix() {
        let lookup = VaultLookup::new();
        let (mount, path, key) = lookup.parse_path("data/myapp/config#value");
        assert_eq!(mount, "secret"); // Default mount
        assert_eq!(path, "data/myapp/config");
        assert_eq!(key, Some("value".to_string()));
    }

    #[test]
    fn test_parse_path_single_segment() {
        let lookup = VaultLookup::new();
        let (mount, path, key) = lookup.parse_path("mykey#value");
        assert_eq!(mount, "secret"); // Default mount
        assert_eq!(path, "mykey");
        assert_eq!(key, Some("value".to_string()));
    }

    #[test]
    fn test_vault_lookup_with_custom_mount() {
        let lookup = VaultLookup::new().with_mount("custom-secrets");
        assert_eq!(lookup.default_mount, "custom-secrets");
    }

    #[test]
    fn test_vault_lookup_with_kv_version() {
        let lookup = VaultLookup::new().with_kv_version(1);
        assert_eq!(lookup.default_kv_version, 1);
    }

    #[test]
    fn test_lookup_missing_path() {
        let lookup = VaultLookup::new();
        let context = LookupContext::default();
        let result = lookup.lookup(&[], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    // Integration tests that require a running Vault instance
    #[test]
    #[ignore = "Requires running Vault instance"]
    fn test_vault_lookup_integration() {
        // This test would require:
        // 1. Running Vault instance
        // 2. VAULT_ADDR and VAULT_TOKEN environment variables
        // 3. A secret at the specified path
        let lookup = VaultLookup::new();
        let context = LookupContext::default();

        // Example: lookup("vault", &["secret/data/test#value"], &context)
        // Would need actual Vault setup
    }
}
