//! Telemetry module for Rustible observability.
//!
//! This module provides comprehensive observability features including:
//! - **OpenTelemetry integration**: Distributed tracing with context propagation
//! - **Structured logging**: Using the `tracing` crate with JSON and pretty formats
//! - **Metrics export**: Prometheus-compatible metrics endpoint
//! - **Distributed tracing**: Multi-host trace correlation
//!
//! # Architecture
//!
//! ```text
//! +------------------+     +------------------+     +------------------+
//! |   Application    |---->|    Telemetry     |---->|    Exporters     |
//! |   (Rustible)     |     |    Layer         |     |  (OTLP/Prom/Log) |
//! +------------------+     +------------------+     +------------------+
//!         |                        |                        |
//!         v                        v                        v
//! +------------------+     +------------------+     +------------------+
//! |   Span Context   |     |  Trace/Metrics   |     |   Jaeger/Zipkin  |
//! |   Propagation    |     |   Collection     |     |   Prometheus     |
//! +------------------+     +------------------+     +------------------+
//! ```
//!
//! # Quick Start
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::telemetry::{TelemetryConfig, TelemetryBuilder};
//! # let host = "localhost";
//!
//! // Initialize telemetry with default settings
//! let telemetry = TelemetryBuilder::new()
//!     .with_service_name("rustible")
//!     .with_otlp_endpoint("http://localhost:4317")
//!     .with_prometheus_port(9090)
//!     .build()?;
//!
//! // Use tracing macros for instrumentation
//! tracing::info!(host = %host, "Connecting to target");
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! - `telemetry-otlp`: Enable OpenTelemetry OTLP exporter
//! - `telemetry-jaeger`: Enable Jaeger exporter
//! - `telemetry-prometheus`: Enable Prometheus metrics endpoint

pub mod config;
pub mod context;
pub mod logging;
pub mod metrics;
pub mod spans;
pub mod tracing_layer;

// Re-exports for convenience
pub use config::{LogFormat, MetricsConfig, TelemetryConfig, TracingConfig};
pub use context::{SpanContext, TraceContext, TraceContextPropagator};
pub use logging::{LoggingBuilder, LoggingLayer};
pub use metrics::{
    Counter, Gauge, Histogram, MetricsExporter, MetricsRecorder, MetricsRegistry,
    PrometheusExporter,
};
pub use spans::{
    create_connection_span, create_module_span, create_playbook_span, create_task_span, SpanExt,
    SpanKind,
};
pub use tracing_layer::{TelemetryBuilder, TelemetryGuard, TelemetryLayer};

use std::sync::OnceLock;

/// Global telemetry instance for the application.
static TELEMETRY: OnceLock<TelemetryGuard> = OnceLock::new();

/// Initialize global telemetry with the given configuration.
///
/// This should be called once at application startup. Subsequent calls
/// will be ignored.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::telemetry::{init_telemetry, TelemetryConfig};
///
/// let config = TelemetryConfig::default();
/// init_telemetry(config)?;
/// # Ok(())
/// # }
/// ```
pub fn init_telemetry(config: TelemetryConfig) -> crate::error::Result<()> {
    let guard = TelemetryBuilder::from_config(config).build()?;
    TELEMETRY
        .set(guard)
        .map_err(|_| crate::error::Error::Config("Telemetry already initialized".to_string()))
}

/// Get the global telemetry guard if initialized.
pub fn get_telemetry() -> Option<&'static TelemetryGuard> {
    TELEMETRY.get()
}

/// Shutdown telemetry gracefully, flushing any pending spans/metrics.
pub fn shutdown_telemetry() {
    if let Some(guard) = TELEMETRY.get() {
        guard.shutdown();
    }
}

/// Prelude module for convenient imports.
pub mod prelude {
    pub use super::config::{LogFormat, MetricsConfig, TelemetryConfig, TracingConfig};
    pub use super::context::{SpanContext, TraceContext};
    pub use super::metrics::{Counter, Gauge, Histogram, MetricsRecorder};
    pub use super::spans::{
        create_connection_span, create_module_span, create_playbook_span, create_task_span,
        SpanExt, SpanKind,
    };
    pub use super::{init_telemetry, shutdown_telemetry};
    pub use tracing::{debug, error, info, instrument, trace, warn, Instrument, Span};
}
