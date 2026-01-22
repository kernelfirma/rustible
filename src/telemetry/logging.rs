//! Structured logging layer using the tracing crate.
//!
//! This module provides a configurable logging layer that supports
//! multiple output formats (pretty, compact, JSON) and destinations.

use crate::telemetry::config::{LogFormat, LogLevel, LoggingConfig};
use std::path::Path;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Builder for constructing a logging layer.
pub struct LoggingBuilder {
    config: LoggingConfig,
}

impl LoggingBuilder {
    /// Create a new logging builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: LoggingConfig::default(),
        }
    }

    /// Create a builder from an existing configuration.
    pub fn from_config(config: LoggingConfig) -> Self {
        Self { config }
    }

    /// Set the log level.
    pub fn with_level(mut self, level: LogLevel) -> Self {
        self.config.level = level;
        self
    }

    /// Set the log format.
    pub fn with_format(mut self, format: LogFormat) -> Self {
        self.config.format = format;
        self
    }

    /// Set ANSI colors.
    pub fn with_ansi(mut self, enabled: bool) -> Self {
        self.config.ansi_colors = enabled;
        self
    }

    /// Include span information.
    pub fn with_spans(mut self, enabled: bool) -> Self {
        self.config.with_spans = enabled;
        self
    }

    /// Include target in logs.
    pub fn with_target(mut self, enabled: bool) -> Self {
        self.config.with_target = enabled;
        self
    }

    /// Include file/line information.
    pub fn with_file(mut self, enabled: bool) -> Self {
        self.config.with_file = enabled;
        self
    }

    /// Set filter directive.
    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.config.filter = Some(filter.into());
        self
    }

    /// Set log file path.
    pub fn with_file_output(mut self, path: impl AsRef<Path>) -> Self {
        self.config.file = Some(path.as_ref().to_path_buf());
        self
    }

    /// Build and initialize the logging layer (global subscriber).
    pub fn init(self) -> crate::error::Result<()> {
        let env_filter = self.build_filter();

        match self.config.format {
            LogFormat::Pretty => self.init_pretty(env_filter),
            LogFormat::Compact => self.init_compact(env_filter),
            LogFormat::Json => self.init_json(env_filter),
            LogFormat::Full => self.init_full(env_filter),
        }
    }

    /// Build a logging layer that can be composed with other layers.
    pub fn build_layer<S>(self) -> Box<dyn Layer<S> + Send + Sync + 'static>
    where
        S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
    {
        let env_filter = self.build_filter();

        match self.config.format {
            LogFormat::Pretty => self.build_pretty_layer(env_filter),
            LogFormat::Compact => self.build_compact_layer(env_filter),
            LogFormat::Json => self.build_json_layer(env_filter),
            LogFormat::Full => self.build_full_layer(env_filter),
        }
    }

    fn build_filter(&self) -> EnvFilter {
        let default_filter = match self.config.level {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        };

        if let Some(ref filter) = self.config.filter {
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new(filter))
                .unwrap_or_else(|_| EnvFilter::new(default_filter))
        } else {
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter))
        }
    }

    fn init_pretty(self, env_filter: EnvFilter) -> crate::error::Result<()> {
        let layer = tracing_subscriber::fmt::layer()
            .pretty()
            .with_ansi(self.config.ansi_colors)
            .with_target(self.config.with_target)
            .with_file(self.config.with_file)
            .with_line_number(self.config.with_file)
            .with_thread_ids(self.config.with_thread_ids)
            .with_thread_names(self.config.with_thread_names)
            .with_span_events(if self.config.with_spans {
                FmtSpan::NEW | FmtSpan::CLOSE
            } else {
                FmtSpan::NONE
            });

        tracing_subscriber::registry()
            .with(env_filter)
            .with(layer)
            .try_init()
            .map_err(|e| crate::error::Error::Config(e.to_string()))
    }

    fn init_compact(self, env_filter: EnvFilter) -> crate::error::Result<()> {
        let layer = tracing_subscriber::fmt::layer()
            .compact()
            .with_ansi(self.config.ansi_colors)
            .with_target(self.config.with_target)
            .with_file(self.config.with_file)
            .with_line_number(self.config.with_file)
            .with_thread_ids(self.config.with_thread_ids)
            .with_thread_names(self.config.with_thread_names)
            .with_span_events(if self.config.with_spans {
                FmtSpan::NEW | FmtSpan::CLOSE
            } else {
                FmtSpan::NONE
            });

        tracing_subscriber::registry()
            .with(env_filter)
            .with(layer)
            .try_init()
            .map_err(|e| crate::error::Error::Config(e.to_string()))
    }

    fn init_json(self, env_filter: EnvFilter) -> crate::error::Result<()> {
        let layer = tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(self.config.with_spans)
            .with_span_list(self.config.with_spans)
            .with_file(self.config.with_file)
            .with_line_number(self.config.with_file)
            .with_thread_ids(self.config.with_thread_ids)
            .with_thread_names(self.config.with_thread_names)
            .with_span_events(if self.config.with_spans {
                FmtSpan::NEW | FmtSpan::CLOSE
            } else {
                FmtSpan::NONE
            });

        tracing_subscriber::registry()
            .with(env_filter)
            .with(layer)
            .try_init()
            .map_err(|e| crate::error::Error::Config(e.to_string()))
    }

    fn init_full(self, env_filter: EnvFilter) -> crate::error::Result<()> {
        let layer = tracing_subscriber::fmt::layer()
            .with_ansi(self.config.ansi_colors)
            .with_target(self.config.with_target)
            .with_file(self.config.with_file)
            .with_line_number(self.config.with_file)
            .with_thread_ids(self.config.with_thread_ids)
            .with_thread_names(self.config.with_thread_names)
            .with_span_events(FmtSpan::FULL);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(layer)
            .try_init()
            .map_err(|e| crate::error::Error::Config(e.to_string()))
    }

    fn build_pretty_layer<S>(self, env_filter: EnvFilter) -> Box<dyn Layer<S> + Send + Sync>
    where
        S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
    {
        let layer = tracing_subscriber::fmt::layer()
            .pretty()
            .with_ansi(self.config.ansi_colors)
            .with_target(self.config.with_target)
            .with_file(self.config.with_file)
            .with_line_number(self.config.with_file)
            .with_thread_ids(self.config.with_thread_ids)
            .with_thread_names(self.config.with_thread_names)
            .with_span_events(if self.config.with_spans {
                FmtSpan::NEW | FmtSpan::CLOSE
            } else {
                FmtSpan::NONE
            })
            .with_filter(env_filter);

        Box::new(layer)
    }

    fn build_compact_layer<S>(self, env_filter: EnvFilter) -> Box<dyn Layer<S> + Send + Sync>
    where
        S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
    {
        let layer = tracing_subscriber::fmt::layer()
            .compact()
            .with_ansi(self.config.ansi_colors)
            .with_target(self.config.with_target)
            .with_file(self.config.with_file)
            .with_line_number(self.config.with_file)
            .with_thread_ids(self.config.with_thread_ids)
            .with_thread_names(self.config.with_thread_names)
            .with_span_events(if self.config.with_spans {
                FmtSpan::NEW | FmtSpan::CLOSE
            } else {
                FmtSpan::NONE
            })
            .with_filter(env_filter);

        Box::new(layer)
    }

    fn build_json_layer<S>(self, env_filter: EnvFilter) -> Box<dyn Layer<S> + Send + Sync>
    where
        S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
    {
        let layer = tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(self.config.with_spans)
            .with_span_list(self.config.with_spans)
            .with_file(self.config.with_file)
            .with_line_number(self.config.with_file)
            .with_thread_ids(self.config.with_thread_ids)
            .with_thread_names(self.config.with_thread_names)
            .with_span_events(if self.config.with_spans {
                FmtSpan::NEW | FmtSpan::CLOSE
            } else {
                FmtSpan::NONE
            })
            .with_filter(env_filter);

        Box::new(layer)
    }

    fn build_full_layer<S>(self, env_filter: EnvFilter) -> Box<dyn Layer<S> + Send + Sync>
    where
        S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
    {
        let layer = tracing_subscriber::fmt::layer()
            .with_ansi(self.config.ansi_colors)
            .with_target(self.config.with_target)
            .with_file(self.config.with_file)
            .with_line_number(self.config.with_file)
            .with_thread_ids(self.config.with_thread_ids)
            .with_thread_names(self.config.with_thread_names)
            .with_span_events(FmtSpan::FULL)
            .with_filter(env_filter);

        Box::new(layer)
    }
}

impl Default for LoggingBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A composable logging layer for use with tracing subscriber.
pub struct LoggingLayer {
    config: LoggingConfig,
}

impl LoggingLayer {
    /// Create a new logging layer with the given configuration.
    pub fn new(config: LoggingConfig) -> Self {
        Self { config }
    }

    /// Get the logging configuration.
    pub fn config(&self) -> &LoggingConfig {
        &self.config
    }
}

/// Structured log event for JSON output.
#[derive(Debug, serde::Serialize)]
pub struct StructuredLogEvent<'a> {
    /// Timestamp in RFC 3339 format
    pub timestamp: String,

    /// Log level
    pub level: &'a str,

    /// Log message
    pub message: &'a str,

    /// Target module
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<&'a str>,

    /// Source file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<&'a str>,

    /// Line number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,

    /// Span name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<&'a str>,

    /// Trace ID for distributed tracing correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,

    /// Span ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,

    /// Additional fields
    #[serde(flatten)]
    pub fields: serde_json::Value,
}

/// Helper function to initialize logging with verbosity level.
pub fn init_from_verbosity(verbosity: u8) -> crate::error::Result<()> {
    let level = LogLevel::from_verbosity(verbosity);
    let config = LoggingConfig {
        level,
        format: if verbosity >= 3 {
            LogFormat::Full
        } else {
            LogFormat::Pretty
        },
        with_file: verbosity >= 3,
        with_target: verbosity >= 2,
        ..Default::default()
    };

    LoggingBuilder::from_config(config).init()
}

/// Helper function to initialize JSON logging for production.
pub fn init_json_logging() -> crate::error::Result<()> {
    LoggingBuilder::from_config(LoggingConfig::production()).init()
}

/// Helper function to initialize development logging.
pub fn init_dev_logging() -> crate::error::Result<()> {
    LoggingBuilder::from_config(LoggingConfig::development()).init()
}

/// Log event macros with structured fields.
#[macro_export]
macro_rules! log_event {
    ($level:ident, $msg:expr, $($key:ident = $value:expr),* $(,)?) => {
        tracing::$level!(
            message = $msg,
            $($key = %$value,)*
        )
    };
}

/// Log a playbook execution event.
#[macro_export]
macro_rules! log_playbook {
    ($level:ident, playbook = $playbook:expr, $msg:expr $(, $($key:ident = $value:expr),*)?) => {
        tracing::$level!(
            playbook = %$playbook,
            message = $msg,
            $($($key = %$value,)*)?
        )
    };
}

/// Log a task execution event.
#[macro_export]
macro_rules! log_task {
    ($level:ident, task = $task:expr, host = $host:expr, $msg:expr $(, $($key:ident = $value:expr),*)?) => {
        tracing::$level!(
            task = %$task,
            host = %$host,
            message = $msg,
            $($($key = %$value,)*)?
        )
    };
}

/// Log a connection event.
#[macro_export]
macro_rules! log_connection {
    ($level:ident, host = $host:expr, $msg:expr $(, $($key:ident = $value:expr),*)?) => {
        tracing::$level!(
            host = %$host,
            message = $msg,
            $($($key = %$value,)*)?
        )
    };
}

/// Log a module execution event.
#[macro_export]
macro_rules! log_module {
    ($level:ident, module = $module:expr, $msg:expr $(, $($key:ident = $value:expr),*)?) => {
        tracing::$level!(
            module = %$module,
            message = $msg,
            $($($key = %$value,)*)?
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::Registry;

    #[test]
    fn test_logging_builder() {
        let builder = LoggingBuilder::new()
            .with_level(LogLevel::Debug)
            .with_format(LogFormat::Json)
            .with_ansi(false)
            .with_target(true);

        assert_eq!(builder.config.level, LogLevel::Debug);
        assert_eq!(builder.config.format, LogFormat::Json);
        assert!(!builder.config.ansi_colors);
        assert!(builder.config.with_target);
    }

    #[test]
    fn test_log_level_from_verbosity() {
        assert_eq!(LogLevel::from_verbosity(0), LogLevel::Warn);
        assert_eq!(LogLevel::from_verbosity(1), LogLevel::Info);
        assert_eq!(LogLevel::from_verbosity(2), LogLevel::Debug);
        assert_eq!(LogLevel::from_verbosity(3), LogLevel::Trace);
    }

    #[test]
    fn test_build_layer_for_formats() {
        let _pretty = LoggingBuilder::new()
            .with_format(LogFormat::Pretty)
            .build_layer::<Registry>();
        let _compact = LoggingBuilder::new()
            .with_format(LogFormat::Compact)
            .build_layer::<Registry>();
        let _json = LoggingBuilder::new()
            .with_format(LogFormat::Json)
            .build_layer::<Registry>();
        let _full = LoggingBuilder::new()
            .with_format(LogFormat::Full)
            .build_layer::<Registry>();
    }

    #[test]
    fn test_logging_layer_config_access() {
        let config = LoggingConfig::default();
        let layer = LoggingLayer::new(config.clone());
        assert_eq!(layer.config().level, config.level);
        assert_eq!(layer.config().format, config.format);
    }
}
