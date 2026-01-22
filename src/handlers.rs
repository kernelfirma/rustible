//! Handler definitions for Rustible
//!
//! This module provides the handler system which allows tasks to trigger
//! specific actions at the end of a play. Handlers are:
//!
//! - **Notified by tasks**: When a task changes something, it can notify handlers
//! - **Deduplicated**: Each handler runs at most once per play, regardless of
//!   how many times it was notified
//! - **Run at play end**: Handlers execute after all tasks complete
//! - **Chainable**: Handlers can notify other handlers
//!
//! # Listen Directive
//!
//! Handlers can listen to multiple notification names using the `listen` field:
//!
//! ```yaml
//! handlers:
//!   - name: restart web services
//!     listen:
//!       - restart nginx
//!       - restart apache
//!     service:
//!       name: "{{ item }}"
//!       state: restarted
//! ```
//!
//! # Force Handlers
//!
//! By default, handlers are skipped if a play fails. Use `force_handlers: true`
//! on the play to ensure handlers run regardless of failures.

// Re-export the canonical Handler type from executor::task
pub use crate::executor::task::Handler;

/// Extension trait for Handler that provides builder-pattern construction
pub trait HandlerBuilder {
    /// Add an argument to the handler
    fn with_arg(self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self;

    /// Set the when condition
    fn with_when(self, condition: impl Into<String>) -> Self;

    /// Add a listen name (for responding to additional notification names)
    fn with_listen(self, name: impl Into<String>) -> Self;
}

impl HandlerBuilder for Handler {
    fn with_arg(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.args.insert(key.into(), value.into());
        self
    }

    fn with_when(mut self, condition: impl Into<String>) -> Self {
        self.when = Some(condition.into());
        self
    }

    fn with_listen(mut self, name: impl Into<String>) -> Self {
        self.listen.push(name.into());
        self
    }
}

/// Create a new handler with the given name and module
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::handlers::{new_handler, HandlerBuilder};
///
/// let handler = new_handler("restart nginx", "service")
///     .with_arg("name", "nginx")
///     .with_arg("state", "restarted")
///     .with_listen("restart web services");
/// # Ok(())
/// # }
/// ```
pub fn new_handler(name: impl Into<String>, module: impl Into<String>) -> Handler {
    Handler {
        name: name.into(),
        module: module.into(),
        args: indexmap::IndexMap::new(),
        when: None,
        listen: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_handler() {
        let handler = new_handler("restart nginx", "service");
        assert_eq!(handler.name, "restart nginx");
        assert_eq!(handler.module, "service");
        assert!(handler.args.is_empty());
        assert!(handler.when.is_none());
        assert!(handler.listen.is_empty());
    }

    #[test]
    fn test_handler_builder() {
        let handler = new_handler("restart nginx", "service")
            .with_arg("name", "nginx")
            .with_arg("state", "restarted")
            .with_when("ansible_os_family == 'Debian'")
            .with_listen("restart web services")
            .with_listen("restart all");

        assert_eq!(handler.name, "restart nginx");
        assert_eq!(handler.module, "service");
        assert_eq!(handler.args.len(), 2);
        assert_eq!(
            handler.when,
            Some("ansible_os_family == 'Debian'".to_string())
        );
        assert_eq!(handler.listen.len(), 2);
        assert!(handler.listen.contains(&"restart web services".to_string()));
        assert!(handler.listen.contains(&"restart all".to_string()));
    }

    #[test]
    fn test_handler_listen_directive() {
        let handler = new_handler("restart web stack", "debug")
            .with_listen("restart nginx")
            .with_listen("restart apache")
            .with_listen("restart haproxy");

        assert_eq!(handler.listen.len(), 3);
        // Handler should respond to any of these notification names
        assert!(handler.listen.contains(&"restart nginx".to_string()));
        assert!(handler.listen.contains(&"restart apache".to_string()));
        assert!(handler.listen.contains(&"restart haproxy".to_string()));
    }
}
