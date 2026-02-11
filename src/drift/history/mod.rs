//! Drift history and timeline explorer
//!
//! This module provides drift snapshot storage, timeline construction,
//! correlation analysis, and export capabilities for tracking how
//! configuration drift evolves over time.

pub mod correlation;
pub mod export;
pub mod store;
pub mod timeline;

pub use correlation::{CorrelationResult, DriftCorrelator, ProbableCause};
pub use export::{ExportFormat, TimelineExporter};
pub use store::{DriftHistoryItem, DriftHistoryStore, DriftSnapshot, DriftTrigger};
pub use timeline::{DriftTimeline, TimelineEntry, TimelineEventType, TimelineFilter};
