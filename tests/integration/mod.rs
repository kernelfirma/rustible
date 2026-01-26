//! Integration test suite for Rustible
//!
//! This module contains comprehensive integration tests that verify
//! the interaction between multiple components of the system:
//!
//! - End-to-end playbook execution
//! - Role execution and dependency resolution
//! - Handler notification and deduplication
//! - Block/rescue/always error handling

pub mod handler_notification_tests;
pub mod playbook_e2e_tests;
pub mod role_execution_tests;
