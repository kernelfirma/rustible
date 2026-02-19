//! Lookup Plugin System for Rustible
//!
//! This module provides lookup plugins that can retrieve data from external sources
//! and make it available as variables during playbook execution. Lookup plugins are
//! similar to Ansible's lookup plugins and can be used in template expressions.
//!
//! # Available Plugins
//!
//! - [`FileLookup`] - Read file contents from the filesystem
//! - [`EnvLookup`] - Read environment variables
//! - [`PasswordLookup`] - Generate random passwords
//! - [`PipeLookup`] - Execute commands and capture output
//! - [`UrlLookup`] - Fetch content from HTTP/HTTPS URLs
//! - [`TemplateLookup`] - Render template files using MiniJinja
//! - [`ItemsLookup`] - Return items for list iteration
//!
//! # Example Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::lookup::{LookupRegistry, LookupContext};
//!
//! let registry = LookupRegistry::with_builtins();
//! let context = LookupContext::default();
//!
//! // Read a file
//! let content = registry.lookup("file", &["/etc/hostname"], &context)?;
//!
//! // Get an environment variable
//! let home = registry.lookup("env", &["HOME"], &context)?;
//!
//! // Generate a password
//! let password = registry.lookup("password", &["length=16"], &context)?;
//! # Ok(())
//! # }
//! ```

pub mod env;
pub mod file;
pub mod items;
pub mod password;
pub mod pipe;
pub mod template;
pub mod url;
#[cfg(feature = "experimental")]
pub mod vault;

pub use env::EnvLookup;
pub use file::FileLookup;
pub use items::ItemsLookup;
pub use password::PasswordLookup;
pub use pipe::PipeLookup;
pub use template::TemplateLookup;
pub use url::UrlLookup;
#[cfg(feature = "experimental")]
pub use vault::VaultLookup;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during lookup operations
#[derive(Error, Debug)]
pub enum LookupError {
    /// Lookup plugin not found
    #[error("Lookup plugin not found: {0}")]
    NotFound(String),

    /// Invalid arguments provided to lookup
    #[error("Invalid lookup arguments: {0}")]
    InvalidArguments(String),

    /// Missing required argument
    #[error("Missing required argument: {0}")]
    MissingArgument(String),

    /// File not found
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    /// IO error during lookup
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP error during URL lookup
    #[error("HTTP error: {0}")]
    Http(String),

    /// Command execution failed
    #[error("Command execution failed: {0}")]
    CommandFailed(String),

    /// Environment variable not found
    #[error("Environment variable not found: {0}")]
    EnvNotFound(String),

    /// Parse error
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Timeout during lookup
    #[error("Lookup timed out after {0} seconds")]
    Timeout(u64),

    /// Generic lookup error
    #[error("{0}")]
    Other(String),
}

/// Result type for lookup operations
pub type LookupResult<T> = Result<T, LookupError>;

/// Context for lookup plugin execution
#[derive(Debug, Clone)]
pub struct LookupContext {
    /// Base directory for relative file paths
    pub base_dir: Option<PathBuf>,

    /// Variables available during lookup
    pub vars: HashMap<String, serde_json::Value>,

    /// Whether to allow unsafe operations (e.g., arbitrary commands)
    pub allow_unsafe: bool,

    /// Timeout for network operations in seconds
    pub timeout_secs: u64,

    /// Whether to fail on errors or return default values
    pub fail_on_error: bool,

    /// Default value to return when lookup fails and fail_on_error is false
    pub default_value: Option<String>,
}

impl LookupContext {
    /// Create a new lookup context
    pub fn new() -> Self {
        Self {
            base_dir: None,
            vars: HashMap::new(),
            allow_unsafe: false,
            timeout_secs: 30,
            fail_on_error: true,
            default_value: None,
        }
    }

    /// Set the base directory for relative paths
    pub fn with_base_dir(mut self, base_dir: impl Into<PathBuf>) -> Self {
        self.base_dir = Some(base_dir.into());
        self
    }

    /// Set variables for template rendering
    pub fn with_vars(mut self, vars: HashMap<String, serde_json::Value>) -> Self {
        self.vars = vars;
        self
    }

    /// Allow unsafe operations
    pub fn with_allow_unsafe(mut self, allow: bool) -> Self {
        self.allow_unsafe = allow;
        self
    }

    /// Set timeout for network operations
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Set whether to fail on errors
    pub fn with_fail_on_error(mut self, fail: bool) -> Self {
        self.fail_on_error = fail;
        self
    }

    /// Set default value for failed lookups
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default_value = Some(default.into());
        self
    }
}

impl Default for LookupContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait that all lookup plugins must implement
pub trait Lookup: Send + Sync {
    /// Returns the name of the lookup plugin
    fn name(&self) -> &'static str;

    /// Returns a description of what this lookup does
    fn description(&self) -> &'static str;

    /// Execute the lookup with the given arguments
    ///
    /// # Arguments
    ///
    /// * `args` - Arguments to the lookup (e.g., file path, URL, etc.)
    /// * `context` - Lookup execution context
    ///
    /// # Returns
    ///
    /// A vector of strings, one for each resolved value.
    /// Most lookups return a single value, but some (like file glob) may return multiple.
    fn lookup(&self, args: &[&str], context: &LookupContext) -> LookupResult<Vec<String>>;

    /// Parse key=value arguments from the args list
    fn parse_options(&self, args: &[&str]) -> HashMap<String, String> {
        let mut options = HashMap::new();
        for arg in args {
            if let Some((key, value)) = arg.split_once('=') {
                options.insert(key.to_string(), value.to_string());
            }
        }
        options
    }
}

/// Registry for looking up plugins by name
pub struct LookupRegistry {
    plugins: HashMap<String, Arc<dyn Lookup>>,
}

impl LookupRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// Create a registry with all built-in lookup plugins
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        // Register all built-in lookup plugins
        registry.register(Arc::new(FileLookup::new()));
        registry.register(Arc::new(EnvLookup::new()));
        registry.register(Arc::new(PasswordLookup::new()));
        registry.register(Arc::new(PipeLookup::new()));
        registry.register(Arc::new(UrlLookup::new()));
        registry.register(Arc::new(TemplateLookup::new()));
        registry.register(Arc::new(ItemsLookup::new()));

        // Register Vault lookup when experimental feature is enabled
        #[cfg(feature = "experimental")]
        registry.register(Arc::new(VaultLookup::new()));

        registry
    }

    /// Register a lookup plugin
    pub fn register(&mut self, plugin: Arc<dyn Lookup>) {
        self.plugins.insert(plugin.name().to_string(), plugin);
    }

    /// Get a lookup plugin by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Lookup>> {
        self.plugins.get(name).cloned()
    }

    /// Check if a lookup plugin exists
    pub fn contains(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Get all registered plugin names
    pub fn names(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }

    /// Execute a lookup by plugin name
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the lookup plugin
    /// * `args` - Arguments to pass to the lookup
    /// * `context` - Lookup execution context
    ///
    /// # Returns
    ///
    /// The lookup result as a vector of strings, or the first element if single value expected
    pub fn lookup(
        &self,
        name: &str,
        args: &[&str],
        context: &LookupContext,
    ) -> LookupResult<Vec<String>> {
        let plugin = self
            .get(name)
            .ok_or_else(|| LookupError::NotFound(name.to_string()))?;

        match plugin.lookup(args, context) {
            Ok(result) => Ok(result),
            Err(_e) if !context.fail_on_error => {
                // Return default value if fail_on_error is false
                Ok(vec![context.default_value.clone().unwrap_or_default()])
            }
            Err(e) => Err(e),
        }
    }

    /// Execute a lookup and return a single value
    ///
    /// This is a convenience method that returns the first value from the lookup.
    pub fn lookup_first(
        &self,
        name: &str,
        args: &[&str],
        context: &LookupContext,
    ) -> LookupResult<String> {
        let results = self.lookup(name, args, context)?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| LookupError::Other("Lookup returned no results".to_string()))
    }
}

impl Default for LookupRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

// ============================================================================
// Prelude Module
// ============================================================================

/// Convenient re-exports for lookup plugin development and usage.
pub mod prelude {
    pub use super::EnvLookup;
    pub use super::FileLookup;
    pub use super::ItemsLookup;
    pub use super::Lookup;
    pub use super::LookupContext;
    pub use super::LookupError;
    pub use super::LookupRegistry;
    pub use super::LookupResult;
    pub use super::PasswordLookup;
    pub use super::PipeLookup;
    pub use super::TemplateLookup;
    pub use super::UrlLookup;
    #[cfg(feature = "experimental")]
    pub use super::VaultLookup;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_with_builtins() {
        let registry = LookupRegistry::with_builtins();

        assert!(registry.contains("file"));
        assert!(registry.contains("env"));
        assert!(registry.contains("password"));
        assert!(registry.contains("pipe"));
        assert!(registry.contains("url"));
        assert!(registry.contains("template"));
    }

    #[test]
    fn test_registry_not_found() {
        let registry = LookupRegistry::new();
        let context = LookupContext::default();

        let result = registry.lookup("nonexistent", &[], &context);
        assert!(matches!(result, Err(LookupError::NotFound(_))));
    }

    #[test]
    fn test_lookup_context_builder() {
        let context = LookupContext::new()
            .with_base_dir("/tmp")
            .with_timeout(60)
            .with_allow_unsafe(true)
            .with_fail_on_error(false)
            .with_default("fallback");

        assert_eq!(context.base_dir, Some(PathBuf::from("/tmp")));
        assert_eq!(context.timeout_secs, 60);
        assert!(context.allow_unsafe);
        assert!(!context.fail_on_error);
        assert_eq!(context.default_value, Some("fallback".to_string()));
    }

    #[test]
    fn test_parse_options() {
        struct TestLookup;
        impl Lookup for TestLookup {
            fn name(&self) -> &'static str {
                "test"
            }
            fn description(&self) -> &'static str {
                "Test lookup"
            }
            fn lookup(
                &self,
                _args: &[&str],
                _context: &LookupContext,
            ) -> LookupResult<Vec<String>> {
                Ok(vec![])
            }
        }

        let lookup = TestLookup;
        let options = lookup.parse_options(&["key1=value1", "key2=value2", "no_equals"]);

        assert_eq!(options.get("key1"), Some(&"value1".to_string()));
        assert_eq!(options.get("key2"), Some(&"value2".to_string()));
        assert!(!options.contains_key("no_equals"));
    }
}
