//! Plugin System for Rustible
//!
//! This module provides a comprehensive plugin infrastructure for extending Rustible's
//! functionality. Plugins are organized into categories based on their purpose:
//!
//! # Plugin Categories
//!
//! ## Filter Plugins
//!
//! Jinja2-compatible filters for template processing. These filters extend the template
//! engine with operations like regex matching, hashing, encoding, and collection manipulation.
//!
//! See the [`filter`] module for available filters.
//!
//! ## Lookup Plugins
//!
//! Plugins for retrieving data from external sources during playbook execution.
//! Lookups can fetch data from files, environment variables, external commands, and more.
//!
//! See the [`lookup`] module for available lookups.
//!
//! # Usage Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::plugins::filter::FilterRegistry;
//! use rustible::plugins::lookup::prelude::*;
//! use minijinja::Environment;
//!
//! // Register all filters with a template environment
//! let mut env = Environment::new();
//! FilterRegistry::register_all(&mut env);
//!
//! // Use lookup plugins
//! let registry = LookupRegistry::new();
//! let context = LookupContext::default();
//! # Ok(())
//! # }
//! ```
//!
//! # Creating Custom Plugins
//!
//! ## Custom Filter
//!
//! Filters are registered directly with the minijinja environment:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use minijinja::Environment;
//!
//! fn my_custom_filter(value: String) -> String {
//!     value.to_uppercase()
//! }
//!
//! let mut env = Environment::new();
//! env.add_filter("my_filter", my_custom_filter);
//! # Ok(())
//! # }
//! ```
//!
//! ## Custom Lookup
//!
//! Implement the [`lookup::LookupPlugin`] trait:
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::plugins::lookup::prelude::*;
//!
//! #[derive(Debug, Default)]
//! struct MyLookup;
//!
//! impl LookupPlugin for MyLookup {
//!     fn name(&self) -> &'static str { "my_lookup" }
//!     fn description(&self) -> &'static str { "My custom lookup" }
//!     fn lookup(
//!         &self,
//!         terms: &[String],
//!         options: &LookupOptions,
//!         context: &LookupContext,
//!     ) -> LookupResult<Vec<serde_json::Value>> {
//!         // Implementation
//!         Ok(vec![])
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub mod filter;
pub mod lookup;
pub mod provider;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use super::filter::FilterRegistry;
    pub use super::lookup::prelude::*;
    pub use super::provider::{
        ModuleContext, ModuleDescriptor, ModuleOutput, ModuleParams, OutputDescriptor,
        ParameterDescriptor, Provider, ProviderCapability, ProviderDependency, ProviderError,
        ProviderIndex, ProviderIndexEntry, ProviderMetadata, ProviderRegistry,
        ProviderRegistryIndex,
    };
}
