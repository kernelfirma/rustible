//! Integration tests for Rustible modules
//!
//! This file includes the integration test modules from the tests/modules/ directory.
//! These tests verify module behavior including parameter validation, metadata,
//! and (where possible) execution against real or mock environments.

mod modules;

// Re-export system module tests
pub use modules::cron_tests;
pub use modules::facts_tests;
pub use modules::hostname_tests;
pub use modules::mount_tests;
pub use modules::sysctl_tests;

// Re-export network module tests
pub use modules::network_tests;

// Re-export container/K8s module tests
pub use modules::container_tests;
