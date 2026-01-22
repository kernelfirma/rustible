//! Environment Variable Lookup Plugin
//!
//! Reads environment variables. Similar to Ansible's `env` lookup plugin.
//!
//! # Usage
//!
//! ```yaml
//! # Read a single environment variable
//! home: "{{ lookup('env', 'HOME') }}"
//!
//! # Read multiple environment variables
//! vars: "{{ lookup('env', 'HOME', 'USER', 'SHELL') }}"
//!
//! # With default value
//! value: "{{ lookup('env', 'MY_VAR', default='fallback') }}"
//! ```
//!
//! # Options
//!
//! - `default` (string): Default value if environment variable is not set

use super::{Lookup, LookupContext, LookupError, LookupResult};
use std::env;

/// Environment variable lookup plugin
#[derive(Debug, Clone, Default)]
pub struct EnvLookup;

impl EnvLookup {
    /// Create a new EnvLookup instance
    pub fn new() -> Self {
        Self
    }

    /// Validate that an environment variable name is safe
    fn validate_var_name(&self, name: &str) -> LookupResult<()> {
        if name.is_empty() {
            return Err(LookupError::InvalidArguments(
                "Environment variable name cannot be empty".to_string(),
            ));
        }

        // Check for null bytes
        if name.contains('\0') {
            return Err(LookupError::InvalidArguments(
                "Environment variable name contains null byte".to_string(),
            ));
        }

        // Check for invalid characters (only alphanumeric and underscore allowed)
        for c in name.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' {
                return Err(LookupError::InvalidArguments(format!(
                    "Invalid character '{}' in environment variable name '{}'",
                    c, name
                )));
            }
        }

        // Check that name doesn't start with a digit
        if name
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            return Err(LookupError::InvalidArguments(format!(
                "Environment variable name '{}' cannot start with a digit",
                name
            )));
        }

        Ok(())
    }
}

impl Lookup for EnvLookup {
    fn name(&self) -> &'static str {
        "env"
    }

    fn description(&self) -> &'static str {
        "Reads environment variables"
    }

    fn lookup(&self, args: &[&str], context: &LookupContext) -> LookupResult<Vec<String>> {
        if args.is_empty() {
            return Err(LookupError::MissingArgument(
                "environment variable name required".to_string(),
            ));
        }

        // Parse options
        let options = self.parse_options(args);
        let default_value = options.get("default").cloned();

        let mut results = Vec::new();

        // Process each non-option argument as an environment variable name
        for arg in args {
            // Skip option arguments
            if arg.contains('=') {
                continue;
            }

            // Validate the variable name
            self.validate_var_name(arg)?;

            // Get the environment variable
            match env::var(arg) {
                Ok(value) => {
                    results.push(value);
                }
                Err(_) => {
                    if let Some(ref default) = default_value {
                        results.push(default.clone());
                    } else if context.fail_on_error {
                        return Err(LookupError::EnvNotFound(arg.to_string()));
                    } else if let Some(ref fallback) = context.default_value {
                        results.push(fallback.clone());
                    } else {
                        // Return empty string for missing env vars when not failing
                        results.push(String::new());
                    }
                }
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_lookup_home() {
        let lookup = EnvLookup::new();
        let context = LookupContext::default();

        // HOME should be set on most Unix systems
        let result = lookup.lookup(&["HOME"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert!(!values[0].is_empty());
    }

    #[test]
    fn test_env_lookup_path() {
        let lookup = EnvLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["PATH"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert!(!values[0].is_empty());
    }

    #[test]
    fn test_env_lookup_multiple() {
        let lookup = EnvLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["HOME", "PATH"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_env_lookup_not_found() {
        let lookup = EnvLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["RUSTIBLE_TEST_NONEXISTENT_VAR_12345"], &context);
        assert!(matches!(result, Err(LookupError::EnvNotFound(_))));
    }

    #[test]
    fn test_env_lookup_with_default() {
        let lookup = EnvLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(
            &[
                "RUSTIBLE_TEST_NONEXISTENT_VAR_12345",
                "default=fallback_value",
            ],
            &context,
        );
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], "fallback_value");
    }

    #[test]
    fn test_env_lookup_not_found_no_fail() {
        let lookup = EnvLookup::new();
        let context = LookupContext::new().with_fail_on_error(false);

        let result = lookup.lookup(&["RUSTIBLE_TEST_NONEXISTENT_VAR_12345"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert!(values[0].is_empty());
    }

    #[test]
    fn test_env_lookup_missing_name() {
        let lookup = EnvLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    #[test]
    fn test_env_lookup_invalid_name() {
        let lookup = EnvLookup::new();
        let context = LookupContext::default();

        // Name with invalid characters
        let result = lookup.lookup(&["INVALID-NAME"], &context);
        assert!(matches!(result, Err(LookupError::InvalidArguments(_))));

        // Empty name
        let result = lookup.lookup(&[""], &context);
        assert!(matches!(result, Err(LookupError::InvalidArguments(_))));

        // Name starting with digit
        let result = lookup.lookup(&["1INVALID"], &context);
        assert!(matches!(result, Err(LookupError::InvalidArguments(_))));
    }

    #[test]
    fn test_env_lookup_underscore_name() {
        // Set a test environment variable
        std::env::set_var("RUSTIBLE_TEST_VAR_WITH_UNDERSCORE", "test_value");

        let lookup = EnvLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["RUSTIBLE_TEST_VAR_WITH_UNDERSCORE"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values[0], "test_value");

        // Clean up
        std::env::remove_var("RUSTIBLE_TEST_VAR_WITH_UNDERSCORE");
    }
}
