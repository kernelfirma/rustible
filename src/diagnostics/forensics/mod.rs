//! Incident Forensics Bundle Export
//!
//! This module provides tools for collecting, redacting, and exporting
//! diagnostic data into a portable forensics bundle for incident analysis.
//!
//! ## Components
//!
//! - **Schema**: Defines the manifest and metadata structures for forensics bundles
//! - **Collector**: Gathers audit events, state snapshots, drift reports, and system info
//! - **Redaction**: Strips sensitive data (passwords, tokens, keys) before export
//! - **Bundle**: Serializes collected data to JSON and verifies bundle integrity

pub mod bundle;
pub mod collector;
pub mod redaction;
pub mod schema;

pub use bundle::ForensicsBundle;
pub use collector::{BundleData, CollectorConfig, ForensicsCollector, SystemInfo};
pub use redaction::{ContentType, RedactionPattern, RedactionRule, Redactor};
pub use schema::{BundleContents, ForensicsBundleManifest, TimeRange};
