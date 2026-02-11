//! Warewulf 4 migration support.
//!
//! This module provides importers for Warewulf 4 node profiles, mapping them
//! to Rustible inventory hosts and groups.

pub mod profile;

pub use profile::{
    ImportedGroup, ImportedHost, ProfileImportResult, WarewulfIpmi, WarewulfKernel,
    WarewulfNetDev, WarewulfNode, WarewulfProfileImporter,
};
