//! Span utilities for structured tracing.
//!
//! This module provides helper functions and extensions for creating
//! well-structured spans for Rustible operations.

use std::collections::HashMap;
use tracing::{info_span, Span};

pub use crate::telemetry::context::SpanKind;

/// Extension trait for spans with additional context.
pub trait SpanExt {
    /// Add a host attribute to the span.
    fn with_host(self, host: &str) -> Self;

    /// Add a task attribute to the span.
    fn with_task(self, task: &str) -> Self;

    /// Add a module attribute to the span.
    fn with_module(self, module: &str) -> Self;

    /// Add an error to the span.
    fn record_error(&self, error: &dyn std::error::Error);

    /// Record success status.
    fn record_ok(&self);

    /// Record changed status.
    fn record_changed(&self);

    /// Record failed status.
    fn record_failed(&self, reason: &str);

    /// Record skipped status.
    fn record_skipped(&self, reason: &str);
}

impl SpanExt for Span {
    fn with_host(self, host: &str) -> Self {
        self.record("host", host);
        self
    }

    fn with_task(self, task: &str) -> Self {
        self.record("task", task);
        self
    }

    fn with_module(self, module: &str) -> Self {
        self.record("module", module);
        self
    }

    fn record_error(&self, error: &dyn std::error::Error) {
        self.record("error", true);
        self.record("error.message", error.to_string().as_str());
    }

    fn record_ok(&self) {
        self.record("status", "ok");
    }

    fn record_changed(&self) {
        self.record("status", "changed");
    }

    fn record_failed(&self, reason: &str) {
        self.record("status", "failed");
        self.record("error", true);
        self.record("error.message", reason);
    }

    fn record_skipped(&self, reason: &str) {
        self.record("status", "skipped");
        self.record("skip.reason", reason);
    }
}

/// Create a span for playbook execution.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::telemetry::spans::create_playbook_span;
/// use tracing::Instrument;
///
/// async fn run_playbook(name: &str) {
///     let span = create_playbook_span(name, Some("/path/to/playbook.yml"));
///
///     async {
///         // Playbook execution logic
///     }
///     .instrument(span)
///     .await;
/// }
/// # Ok(())
/// # }
/// ```
pub fn create_playbook_span(name: &str, path: Option<&str>) -> Span {
    info_span!(
        "playbook",
        otel.name = %format!("playbook:{}", name),
        otel.kind = "internal",
        playbook.name = %name,
        playbook.path = path.unwrap_or(""),
        status = tracing::field::Empty,
        hosts.total = tracing::field::Empty,
        hosts.ok = tracing::field::Empty,
        hosts.changed = tracing::field::Empty,
        hosts.failed = tracing::field::Empty,
        hosts.skipped = tracing::field::Empty,
        tasks.total = tracing::field::Empty,
    )
}

/// Create a span for play execution.
pub fn create_play_span(name: &str, hosts_pattern: &str) -> Span {
    info_span!(
        "play",
        otel.name = %format!("play:{}", name),
        otel.kind = "internal",
        play.name = %name,
        play.hosts = %hosts_pattern,
        status = tracing::field::Empty,
        hosts.matched = tracing::field::Empty,
        tasks.total = tracing::field::Empty,
    )
}

/// Create a span for task execution.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::telemetry::spans::create_task_span;
/// use tracing::Instrument;
///
/// async fn execute_task(name: &str, module: &str, host: &str) {
///     let span = create_task_span(name, module, host);
///     let span_for_record = span.clone();
///
///     async move {
///         // Task execution logic
///         span_for_record.record("status", "ok");
///     }
///     .instrument(span)
///     .await;
/// }
/// # Ok(())
/// # }
/// ```
pub fn create_task_span(name: &str, module: &str, host: &str) -> Span {
    info_span!(
        "task",
        otel.name = %format!("task:{}:{}", module, name),
        otel.kind = "internal",
        task.name = %name,
        module.name = %module,
        host = %host,
        status = tracing::field::Empty,
        changed = tracing::field::Empty,
        error = tracing::field::Empty,
        error.message = tracing::field::Empty,
        skip.reason = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Create a span for module execution.
pub fn create_module_span(module: &str, host: &str) -> Span {
    info_span!(
        "module",
        otel.name = %format!("module:{}", module),
        otel.kind = "internal",
        module.name = %module,
        host = %host,
        status = tracing::field::Empty,
        changed = tracing::field::Empty,
        error = tracing::field::Empty,
        error.message = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Create a span for connection operations.
///
/// # Example
///
/// ```rust,ignore,no_run
/// # #[tokio::main]
/// # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
/// use rustible::prelude::*;
/// use rustible::telemetry::spans::create_connection_span;
/// use tracing::Instrument;
///
/// async fn connect_to_host(host: &str, port: u16) {
///     let span = create_connection_span(host, port, "ssh");
///
///     async {
///         // Connection logic
///     }
///     .instrument(span)
///     .await;
/// }
/// # Ok(())
/// # }
/// ```
pub fn create_connection_span(host: &str, port: u16, connection_type: &str) -> Span {
    info_span!(
        "connection",
        otel.name = %format!("connection:{}:{}", connection_type, host),
        otel.kind = "client",
        net.peer.name = %host,
        net.peer.port = %port,
        connection.type = %connection_type,
        status = tracing::field::Empty,
        error = tracing::field::Empty,
        error.message = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Create a span for SSH command execution.
pub fn create_ssh_command_span(host: &str, command: &str) -> Span {
    // Truncate command for span name
    let cmd_short = if command.len() > 50 {
        format!("{}...", &command[..47])
    } else {
        command.to_string()
    };

    info_span!(
        "ssh_command",
        otel.name = %format!("ssh:command:{}", host),
        otel.kind = "client",
        host = %host,
        command = %cmd_short,
        exit_code = tracing::field::Empty,
        stdout_bytes = tracing::field::Empty,
        stderr_bytes = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Create a span for file transfer operations.
pub fn create_file_transfer_span(
    host: &str,
    local_path: &str,
    remote_path: &str,
    direction: FileTransferDirection,
) -> Span {
    let direction_str = match direction {
        FileTransferDirection::Upload => "upload",
        FileTransferDirection::Download => "download",
    };

    info_span!(
        "file_transfer",
        otel.name = %format!("file:{}:{}", direction_str, host),
        otel.kind = "client",
        host = %host,
        file.local_path = %local_path,
        file.remote_path = %remote_path,
        transfer.direction = %direction_str,
        file.size_bytes = tracing::field::Empty,
        status = tracing::field::Empty,
        error = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Direction of file transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTransferDirection {
    /// Upload from local to remote
    Upload,
    /// Download from remote to local
    Download,
}

/// Create a span for handler execution.
pub fn create_handler_span(name: &str, host: &str) -> Span {
    info_span!(
        "handler",
        otel.name = %format!("handler:{}", name),
        otel.kind = "internal",
        handler.name = %name,
        host = %host,
        status = tracing::field::Empty,
        error = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Create a span for role execution.
pub fn create_role_span(name: &str) -> Span {
    info_span!(
        "role",
        otel.name = %format!("role:{}", name),
        otel.kind = "internal",
        role.name = %name,
        tasks.total = tracing::field::Empty,
        handlers.total = tracing::field::Empty,
        status = tracing::field::Empty,
    )
}

/// Create a span for fact gathering.
pub fn create_gather_facts_span(host: &str) -> Span {
    info_span!(
        "gather_facts",
        otel.name = %format!("facts:{}", host),
        otel.kind = "internal",
        host = %host,
        facts.count = tracing::field::Empty,
        cached = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Create a span for template rendering.
pub fn create_template_span(template_name: &str) -> Span {
    info_span!(
        "template",
        otel.name = %format!("template:{}", template_name),
        otel.kind = "internal",
        template.name = %template_name,
        template.size_bytes = tracing::field::Empty,
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Create a span for inventory loading.
pub fn create_inventory_span(source: &str) -> Span {
    info_span!(
        "inventory",
        otel.name = %format!("inventory:{}", source),
        otel.kind = "internal",
        inventory.source = %source,
        hosts.total = tracing::field::Empty,
        groups.total = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Create a span for vault operations.
pub fn create_vault_span(operation: &str) -> Span {
    info_span!(
        "vault",
        otel.name = %format!("vault:{}", operation),
        otel.kind = "internal",
        vault.operation = %operation,
        status = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

/// Span attributes for Rustible-specific context.
#[derive(Debug, Clone, Default)]
pub struct SpanAttributes {
    /// Key-value pairs for span attributes
    attributes: HashMap<String, SpanAttributeValue>,
}

impl SpanAttributes {
    /// Create new empty span attributes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a string attribute.
    pub fn string(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes
            .insert(key.into(), SpanAttributeValue::String(value.into()));
        self
    }

    /// Add an integer attribute.
    pub fn int(mut self, key: impl Into<String>, value: i64) -> Self {
        self.attributes
            .insert(key.into(), SpanAttributeValue::Int(value));
        self
    }

    /// Add a boolean attribute.
    pub fn bool(mut self, key: impl Into<String>, value: bool) -> Self {
        self.attributes
            .insert(key.into(), SpanAttributeValue::Bool(value));
        self
    }

    /// Add a float attribute.
    pub fn float(mut self, key: impl Into<String>, value: f64) -> Self {
        self.attributes
            .insert(key.into(), SpanAttributeValue::Float(value));
        self
    }

    /// Get the attributes.
    pub fn into_inner(self) -> HashMap<String, SpanAttributeValue> {
        self.attributes
    }
}

/// Value type for span attributes.
#[derive(Debug, Clone)]
pub enum SpanAttributeValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// Record the duration on a span.
pub fn record_duration(span: &Span, duration: std::time::Duration) {
    span.record("duration_ms", duration.as_millis() as i64);
}

/// Record bytes transferred on a span.
pub fn record_bytes(span: &Span, field: &str, bytes: usize) {
    span.record(field, bytes as i64);
}

/// Create a linked span for distributed tracing across hosts.
pub fn create_remote_span(local_span: &Span, remote_host: &str, operation: &str) -> Span {
    info_span!(
        parent: local_span,
        "remote_operation",
        otel.name = %format!("remote:{}:{}", operation, remote_host),
        otel.kind = "client",
        remote.host = %remote_host,
        operation = %operation,
        status = tracing::field::Empty,
        error = tracing::field::Empty,
        duration_ms = tracing::field::Empty,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    fn init_tracing() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        });
    }

    #[test]
    fn test_create_playbook_span() {
        init_tracing();
        let span = create_playbook_span("test-playbook", Some("/path/to/playbook.yml"));
        assert!(!span.is_disabled());
    }

    #[test]
    fn test_create_task_span() {
        init_tracing();
        let span = create_task_span("Install nginx", "apt", "web-server");
        assert!(!span.is_disabled());
    }

    #[test]
    fn test_create_connection_span() {
        init_tracing();
        let span = create_connection_span("192.168.1.1", 22, "ssh");
        assert!(!span.is_disabled());
    }

    #[test]
    fn test_span_attributes() {
        let attrs = SpanAttributes::new()
            .string("host", "server1")
            .int("port", 22)
            .bool("connected", true)
            .float("latency_ms", 1.5);

        let inner = attrs.into_inner();
        assert_eq!(inner.len(), 4);
    }

    #[test]
    fn test_file_transfer_span() {
        init_tracing();
        let span = create_file_transfer_span(
            "server1",
            "/local/file.txt",
            "/remote/file.txt",
            FileTransferDirection::Upload,
        );
        assert!(!span.is_disabled());
    }
}
