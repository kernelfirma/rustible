//! Reusable policy pack framework.
//!
//! Policy packs are collections of related policy rules that can be
//! discovered, loaded, and evaluated against playbook data. Each pack
//! is described by a [`PolicyPackManifest`] and contains a set of
//! [`PackRule`] checks that are run during evaluation.

pub mod builtins;
pub mod loader;
pub mod manifest;
pub mod registry;

pub use loader::{PackLoader, PackRule, PolicyPack, RuleCheck};
pub use manifest::{PackCategory, PackParameter, PolicyPackManifest};
pub use registry::{PackEvaluationResult, PackRegistry};
