//! Items Lookup Plugin
//!
//! Returns a list of items for iteration. This is the lookup equivalent of
//! Ansible's `with_items` loop construct, providing a passthrough mechanism
//! for list iteration in playbook expressions.
//!
//! # Usage
//!
//! ```yaml
//! # Iterate over a list of items
//! - name: Install packages
//!   apt:
//!     name: "{{ item }}"
//!   with_items:
//!     - nginx
//!     - curl
//!     - git
//! ```
//!
//! The items lookup simply returns each argument as a separate item in the
//! result list, enabling `with_items` style loops.

use super::{Lookup, LookupContext, LookupError, LookupResult};

/// Items lookup plugin for list iteration support
#[derive(Debug, Clone, Default)]
pub struct ItemsLookup;

impl ItemsLookup {
    /// Create a new ItemsLookup instance
    pub fn new() -> Self {
        Self
    }
}

impl Lookup for ItemsLookup {
    fn name(&self) -> &'static str {
        "items"
    }

    fn description(&self) -> &'static str {
        "Returns a list of items for iteration (with_items support)"
    }

    fn lookup(&self, args: &[&str], _context: &LookupContext) -> LookupResult<Vec<String>> {
        if args.is_empty() {
            return Err(LookupError::MissingArgument(
                "at least one item is required".to_string(),
            ));
        }

        // Each argument becomes an item in the result list
        // This supports both simple values and comma-separated lists
        let mut results = Vec::new();
        for arg in args {
            // Support comma-separated values within a single argument
            if arg.contains(',') {
                for item in arg.split(',') {
                    let trimmed = item.trim();
                    if !trimmed.is_empty() {
                        results.push(trimmed.to_string());
                    }
                }
            } else {
                results.push(arg.to_string());
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_items_lookup_name() {
        let lookup = ItemsLookup::new();
        assert_eq!(lookup.name(), "items");
    }

    #[test]
    fn test_items_lookup_single_item() {
        let lookup = ItemsLookup::new();
        let context = LookupContext::default();
        let result = lookup.lookup(&["hello"], &context).unwrap();
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn test_items_lookup_multiple_items() {
        let lookup = ItemsLookup::new();
        let context = LookupContext::default();
        let result = lookup.lookup(&["nginx", "curl", "git"], &context).unwrap();
        assert_eq!(result, vec!["nginx", "curl", "git"]);
    }

    #[test]
    fn test_items_lookup_comma_separated() {
        let lookup = ItemsLookup::new();
        let context = LookupContext::default();
        let result = lookup.lookup(&["a, b, c"], &context).unwrap();
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_items_lookup_mixed() {
        let lookup = ItemsLookup::new();
        let context = LookupContext::default();
        let result = lookup.lookup(&["single", "a,b"], &context).unwrap();
        assert_eq!(result, vec!["single", "a", "b"]);
    }

    #[test]
    fn test_items_lookup_empty_args() {
        let lookup = ItemsLookup::new();
        let context = LookupContext::default();
        let result = lookup.lookup(&[], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    #[test]
    fn test_items_lookup_preserves_order() {
        let lookup = ItemsLookup::new();
        let context = LookupContext::default();
        let result = lookup.lookup(&["z", "a", "m", "b"], &context).unwrap();
        assert_eq!(result, vec!["z", "a", "m", "b"]);
    }
}
