//! Audit sink pipeline for fan-out delivery
//!
//! Sinks are output destinations for audit entries. A `SinkPipeline` fans out
//! each entry to multiple sinks, enabling simultaneous delivery to files, HTTP
//! endpoints, syslog, and other backends.

use super::hashchain::HashChainEntry;
use async_trait::async_trait;
use std::path::PathBuf;
use thiserror::Error;

/// Errors from audit sink operations.
#[derive(Error, Debug)]
pub enum SinkError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("HTTP sink error: {0}")]
    Http(String),

    #[error("syslog sink error: {0}")]
    Syslog(String),

    #[error("pipeline error: {failures} of {total} sinks failed")]
    Pipeline { failures: usize, total: usize },
}

/// Result type for sink operations.
pub type SinkResult<T> = std::result::Result<T, SinkError>;

/// Trait for audit entry output destinations.
#[async_trait]
pub trait AuditSink: Send + Sync {
    /// Send a hash-chain entry to this sink.
    async fn send(&self, entry: &HashChainEntry) -> SinkResult<()>;

    /// Human-readable name for this sink (used in error messages).
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// FileSink
// ---------------------------------------------------------------------------

/// Sink that appends JSON-lines entries to a local file.
#[derive(Debug, Clone)]
pub struct FileSink {
    path: PathBuf,
}

impl FileSink {
    /// Create a file sink targeting the given path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[async_trait]
impl AuditSink for FileSink {
    async fn send(&self, entry: &HashChainEntry) -> SinkResult<()> {
        use std::fs::OpenOptions;
        use std::io::Write;

        let mut line = serde_json::to_string(entry)?;
        line.push('\n');

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    fn name(&self) -> &str {
        "file"
    }
}

// ---------------------------------------------------------------------------
// HttpSink (stub)
// ---------------------------------------------------------------------------

/// Stub sink for forwarding audit entries to an HTTP endpoint.
///
/// A real implementation would use `reqwest` to POST entries. This stub
/// simply records that a send was attempted.
#[derive(Debug, Clone)]
pub struct HttpSink {
    endpoint: String,
}

impl HttpSink {
    /// Create a new HTTP sink pointing at the given URL.
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
        }
    }

    /// Get the configured endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[async_trait]
impl AuditSink for HttpSink {
    async fn send(&self, _entry: &HashChainEntry) -> SinkResult<()> {
        // Stub: in production this would POST the entry as JSON.
        tracing::debug!(endpoint = %self.endpoint, "HttpSink send (stub)");
        Ok(())
    }

    fn name(&self) -> &str {
        "http"
    }
}

// ---------------------------------------------------------------------------
// SyslogSink (stub)
// ---------------------------------------------------------------------------

/// Stub sink for forwarding audit entries to the system syslog.
#[derive(Debug, Clone)]
pub struct SyslogSink {
    facility: String,
}

impl SyslogSink {
    /// Create a syslog sink with the given facility name.
    pub fn new(facility: impl Into<String>) -> Self {
        Self {
            facility: facility.into(),
        }
    }
}

#[async_trait]
impl AuditSink for SyslogSink {
    async fn send(&self, _entry: &HashChainEntry) -> SinkResult<()> {
        // Stub: in production this would write to syslog.
        tracing::debug!(facility = %self.facility, "SyslogSink send (stub)");
        Ok(())
    }

    fn name(&self) -> &str {
        "syslog"
    }
}

// ---------------------------------------------------------------------------
// SinkPipeline
// ---------------------------------------------------------------------------

/// Pipeline that fans out audit entries to multiple sinks.
///
/// By default the pipeline is **fail-open**: if some sinks fail the entry is
/// still considered delivered as long as at least one succeeds. Set
/// `require_all` to `true` to require every sink to succeed.
pub struct SinkPipeline {
    sinks: Vec<Box<dyn AuditSink>>,
    require_all: bool,
}

impl SinkPipeline {
    /// Create an empty pipeline.
    pub fn new() -> Self {
        Self {
            sinks: Vec::new(),
            require_all: false,
        }
    }

    /// Add a sink to the pipeline.
    pub fn add_sink(&mut self, sink: Box<dyn AuditSink>) {
        self.sinks.push(sink);
    }

    /// Set whether all sinks must succeed for `send` to return `Ok`.
    pub fn set_require_all(&mut self, require_all: bool) {
        self.require_all = require_all;
    }

    /// Send an entry to all sinks. Returns an error only when the failure
    /// policy is violated.
    pub async fn send(&self, entry: &HashChainEntry) -> SinkResult<()> {
        let total = self.sinks.len();
        let mut failures = 0usize;

        for sink in &self.sinks {
            if let Err(e) = sink.send(entry).await {
                tracing::warn!(sink = sink.name(), error = %e, "audit sink delivery failed");
                failures += 1;
            }
        }

        if self.require_all && failures > 0 {
            return Err(SinkError::Pipeline { failures, total });
        }

        // Fail-open: as long as not *all* sinks failed we are OK.
        if failures == total && total > 0 {
            return Err(SinkError::Pipeline { failures, total });
        }

        Ok(())
    }

    /// Get the number of configured sinks.
    pub fn sink_count(&self) -> usize {
        self.sinks.len()
    }
}

impl Default for SinkPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_file_sink_writes_entry() {
        let tmp = NamedTempFile::new().unwrap();
        let sink = FileSink::new(tmp.path());

        let entry = HashChainEntry {
            sequence: 0,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_hash: "abc123".to_string(),
            previous_hash: String::new(),
            chain_hash: "def456".to_string(),
        };

        sink.send(&entry).await.unwrap();

        let content = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(content.contains("abc123"));
        assert!(content.contains("def456"));
    }

    #[tokio::test]
    async fn test_pipeline_fan_out() {
        let tmp1 = NamedTempFile::new().unwrap();
        let tmp2 = NamedTempFile::new().unwrap();

        let mut pipeline = SinkPipeline::new();
        pipeline.add_sink(Box::new(FileSink::new(tmp1.path())));
        pipeline.add_sink(Box::new(FileSink::new(tmp2.path())));

        let entry = HashChainEntry {
            sequence: 0,
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            event_hash: "aaa".to_string(),
            previous_hash: String::new(),
            chain_hash: "bbb".to_string(),
        };

        pipeline.send(&entry).await.unwrap();

        let c1 = std::fs::read_to_string(tmp1.path()).unwrap();
        let c2 = std::fs::read_to_string(tmp2.path()).unwrap();
        assert!(c1.contains("aaa"));
        assert!(c2.contains("aaa"));
    }
}
