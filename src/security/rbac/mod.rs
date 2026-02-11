//! Role-Based Access Control (RBAC) for Rustible
//!
//! Provides enterprise-grade authorization with role hierarchies,
//! resource pattern matching, and deny-takes-precedence semantics.

pub mod engine;
pub mod model;
pub mod store;

pub use engine::RbacEngine;
pub use model::{Action, AuthzDecision, AuthzRequest, Effect, Permission, ResourcePattern, Role};
pub use store::RbacConfig;
