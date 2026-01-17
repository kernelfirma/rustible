//! Jinja2-compatible filter plugins for Rustible.
//!
//! This module provides a comprehensive set of filters that match Ansible's Jinja2 filter
//! functionality. Filters are organized into categories:
//!
//! - **regex**: Regular expression operations (search, replace, findall, escape)
//! - **serialization**: JSON/YAML encoding and decoding
//! - **hash**: Cryptographic hashing and checksums
//! - **encoding**: Base64 encoding/decoding
//! - **collections**: Set operations (combine, union, difference, intersect)
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::plugins::filter::FilterRegistry;
//! use minijinja::Environment;
//!
//! let mut env = Environment::new();
//! FilterRegistry::register_all(&mut env);
//! # Ok(())
//! # }
//! ```
//!
//! # Ansible Compatibility
//!
//! All filters are designed to be compatible with Ansible's Jinja2 filters.
//! See individual modules for detailed compatibility notes.

pub mod collections;
pub mod encoding;
pub mod hash;
pub mod regex;
pub mod serialization;

use minijinja::Environment;

/// Registry for managing and registering filter plugins.
///
/// This struct provides methods to register all available filters
/// with a minijinja Environment.
pub struct FilterRegistry;

impl FilterRegistry {
    /// Register all available filters with the given environment.
    ///
    /// This is the recommended way to add all Rustible filters to your
    /// template environment in one call.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::prelude::*;
    /// use minijinja::Environment;
    /// use rustible::plugins::filter::FilterRegistry;
    ///
    /// let mut env = Environment::new();
    /// FilterRegistry::register_all(&mut env);
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_all(env: &mut Environment<'static>) {
        regex::register_filters(env);
        serialization::register_filters(env);
        hash::register_filters(env);
        encoding::register_filters(env);
        collections::register_filters(env);
    }

    /// Register only regex filters.
    pub fn register_regex(env: &mut Environment<'static>) {
        regex::register_filters(env);
    }

    /// Register only serialization filters (JSON/YAML).
    pub fn register_serialization(env: &mut Environment<'static>) {
        serialization::register_filters(env);
    }

    /// Register only hash/checksum filters.
    pub fn register_hash(env: &mut Environment<'static>) {
        hash::register_filters(env);
    }

    /// Register only encoding filters (Base64).
    pub fn register_encoding(env: &mut Environment<'static>) {
        encoding::register_filters(env);
    }

    /// Register only collection filters (combine, union, difference, etc.).
    pub fn register_collections(env: &mut Environment<'static>) {
        collections::register_filters(env);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::Value;

    fn create_env_with_all_filters() -> Environment<'static> {
        let mut env = Environment::new();
        FilterRegistry::register_all(&mut env);
        env
    }

    #[test]
    fn test_register_all_filters() {
        let mut env = create_env_with_all_filters();

        // Test that regex filters are registered
        env.add_template("regex_test", "{{ 'hello123' | regex_search('[0-9]+') }}")
            .unwrap();

        // Test that hash filters are registered
        env.add_template("hash_test", "{{ 'test' | hash('sha256') }}")
            .unwrap();

        // Test that encoding filters are registered
        env.add_template("b64_test", "{{ 'hello' | b64encode }}")
            .unwrap();

        // Test that serialization filters are registered
        env.add_template("json_test", "{{ {'key': 'value'} | to_json }}")
            .unwrap();
    }

    #[test]
    fn test_selective_registration() {
        let mut env = Environment::new();
        FilterRegistry::register_hash(&mut env);

        // Hash filter should work
        env.add_template("hash_test", "{{ 'test' | hash('md5') }}")
            .unwrap();

        // Regex filter should not be registered (will fail at runtime if used)
        // We just verify hash works independently
        let tmpl = env.get_template("hash_test").unwrap();
        let result = tmpl.render(Value::UNDEFINED).unwrap();
        assert!(!result.is_empty());
    }
}
