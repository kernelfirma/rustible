//! Lookup Plugin System for Rustible
//!
//! This module provides a lookup plugin infrastructure for retrieving data from
//! various sources during playbook execution. Lookups allow dynamic data retrieval
//! from files, environment variables, password stores, external commands, and more.
//!
//! # Architecture
//!
//! The lookup system consists of several key components:
//!
//! 1. **[`LookupPlugin`]** trait: Core trait for all lookup implementations
//! 2. **[`LookupRegistry`]**: Central registry for lookup plugin discovery
//! 3. **[`LookupContext`]**: Execution context passed to lookups
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::plugins::lookup::prelude::*;
//!
//! // Create registry with built-in lookups
//! let registry = LookupRegistry::new();
//!
//! // Look up a file's contents
//! let context = LookupContext::default();
//! let content = registry.lookup("file", &["config.txt"], &context)?;
//!
//! // Look up environment variable
//! let home = registry.lookup("env", &["HOME"], &context)?;
//!
//! // Look up with options
//! let options = LookupOptions::new()
//!     .with_option("default", "fallback_value");
//! let value = registry.lookup_with_options("env", &["MAYBE_SET"], &options, &context)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Creating Custom Lookups
//!
//! Implement [`LookupPlugin`] to create custom lookups:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::plugins::lookup::prelude::*;
//!
//! #[derive(Debug, Default)]
//! struct MyCustomLookup;
//!
//! impl LookupPlugin for MyCustomLookup {
//!     fn name(&self) -> &'static str {
//!         "custom"
//!     }
//!
//!     fn description(&self) -> &'static str {
//!         "My custom lookup plugin"
//!     }
//!
//!     fn lookup(
//!         &self,
//!         terms: &[String],
//!         options: &LookupOptions,
//!         context: &LookupContext,
//!     ) -> LookupResult<Vec<serde_json::Value>> {
//!         // Implementation
//!         Ok(terms.iter().map(|t| serde_json::json!(t.to_uppercase())).collect())
//!     }
//! }
//! # Ok(())
//! # }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during lookup operations
#[derive(Error, Debug)]
pub enum LookupError {
    /// The requested lookup plugin was not found
    #[error("Lookup plugin not found: {0}")]
    NotFound(String),

    /// Invalid lookup term or argument
    #[error("Invalid lookup term: {0}")]
    InvalidTerm(String),

    /// Missing required option
    #[error("Missing required option: {0}")]
    MissingOption(String),

    /// Invalid option value
    #[error("Invalid option '{option}': {message}")]
    InvalidOption { option: String, message: String },

    /// File not found during lookup
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    /// Permission denied during lookup
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// IO error during lookup
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Template rendering error
    #[error("Template error: {0}")]
    TemplateError(String),

    /// Command execution failed
    #[error("Command failed with exit code {code}: {message}")]
    CommandFailed { code: i32, message: String },

    /// Parse error (CSV, JSON, etc.)
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Environment variable not found
    #[error("Environment variable not found: {0}")]
    EnvNotFound(String),

    /// Password generation/retrieval error
    #[error("Password error: {0}")]
    PasswordError(String),

    /// Lookup execution failed
    #[error("Lookup failed: {0}")]
    ExecutionFailed(String),

    /// Timeout during lookup
    #[error("Lookup timed out after {0} seconds")]
    Timeout(u64),
}

/// Result type for lookup operations
pub type LookupResult<T> = Result<T, LookupError>;

// ============================================================================
// Lookup Options
// ============================================================================

/// Options that can be passed to lookup plugins
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LookupOptions {
    /// Key-value options passed to the lookup
    #[serde(flatten)]
    pub options: HashMap<String, serde_json::Value>,

    /// Lookup-specific errors behavior (fail or return default)
    #[serde(default)]
    pub errors: ErrorBehavior,

    /// Default value if lookup returns nothing
    #[serde(default)]
    pub default: Option<serde_json::Value>,

    /// Whether to split results (for string values)
    #[serde(default)]
    pub wantlist: bool,
}

impl LookupOptions {
    /// Create new empty options
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an option
    pub fn with_option(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }

    /// Set error behavior
    pub fn with_errors(mut self, behavior: ErrorBehavior) -> Self {
        self.errors = behavior;
        self
    }

    /// Set default value
    pub fn with_default(mut self, value: serde_json::Value) -> Self {
        self.default = Some(value);
        self
    }

    /// Set wantlist flag
    pub fn with_wantlist(mut self, wantlist: bool) -> Self {
        self.wantlist = wantlist;
        self
    }

    /// Get a string option
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.options.get(key).map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            _ => v.to_string().trim_matches('"').to_string(),
        })
    }

    /// Get a boolean option
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.options.get(key).and_then(|v| match v {
            serde_json::Value::Bool(b) => Some(*b),
            serde_json::Value::String(s) => match s.to_lowercase().as_str() {
                "true" | "yes" | "1" | "on" => Some(true),
                "false" | "no" | "0" | "off" => Some(false),
                _ => None,
            },
            _ => None,
        })
    }

    /// Get a boolean option with default
    pub fn get_bool_or(&self, key: &str, default: bool) -> bool {
        self.get_bool(key).unwrap_or(default)
    }

    /// Get an integer option
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.options.get(key).and_then(|v| match v {
            serde_json::Value::Number(n) => n.as_i64(),
            serde_json::Value::String(s) => s.parse().ok(),
            _ => None,
        })
    }

    /// Get an unsigned integer option
    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.options.get(key).and_then(|v| match v {
            serde_json::Value::Number(n) => n.as_u64(),
            serde_json::Value::String(s) => s.parse().ok(),
            _ => None,
        })
    }

    /// Check if option exists
    pub fn has(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }
}

/// Behavior when lookup encounters an error
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ErrorBehavior {
    /// Raise an error (default)
    #[default]
    Strict,
    /// Return empty list/None on error
    Ignore,
    /// Log warning and continue
    Warn,
}

impl fmt::Display for ErrorBehavior {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorBehavior::Strict => write!(f, "strict"),
            ErrorBehavior::Ignore => write!(f, "ignore"),
            ErrorBehavior::Warn => write!(f, "warn"),
        }
    }
}

// ============================================================================
// Lookup Context
// ============================================================================

/// Context for lookup execution
#[derive(Debug, Clone, Default)]
pub struct LookupContext {
    /// Variables available for template rendering
    pub variables: HashMap<String, serde_json::Value>,

    /// Facts about the current host
    pub facts: HashMap<String, serde_json::Value>,

    /// Current working directory for file lookups
    pub work_dir: Option<PathBuf>,

    /// Search paths for file lookups
    pub search_paths: Vec<PathBuf>,

    /// Whether running in check mode
    pub check_mode: bool,

    /// Environment variables override
    pub environment: HashMap<String, String>,

    /// Timeout for external commands (in seconds)
    pub timeout: Option<u64>,
}

impl LookupContext {
    /// Create a new context
    pub fn new() -> Self {
        Self::default()
    }

    /// Set variables
    pub fn with_variables(mut self, vars: HashMap<String, serde_json::Value>) -> Self {
        self.variables = vars;
        self
    }

    /// Add a variable
    pub fn with_variable(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.variables.insert(key.into(), value);
        self
    }

    /// Set facts
    pub fn with_facts(mut self, facts: HashMap<String, serde_json::Value>) -> Self {
        self.facts = facts;
        self
    }

    /// Set working directory
    pub fn with_work_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.work_dir = Some(path.into());
        self
    }

    /// Add search path
    pub fn with_search_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.search_paths.push(path.into());
        self
    }

    /// Set search paths
    pub fn with_search_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.search_paths = paths;
        self
    }

    /// Set check mode
    pub fn with_check_mode(mut self, check_mode: bool) -> Self {
        self.check_mode = check_mode;
        self
    }

    /// Set environment variables
    pub fn with_environment(mut self, env: HashMap<String, String>) -> Self {
        self.environment = env;
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: u64) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Get effective working directory
    pub fn effective_work_dir(&self) -> PathBuf {
        self.work_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    /// Resolve a path relative to search paths
    pub fn resolve_path(&self, path: &str) -> Option<PathBuf> {
        let path = PathBuf::from(path);

        // If absolute, check if it exists
        if path.is_absolute() {
            if path.exists() {
                return Some(path);
            }
            return None;
        }

        // Check relative to working directory first
        let work_dir = self.effective_work_dir();
        let full_path = work_dir.join(&path);
        if full_path.exists() {
            return Some(full_path);
        }

        // Check search paths
        for search_path in &self.search_paths {
            let full_path = search_path.join(&path);
            if full_path.exists() {
                return Some(full_path);
            }
        }

        None
    }
}

// ============================================================================
// Lookup Plugin Trait
// ============================================================================

/// Trait that all lookup plugins must implement
pub trait LookupPlugin: Send + Sync + fmt::Debug {
    /// Returns the name of the lookup plugin
    fn name(&self) -> &'static str;

    /// Returns a description of what the lookup does
    fn description(&self) -> &'static str;

    /// Execute the lookup with the given terms and options
    ///
    /// # Arguments
    ///
    /// * `terms` - The lookup terms/arguments
    /// * `options` - Options passed to the lookup
    /// * `context` - Execution context with variables, facts, etc.
    ///
    /// # Returns
    ///
    /// A vector of JSON values (one per term, or multiple for list-returning lookups)
    fn lookup(
        &self,
        terms: &[String],
        options: &LookupOptions,
        context: &LookupContext,
    ) -> LookupResult<Vec<serde_json::Value>>;

    /// Validate options before execution
    fn validate_options(&self, _options: &LookupOptions) -> LookupResult<()> {
        Ok(())
    }

    /// Returns example usage for documentation
    fn examples(&self) -> Vec<&'static str> {
        vec![]
    }

    /// Returns a list of available options with descriptions
    fn available_options(&self) -> Vec<LookupOptionInfo> {
        vec![]
    }
}

/// Information about a lookup option
#[derive(Debug, Clone)]
pub struct LookupOptionInfo {
    /// Option name
    pub name: &'static str,
    /// Option description
    pub description: &'static str,
    /// Option type
    pub option_type: &'static str,
    /// Default value as string
    pub default: Option<&'static str>,
    /// Whether the option is required
    pub required: bool,
}

impl LookupOptionInfo {
    /// Create a new option info
    pub fn new(name: &'static str, description: &'static str, option_type: &'static str) -> Self {
        Self {
            name,
            description,
            option_type,
            default: None,
            required: false,
        }
    }

    /// Set default value
    pub fn with_default(mut self, default: &'static str) -> Self {
        self.default = Some(default);
        self
    }

    /// Mark as required
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

// ============================================================================
// Lookup Registry
// ============================================================================

/// Registry for managing lookup plugins
#[derive(Debug, Default)]
pub struct LookupRegistry {
    plugins: HashMap<String, Arc<dyn LookupPlugin>>,
}

impl LookupRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a lookup plugin
    pub fn register<P: LookupPlugin + 'static>(&mut self, plugin: P) {
        let name = plugin.name().to_string();
        self.plugins.insert(name, Arc::new(plugin));
    }

    /// Get a lookup plugin by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn LookupPlugin>> {
        self.plugins.get(name).cloned()
    }

    /// List all registered plugin names
    pub fn list(&self) -> Vec<&str> {
        self.plugins.keys().map(|s| s.as_str()).collect()
    }

    /// Execute a lookup by plugin name
    pub fn lookup(
        &self,
        name: &str,
        terms: &[&str],
        context: &LookupContext,
    ) -> LookupResult<Vec<serde_json::Value>> {
        let plugin = self
            .plugins
            .get(name)
            .ok_or_else(|| LookupError::NotFound(name.to_string()))?;

        let terms: Vec<String> = terms.iter().map(|s| s.to_string()).collect();
        let options = LookupOptions::default();
        plugin.lookup(&terms, &options, context)
    }

    /// Execute a lookup with options
    pub fn lookup_with_options(
        &self,
        name: &str,
        terms: &[&str],
        options: &LookupOptions,
        context: &LookupContext,
    ) -> LookupResult<Vec<serde_json::Value>> {
        let plugin = self
            .plugins
            .get(name)
            .ok_or_else(|| LookupError::NotFound(name.to_string()))?;

        plugin.validate_options(options)?;
        let terms: Vec<String> = terms.iter().map(|s| s.to_string()).collect();
        plugin.lookup(&terms, options, context)
    }
}

// ============================================================================
// Prelude Module
// ============================================================================

/// Convenient re-exports for lookup development and usage.
pub mod prelude {
    pub use super::{
        ErrorBehavior, LookupContext, LookupError, LookupOptionInfo, LookupOptions, LookupPlugin,
        LookupRegistry, LookupResult,
    };
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_options_new() {
        let opts = LookupOptions::new();
        assert!(opts.options.is_empty());
        assert_eq!(opts.errors, ErrorBehavior::Strict);
        assert!(opts.default.is_none());
    }

    #[test]
    fn test_lookup_options_with_option() {
        let opts = LookupOptions::new()
            .with_option("key", "value")
            .with_option("count", 42);

        assert_eq!(opts.get_string("key"), Some("value".to_string()));
        assert_eq!(opts.get_i64("count"), Some(42));
    }

    #[test]
    fn test_lookup_options_get_bool() {
        let opts = LookupOptions::new()
            .with_option("flag1", true)
            .with_option("flag2", "yes")
            .with_option("flag3", "false")
            .with_option("flag4", "invalid");

        assert_eq!(opts.get_bool("flag1"), Some(true));
        assert_eq!(opts.get_bool("flag2"), Some(true));
        assert_eq!(opts.get_bool("flag3"), Some(false));
        assert_eq!(opts.get_bool("flag4"), None);
        assert_eq!(opts.get_bool("nonexistent"), None);
    }

    #[test]
    fn test_lookup_options_get_bool_or() {
        let opts = LookupOptions::new().with_option("flag", true);

        assert!(opts.get_bool_or("flag", false));
        assert!(!opts.get_bool_or("nonexistent", false));
        assert!(opts.get_bool_or("nonexistent", true));
    }

    #[test]
    fn test_lookup_context_new() {
        let ctx = LookupContext::new();
        assert!(ctx.variables.is_empty());
        assert!(ctx.facts.is_empty());
        assert!(ctx.work_dir.is_none());
        assert!(ctx.search_paths.is_empty());
        assert!(!ctx.check_mode);
    }

    #[test]
    fn test_lookup_context_with_variable() {
        let ctx = LookupContext::new()
            .with_variable("name", serde_json::json!("test"))
            .with_variable("count", serde_json::json!(42));

        assert_eq!(ctx.variables.get("name"), Some(&serde_json::json!("test")));
        assert_eq!(ctx.variables.get("count"), Some(&serde_json::json!(42)));
    }

    #[test]
    fn test_lookup_context_resolve_path() {
        let ctx = LookupContext::new();

        // Non-existent file should return None
        assert!(ctx.resolve_path("nonexistent_file_12345.txt").is_none());

        // Existing absolute path should work
        if std::path::Path::new("/etc/passwd").exists() {
            assert!(ctx.resolve_path("/etc/passwd").is_some());
        }
    }

    #[test]
    fn test_error_behavior_display() {
        assert_eq!(ErrorBehavior::Strict.to_string(), "strict");
        assert_eq!(ErrorBehavior::Ignore.to_string(), "ignore");
        assert_eq!(ErrorBehavior::Warn.to_string(), "warn");
    }

    #[test]
    fn test_lookup_option_info() {
        let info = LookupOptionInfo::new("delimiter", "Field delimiter", "string")
            .with_default(",")
            .required();

        assert_eq!(info.name, "delimiter");
        assert_eq!(info.option_type, "string");
        assert_eq!(info.default, Some(","));
        assert!(info.required);
    }

    #[test]
    fn test_lookup_registry_new() {
        let registry = LookupRegistry::new();
        assert!(registry.list().is_empty());
    }
}
