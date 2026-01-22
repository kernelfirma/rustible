//! Null callback plugin for Rustible.
//!
//! This plugin produces absolutely no output - a true no-op callback.
//! Ideal for scripting scenarios where output is captured elsewhere,
//! or when combining with file-based callbacks for logging.
//!
//! # Features
//!
//! - **Zero output**: All callback methods are no-ops
//! - **Fastest callback**: No I/O, no locking, no allocations
//! - **Composable**: Combine with file-based callbacks for logging
//! - **Scripting-friendly**: Perfect for programmatic usage
//!
//! # Use Cases
//!
//! - Script automation where only exit codes matter
//! - Testing scenarios requiring silent execution
//! - Paired with file-based callbacks for background logging
//! - Benchmark runs requiring minimal overhead
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::NullCallback;
//!
//! // Silent execution - no console output
//! let callback = NullCallback;
//! # let _ = ();
//! # Ok(())
//! # }
//! ```
//!
//! # Performance
//!
//! The `NullCallback` is a zero-sized type (ZST) with all methods
//! being inline no-ops. The compiler will optimize away all callback
//! invocations entirely, resulting in zero runtime overhead.

use async_trait::async_trait;

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

/// Null callback plugin that suppresses all output.
///
/// This is the fastest possible callback implementation - a zero-sized
/// type with no-op methods that the compiler can completely inline
/// and eliminate.
///
/// # Design
///
/// - Zero-sized type (no memory footprint)
/// - All async methods return immediately
/// - No allocations, no I/O, no locking
/// - Fully `Send + Sync` with no interior mutability
///
/// # Usage
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::callback::prelude::*;
/// use rustible::callback::{JsonCallback, NullCallback};
///
/// // For silent execution
/// let _callback = NullCallback;
///
/// // Combine with file-based callback for logging without console output
/// let file_callback = JsonCallback::builder()
///     .output_file("execution.json")
///     .build();
/// let callbacks: Vec<Box<dyn ExecutionCallback>> = vec![
///     Box::new(NullCallback),
///     Box::new(file_callback),
/// ];
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NullCallback;

impl NullCallback {
    /// Creates a new null callback.
    ///
    /// This is equivalent to `NullCallback` or `NullCallback::default()`,
    /// provided for API consistency with other callbacks.
    ///
    /// # Example
    ///
    /// ```rust,ignore,no_run
    /// # #[tokio::main]
    /// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    /// use rustible::callback::prelude::*;
    /// let callback = NullCallback::new();
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ExecutionCallback for NullCallback {
    /// No-op: Does not produce any output.
    #[inline]
    async fn on_playbook_start(&self, _name: &str) {
        // Intentionally empty - null callback produces no output
    }

    /// No-op: Does not produce any output.
    #[inline]
    async fn on_playbook_end(&self, _name: &str, _success: bool) {
        // Intentionally empty - null callback produces no output
    }

    /// No-op: Does not produce any output.
    #[inline]
    async fn on_play_start(&self, _name: &str, _hosts: &[String]) {
        // Intentionally empty - null callback produces no output
    }

    /// No-op: Does not produce any output.
    #[inline]
    async fn on_play_end(&self, _name: &str, _success: bool) {
        // Intentionally empty - null callback produces no output
    }

    /// No-op: Does not produce any output.
    #[inline]
    async fn on_task_start(&self, _name: &str, _host: &str) {
        // Intentionally empty - null callback produces no output
    }

    /// No-op: Does not produce any output.
    #[inline]
    async fn on_task_complete(&self, _result: &ExecutionResult) {
        // Intentionally empty - null callback produces no output
    }

    /// No-op: Does not produce any output.
    #[inline]
    async fn on_handler_triggered(&self, _name: &str) {
        // Intentionally empty - null callback produces no output
    }

    /// No-op: Does not produce any output.
    #[inline]
    async fn on_facts_gathered(&self, _host: &str, _facts: &Facts) {
        // Intentionally empty - null callback produces no output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::ModuleResult;
    use std::time::Duration;

    fn create_test_result() -> ExecutionResult {
        ExecutionResult {
            host: "test-host".to_string(),
            task_name: "test-task".to_string(),
            result: ModuleResult {
                success: true,
                changed: false,
                message: "ok".to_string(),
                skipped: false,
                data: None,
                warnings: Vec::new(),
            },
            duration: Duration::from_millis(100),
            notify: Vec::new(),
        }
    }

    #[test]
    fn test_null_callback_is_zst() {
        // Verify NullCallback is a zero-sized type
        assert_eq!(std::mem::size_of::<NullCallback>(), 0);
    }

    #[test]
    fn test_null_callback_new() {
        let callback = NullCallback::new();
        assert_eq!(callback, NullCallback);
    }

    #[test]
    fn test_null_callback_default() {
        let callback = NullCallback::default();
        assert_eq!(callback, NullCallback);
    }

    #[test]
    fn test_null_callback_clone() {
        let callback1 = NullCallback;
        let callback2 = callback1;
        assert_eq!(callback1, callback2);
    }

    #[test]
    fn test_null_callback_copy() {
        let callback1 = NullCallback;
        let callback2 = callback1;
        // Both are valid after copy (not move)
        let _ = callback1;
        let _ = callback2;
    }

    #[tokio::test]
    async fn test_null_callback_playbook_lifecycle() {
        let callback = NullCallback;

        // All these should complete without any side effects
        callback.on_playbook_start("test-playbook").await;
        callback
            .on_play_start("test-play", &["host1".to_string(), "host2".to_string()])
            .await;
        callback.on_task_start("test-task", "host1").await;
        callback.on_task_complete(&create_test_result()).await;
        callback.on_handler_triggered("test-handler").await;
        callback
            .on_facts_gathered("host1", &crate::facts::Facts::new())
            .await;
        callback.on_play_end("test-play", true).await;
        callback.on_playbook_end("test-playbook", true).await;
    }

    #[tokio::test]
    async fn test_null_callback_produces_no_output() {
        // This test verifies that NullCallback doesn't panic or produce errors
        // In a real test environment, you might capture stdout to verify silence
        let callback = NullCallback;

        for _ in 0..1000 {
            callback.on_task_start("task", "host").await;
            callback.on_task_complete(&create_test_result()).await;
        }
        // If we get here without panics, the test passes
    }

    #[test]
    fn test_null_callback_debug() {
        let callback = NullCallback;
        let debug_str = format!("{:?}", callback);
        assert_eq!(debug_str, "NullCallback");
    }

    #[test]
    fn test_null_callback_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(NullCallback);
        assert!(set.contains(&NullCallback));
    }
}
