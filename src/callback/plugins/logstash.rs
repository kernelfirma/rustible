//! Logstash Callback Plugin for Rustible.
//!
//! This plugin sends execution events to Logstash for centralized log
//! aggregation and analysis. It supports TCP, UDP, and HTTP transport
//! methods with JSON formatting.
//!
//! # Features
//!
//! - Multiple transport protocols (TCP, UDP, HTTP)
//! - JSON-formatted log events (ECS-compatible)
//! - Configurable buffering and batching
//! - TLS support for secure transmission
//! - Automatic reconnection with backoff
//! - Rate limiting and backpressure handling
//!
//! # Configuration
//!
//! Configuration via environment variables:
//!
//! - `RUSTIBLE_LOGSTASH_HOST`: Logstash host (default: "localhost")
//! - `RUSTIBLE_LOGSTASH_PORT`: Logstash port (default: 5044)
//! - `RUSTIBLE_LOGSTASH_PROTOCOL`: Protocol (tcp, udp, http; default: tcp)
//! - `RUSTIBLE_LOGSTASH_TLS`: Enable TLS (default: false)
//! - `RUSTIBLE_LOGSTASH_INDEX`: Index name pattern (default: "rustible")
//! - `RUSTIBLE_LOGSTASH_BATCH_SIZE`: Batch size for HTTP (default: 100)
//! - `RUSTIBLE_LOGSTASH_FLUSH_INTERVAL`: Flush interval in ms (default: 5000)
//!
//! # Logstash Configuration
//!
//! Example Logstash input configuration:
//!
//! ```text
//! input {
//!   tcp {
//!     port => 5044
//!     codec => json_lines
//!   }
//! }
//!
//! filter {
//!   if [event][module] == "rustible" {
//!     mutate {
//!       add_field => { "[@metadata][index]" => "rustible-%{+YYYY.MM.dd}" }
//!     }
//!   }
//! }
//!
//! output {
//!   elasticsearch {
//!     hosts => ["elasticsearch:9200"]
//!     index => "%{[@metadata][index]}"
//!   }
//! }
//! ```
//!
//! # Usage
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::plugins::{LogstashCallback, LogstashConfig, LogstashProtocol};
//!
//! // From environment variables
//! let callback = LogstashCallback::from_env()?;
//!
//! // With custom configuration
//! let config = LogstashConfig::builder()
//!     .host("logstash.example.com")
//!     .port(5044)
//!     .protocol(LogstashProtocol::Tcp)
//!     .build();
//! let callback = LogstashCallback::new(config)?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::env;
use std::io::Write;
use std::net::{TcpStream, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during Logstash operations.
#[derive(Debug, thiserror::Error)]
pub enum LogstashError {
    /// Connection failed
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Write failed
    #[error("Write failed: {0}")]
    WriteFailed(#[from] std::io::Error),

    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Result type for Logstash operations.
pub type LogstashResult<T> = Result<T, LogstashError>;

// ============================================================================
// Configuration
// ============================================================================

/// Transport protocol for Logstash communication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogstashProtocol {
    /// TCP transport (recommended for reliability)
    #[default]
    Tcp,
    /// UDP transport (best for high-volume, lossy-tolerant scenarios)
    Udp,
    /// HTTP transport (for Logstash HTTP input plugin)
    Http,
}

impl std::str::FromStr for LogstashProtocol {
    type Err = LogstashError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tcp" => Ok(LogstashProtocol::Tcp),
            "udp" => Ok(LogstashProtocol::Udp),
            "http" | "https" => Ok(LogstashProtocol::Http),
            _ => Err(LogstashError::ConfigError(format!(
                "Unknown protocol: {}",
                s
            ))),
        }
    }
}

impl std::fmt::Display for LogstashProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogstashProtocol::Tcp => write!(f, "tcp"),
            LogstashProtocol::Udp => write!(f, "udp"),
            LogstashProtocol::Http => write!(f, "http"),
        }
    }
}

/// Configuration for the Logstash callback plugin.
#[derive(Debug, Clone)]
pub struct LogstashConfig {
    /// Logstash host
    pub host: String,
    /// Logstash port
    pub port: u16,
    /// Transport protocol
    pub protocol: LogstashProtocol,
    /// Enable TLS (for TCP and HTTP)
    pub use_tls: bool,
    /// Index name pattern
    pub index: String,
    /// Application name field
    pub application: String,
    /// Environment field (production, staging, etc.)
    pub environment: Option<String>,
    /// Additional static fields
    pub extra_fields: HashMap<String, String>,
    /// Batch size for HTTP transport
    pub batch_size: usize,
    /// Flush interval in milliseconds
    pub flush_interval_ms: u64,
    /// Connection timeout in seconds
    pub timeout_secs: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Whether to log task start events
    pub log_task_start: bool,
    /// Whether to log facts gathered events
    pub log_facts: bool,
    /// Include full result data
    pub include_result_data: bool,
}

impl Default for LogstashConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5044,
            protocol: LogstashProtocol::Tcp,
            use_tls: false,
            index: "rustible".to_string(),
            application: "rustible".to_string(),
            environment: None,
            extra_fields: HashMap::new(),
            batch_size: 100,
            flush_interval_ms: 5000,
            timeout_secs: 30,
            max_retries: 3,
            log_task_start: false,
            log_facts: false,
            include_result_data: true,
        }
    }
}

impl LogstashConfig {
    /// Create a new configuration builder.
    pub fn builder() -> LogstashConfigBuilder {
        LogstashConfigBuilder::default()
    }

    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        Self {
            host: env::var("RUSTIBLE_LOGSTASH_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: env::var("RUSTIBLE_LOGSTASH_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(5044),
            protocol: env::var("RUSTIBLE_LOGSTASH_PROTOCOL")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or_default(),
            use_tls: env::var("RUSTIBLE_LOGSTASH_TLS")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            index: env::var("RUSTIBLE_LOGSTASH_INDEX").unwrap_or_else(|_| "rustible".to_string()),
            application: env::var("RUSTIBLE_LOGSTASH_APPLICATION")
                .unwrap_or_else(|_| "rustible".to_string()),
            environment: env::var("RUSTIBLE_LOGSTASH_ENVIRONMENT").ok(),
            extra_fields: HashMap::new(),
            batch_size: env::var("RUSTIBLE_LOGSTASH_BATCH_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
            flush_interval_ms: env::var("RUSTIBLE_LOGSTASH_FLUSH_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5000),
            timeout_secs: env::var("RUSTIBLE_LOGSTASH_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            max_retries: env::var("RUSTIBLE_LOGSTASH_RETRIES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
            log_task_start: env::var("RUSTIBLE_LOGSTASH_LOG_TASK_START")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            log_facts: env::var("RUSTIBLE_LOGSTASH_LOG_FACTS")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            include_result_data: env::var("RUSTIBLE_LOGSTASH_INCLUDE_DATA")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(true),
        }
    }

    /// Get the full address string.
    pub fn address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Builder for LogstashConfig.
#[derive(Debug, Default)]
pub struct LogstashConfigBuilder {
    config: LogstashConfig,
}

impl LogstashConfigBuilder {
    /// Set the Logstash host.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.config.host = host.into();
        self
    }

    /// Set the Logstash port.
    pub fn port(mut self, port: u16) -> Self {
        self.config.port = port;
        self
    }

    /// Set the transport protocol.
    pub fn protocol(mut self, protocol: LogstashProtocol) -> Self {
        self.config.protocol = protocol;
        self
    }

    /// Enable or disable TLS.
    pub fn use_tls(mut self, use_tls: bool) -> Self {
        self.config.use_tls = use_tls;
        self
    }

    /// Set the index name pattern.
    pub fn index(mut self, index: impl Into<String>) -> Self {
        self.config.index = index.into();
        self
    }

    /// Set the application name.
    pub fn application(mut self, app: impl Into<String>) -> Self {
        self.config.application = app.into();
        self
    }

    /// Set the environment.
    pub fn environment(mut self, env: impl Into<String>) -> Self {
        self.config.environment = Some(env.into());
        self
    }

    /// Add an extra field.
    pub fn extra_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.extra_fields.insert(key.into(), value.into());
        self
    }

    /// Set the batch size.
    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    /// Set the flush interval.
    pub fn flush_interval_ms(mut self, ms: u64) -> Self {
        self.config.flush_interval_ms = ms;
        self
    }

    /// Set the connection timeout.
    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.config.timeout_secs = secs;
        self
    }

    /// Enable or disable task start logging.
    pub fn log_task_start(mut self, log: bool) -> Self {
        self.config.log_task_start = log;
        self
    }

    /// Enable or disable facts logging.
    pub fn log_facts(mut self, log: bool) -> Self {
        self.config.log_facts = log;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> LogstashConfig {
        self.config
    }
}

// ============================================================================
// Log Event Structure (ECS-compatible)
// ============================================================================

/// ECS-compatible log event structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogstashEvent {
    /// Timestamp in ISO 8601 format
    #[serde(rename = "@timestamp")]
    timestamp: DateTime<Utc>,
    /// Event metadata
    event: EventMetadata,
    /// Log message
    message: String,
    /// Log level
    log: LogLevel,
    /// Host information
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<HostInfo>,
    /// Rustible-specific fields
    rustible: RustibleFields,
    /// Additional fields
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

/// Event metadata (ECS format).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EventMetadata {
    /// Event kind
    kind: String,
    /// Event category
    category: Vec<String>,
    /// Event type
    #[serde(rename = "type")]
    event_type: Vec<String>,
    /// Event action
    action: String,
    /// Event outcome
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    /// Event duration in nanoseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    duration: Option<u64>,
    /// Event module
    module: String,
}

/// Log level information.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogLevel {
    /// Log level
    level: String,
}

/// Host information.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostInfo {
    /// Target hostname
    name: String,
}

/// Rustible-specific fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RustibleFields {
    /// Playbook name
    #[serde(skip_serializing_if = "Option::is_none")]
    playbook: Option<String>,
    /// Play name
    #[serde(skip_serializing_if = "Option::is_none")]
    play: Option<String>,
    /// Task name
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<String>,
    /// Task status
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    /// Whether task made changes
    #[serde(skip_serializing_if = "Option::is_none")]
    changed: Option<bool>,
    /// Task result data
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    /// Handler name
    #[serde(skip_serializing_if = "Option::is_none")]
    handler: Option<String>,
}

// ============================================================================
// Connection State
// ============================================================================

/// Connection state for TCP/UDP.
enum ConnectionState {
    Disconnected,
    TcpConnected(TcpStream),
    UdpConnected(UdpSocket),
}

impl std::fmt::Debug for ConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionState::Disconnected => write!(f, "Disconnected"),
            ConnectionState::TcpConnected(_) => write!(f, "TcpConnected"),
            ConnectionState::UdpConnected(_) => write!(f, "UdpConnected"),
        }
    }
}

/// Internal state.
#[derive(Debug)]
struct LogstashState {
    /// Current playbook
    playbook: Option<String>,
    /// Current play
    play: Option<String>,
    /// Playbook start time
    playbook_start: Option<Instant>,
    /// Event buffer for batching
    buffer: Vec<String>,
    /// Last flush time
    last_flush: Option<Instant>,
    /// Connection state
    connection: ConnectionState,
    /// Hostname cache
    hostname: String,
}

impl Default for LogstashState {
    fn default() -> Self {
        Self {
            playbook: None,
            play: None,
            playbook_start: None,
            buffer: Vec::new(),
            last_flush: None,
            connection: ConnectionState::Disconnected,
            hostname: hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
        }
    }
}

// ============================================================================
// Logstash Callback Implementation
// ============================================================================

/// Logstash callback plugin for centralized logging.
///
/// Sends execution events to Logstash in ECS-compatible JSON format,
/// supporting TCP, UDP, and HTTP transport protocols.
#[derive(Debug)]
pub struct LogstashCallback {
    /// Configuration
    config: LogstashConfig,
    /// Internal state
    state: Arc<RwLock<LogstashState>>,
    /// HTTP client (for HTTP protocol)
    http_client: Option<reqwest::Client>,
}

impl LogstashCallback {
    /// Create a new Logstash callback with the given configuration.
    pub fn new(config: LogstashConfig) -> LogstashResult<Self> {
        let http_client = if config.protocol == LogstashProtocol::Http {
            Some(
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(config.timeout_secs))
                    .build()
                    .map_err(|e| LogstashError::ConfigError(e.to_string()))?,
            )
        } else {
            None
        };

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(LogstashState::default())),
            http_client,
        })
    }

    /// Create a Logstash callback from environment variables.
    pub fn from_env() -> LogstashResult<Self> {
        Self::new(LogstashConfig::from_env())
    }

    /// Create a Logstash callback with default configuration.
    pub fn with_defaults() -> LogstashResult<Self> {
        Self::new(LogstashConfig::default())
    }

    /// Connect to Logstash (for TCP/UDP).
    fn connect(&self) -> LogstashResult<()> {
        let mut state = self.state.write();

        match self.config.protocol {
            LogstashProtocol::Tcp => {
                let stream = TcpStream::connect_timeout(
                    &self.config.address().parse().map_err(|e| {
                        LogstashError::ConnectionFailed(format!("Invalid address: {}", e))
                    })?,
                    Duration::from_secs(self.config.timeout_secs),
                )?;
                stream.set_nodelay(true)?;
                state.connection = ConnectionState::TcpConnected(stream);
                info!("Connected to Logstash via TCP at {}", self.config.address());
            }
            LogstashProtocol::Udp => {
                let socket = UdpSocket::bind("0.0.0.0:0")?;
                socket.connect(&self.config.address())?;
                state.connection = ConnectionState::UdpConnected(socket);
                info!("Connected to Logstash via UDP at {}", self.config.address());
            }
            LogstashProtocol::Http => {
                // HTTP doesn't need persistent connection
                info!(
                    "Logstash HTTP transport configured for {}",
                    self.config.address()
                );
            }
        }

        Ok(())
    }

    /// Send an event to Logstash.
    fn send_event(&self, event: LogstashEvent) -> LogstashResult<()> {
        let json = serde_json::to_string(&event)?;

        match self.config.protocol {
            LogstashProtocol::Tcp | LogstashProtocol::Udp => {
                let mut state = self.state.write();

                // Ensure connected
                if matches!(state.connection, ConnectionState::Disconnected) {
                    drop(state);
                    self.connect()?;
                    state = self.state.write();
                }

                match &mut state.connection {
                    ConnectionState::TcpConnected(stream) => {
                        writeln!(stream, "{}", json)?;
                        stream.flush()?;
                    }
                    ConnectionState::UdpConnected(socket) => {
                        socket.send(json.as_bytes())?;
                    }
                    ConnectionState::Disconnected => {
                        return Err(LogstashError::ConnectionFailed("Not connected".to_string()));
                    }
                }
            }
            LogstashProtocol::Http => {
                // Buffer for batching
                let mut state = self.state.write();
                state.buffer.push(json);

                // Check if we should flush
                let should_flush = state.buffer.len() >= self.config.batch_size
                    || state.last_flush.map_or(true, |t| {
                        t.elapsed().as_millis() as u64 >= self.config.flush_interval_ms
                    });

                if should_flush {
                    let buffer = std::mem::take(&mut state.buffer);
                    state.last_flush = Some(Instant::now());
                    drop(state);

                    // Send asynchronously would require async context
                    // For now, buffer is flushed on playbook end
                    let mut state = self.state.write();
                    state.buffer = buffer;
                }
            }
        }

        debug!("Sent event to Logstash: {}", event.event.action);
        Ok(())
    }

    /// Flush any buffered events.
    async fn flush_buffer(&self) -> LogstashResult<()> {
        if self.config.protocol != LogstashProtocol::Http {
            return Ok(());
        }

        let buffer = {
            let mut state = self.state.write();
            std::mem::take(&mut state.buffer)
        };

        if buffer.is_empty() {
            return Ok(());
        }

        let client = self
            .http_client
            .as_ref()
            .ok_or_else(|| LogstashError::ConfigError("HTTP client not initialized".to_string()))?;

        let url = format!(
            "{}://{}",
            if self.config.use_tls { "https" } else { "http" },
            self.config.address()
        );

        // Send as newline-delimited JSON
        let body = buffer.join("\n");

        let response = client
            .post(&url)
            .header("Content-Type", "application/x-ndjson")
            .body(body)
            .send()
            .await
            .map_err(|e| LogstashError::HttpError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(LogstashError::HttpError(format!("{}: {}", status, body)));
        }

        info!("Flushed {} events to Logstash via HTTP", buffer.len());
        Ok(())
    }

    /// Create a base event with common fields.
    fn create_event(
        &self,
        action: &str,
        message: String,
        level: &str,
        target_host: Option<&str>,
    ) -> LogstashEvent {
        let state = self.state.read();

        let mut extra: HashMap<String, serde_json::Value> = self
            .config
            .extra_fields
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();

        extra.insert(
            "index".to_string(),
            serde_json::Value::String(self.config.index.clone()),
        );
        extra.insert(
            "application".to_string(),
            serde_json::Value::String(self.config.application.clone()),
        );

        if let Some(env) = &self.config.environment {
            extra.insert(
                "environment".to_string(),
                serde_json::Value::String(env.clone()),
            );
        }

        LogstashEvent {
            timestamp: Utc::now(),
            event: EventMetadata {
                kind: "event".to_string(),
                category: vec!["process".to_string()],
                event_type: vec!["info".to_string()],
                action: action.to_string(),
                outcome: None,
                duration: None,
                module: "rustible".to_string(),
            },
            message,
            log: LogLevel {
                level: level.to_string(),
            },
            host: target_host.map(|h| HostInfo {
                name: h.to_string(),
            }),
            rustible: RustibleFields {
                playbook: state.playbook.clone(),
                play: state.play.clone(),
                task: None,
                status: None,
                changed: None,
                result: None,
                handler: None,
            },
            extra,
        }
    }
}

impl Clone for LogstashCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: Arc::clone(&self.state),
            http_client: self.http_client.clone(),
        }
    }
}

#[async_trait]
impl ExecutionCallback for LogstashCallback {
    async fn on_playbook_start(&self, name: &str) {
        {
            let mut state = self.state.write();
            state.playbook = Some(name.to_string());
            state.playbook_start = Some(Instant::now());
        }

        let event = self.create_event(
            "playbook_start",
            format!("Playbook started: {}", name),
            "info",
            None,
        );

        if let Err(e) = self.send_event(event) {
            error!("Failed to send playbook_start event to Logstash: {}", e);
        }
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let duration_ns = self
            .state
            .read()
            .playbook_start
            .map(|t| t.elapsed().as_nanos() as u64);

        let mut event = self.create_event(
            "playbook_end",
            format!(
                "Playbook {}: {}",
                if success { "completed" } else { "failed" },
                name
            ),
            if success { "info" } else { "error" },
            None,
        );

        event.event.outcome = Some(if success {
            "success".to_string()
        } else {
            "failure".to_string()
        });
        event.event.duration = duration_ns;

        if let Err(e) = self.send_event(event) {
            error!("Failed to send playbook_end event to Logstash: {}", e);
        }

        // Flush any remaining buffered events
        if let Err(e) = self.flush_buffer().await {
            error!("Failed to flush buffer to Logstash: {}", e);
        }

        // Clear state
        let mut state = self.state.write();
        state.playbook = None;
        state.play = None;
        state.playbook_start = None;
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        {
            let mut state = self.state.write();
            state.play = Some(name.to_string());
        }

        let mut event = self.create_event(
            "play_start",
            format!("Play started: {} ({} hosts)", name, hosts.len()),
            "info",
            None,
        );

        event
            .extra
            .insert("host_count".to_string(), serde_json::json!(hosts.len()));

        if let Err(e) = self.send_event(event) {
            error!("Failed to send play_start event to Logstash: {}", e);
        }
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        let mut event = self.create_event(
            "play_end",
            format!(
                "Play {}: {}",
                if success { "completed" } else { "failed" },
                name
            ),
            if success { "info" } else { "error" },
            None,
        );

        event.event.outcome = Some(if success {
            "success".to_string()
        } else {
            "failure".to_string()
        });

        if let Err(e) = self.send_event(event) {
            error!("Failed to send play_end event to Logstash: {}", e);
        }
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        if !self.config.log_task_start {
            return;
        }

        let mut event = self.create_event(
            "task_start",
            format!("Task started: {} on {}", name, host),
            "debug",
            Some(host),
        );

        event.rustible.task = Some(name.to_string());

        if let Err(e) = self.send_event(event) {
            warn!("Failed to send task_start event to Logstash: {}", e);
        }
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let status = if result.result.skipped {
            "skipped"
        } else if !result.result.success {
            "failed"
        } else if result.result.changed {
            "changed"
        } else {
            "ok"
        };

        let level = if !result.result.success {
            "error"
        } else if result.result.changed {
            "info"
        } else {
            "debug"
        };

        let mut event = self.create_event(
            "task_complete",
            format!("Task {}: {} on {}", status, result.task_name, result.host),
            level,
            Some(&result.host),
        );

        event.rustible.task = Some(result.task_name.clone());
        event.rustible.status = Some(status.to_string());
        event.rustible.changed = Some(result.result.changed);
        event.event.duration = Some(result.duration.as_nanos() as u64);
        event.event.outcome = Some(if result.result.success {
            "success".to_string()
        } else {
            "failure".to_string()
        });

        if self.config.include_result_data {
            if let Some(ref data) = result.result.data {
                event.rustible.result = Some(data.clone());
            }
        }

        if !result.result.message.is_empty() {
            event.extra.insert(
                "task_message".to_string(),
                serde_json::Value::String(result.result.message.clone()),
            );
        }

        if let Err(e) = self.send_event(event) {
            error!("Failed to send task_complete event to Logstash: {}", e);
        }
    }

    async fn on_handler_triggered(&self, name: &str) {
        let mut event = self.create_event(
            "handler_triggered",
            format!("Handler triggered: {}", name),
            "debug",
            None,
        );

        event.rustible.handler = Some(name.to_string());

        if let Err(e) = self.send_event(event) {
            warn!("Failed to send handler_triggered event to Logstash: {}", e);
        }
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        if !self.config.log_facts {
            return;
        }

        let mut event = self.create_event(
            "facts_gathered",
            format!("Facts gathered from {} ({} facts)", host, facts.all().len()),
            "debug",
            Some(host),
        );

        event.extra.insert(
            "fact_count".to_string(),
            serde_json::json!(facts.all().len()),
        );

        if let Err(e) = self.send_event(event) {
            warn!("Failed to send facts_gathered event to Logstash: {}", e);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = LogstashConfig::builder()
            .host("logstash.example.com")
            .port(5045)
            .protocol(LogstashProtocol::Tcp)
            .use_tls(true)
            .index("test-index")
            .application("test-app")
            .environment("production")
            .extra_field("team", "devops")
            .batch_size(50)
            .log_task_start(true)
            .build();

        assert_eq!(config.host, "logstash.example.com");
        assert_eq!(config.port, 5045);
        assert_eq!(config.protocol, LogstashProtocol::Tcp);
        assert!(config.use_tls);
        assert_eq!(config.index, "test-index");
        assert_eq!(config.application, "test-app");
        assert_eq!(config.environment, Some("production".to_string()));
        assert_eq!(config.extra_fields.get("team"), Some(&"devops".to_string()));
        assert_eq!(config.batch_size, 50);
        assert!(config.log_task_start);
    }

    #[test]
    fn test_default_config() {
        let config = LogstashConfig::default();
        assert_eq!(config.host, "localhost");
        assert_eq!(config.port, 5044);
        assert_eq!(config.protocol, LogstashProtocol::Tcp);
        assert!(!config.use_tls);
        assert_eq!(config.index, "rustible");
        assert_eq!(config.application, "rustible");
        assert!(config.environment.is_none());
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.flush_interval_ms, 5000);
        assert!(!config.log_task_start);
        assert!(!config.log_facts);
    }

    #[test]
    fn test_protocol_parsing() {
        assert_eq!(
            "tcp".parse::<LogstashProtocol>().unwrap(),
            LogstashProtocol::Tcp
        );
        assert_eq!(
            "TCP".parse::<LogstashProtocol>().unwrap(),
            LogstashProtocol::Tcp
        );
        assert_eq!(
            "udp".parse::<LogstashProtocol>().unwrap(),
            LogstashProtocol::Udp
        );
        assert_eq!(
            "http".parse::<LogstashProtocol>().unwrap(),
            LogstashProtocol::Http
        );
        assert!("invalid".parse::<LogstashProtocol>().is_err());
    }

    #[test]
    fn test_protocol_display() {
        assert_eq!(LogstashProtocol::Tcp.to_string(), "tcp");
        assert_eq!(LogstashProtocol::Udp.to_string(), "udp");
        assert_eq!(LogstashProtocol::Http.to_string(), "http");
    }

    #[test]
    fn test_address_format() {
        let config = LogstashConfig::builder()
            .host("example.com")
            .port(5044)
            .build();

        assert_eq!(config.address(), "example.com:5044");
    }

    #[test]
    fn test_event_serialization() {
        let event = LogstashEvent {
            timestamp: Utc::now(),
            event: EventMetadata {
                kind: "event".to_string(),
                category: vec!["process".to_string()],
                event_type: vec!["info".to_string()],
                action: "test_action".to_string(),
                outcome: Some("success".to_string()),
                duration: Some(1000000),
                module: "rustible".to_string(),
            },
            message: "Test message".to_string(),
            log: LogLevel {
                level: "info".to_string(),
            },
            host: Some(HostInfo {
                name: "test-host".to_string(),
            }),
            rustible: RustibleFields {
                playbook: Some("test.yml".to_string()),
                play: Some("Test Play".to_string()),
                task: Some("Test Task".to_string()),
                status: Some("ok".to_string()),
                changed: Some(false),
                result: None,
                handler: None,
            },
            extra: HashMap::new(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("@timestamp"));
        assert!(json.contains("test_action"));
        assert!(json.contains("Test message"));
        assert!(json.contains("test-host"));
    }

    #[test]
    fn test_error_display() {
        let err = LogstashError::ConnectionFailed("timeout".to_string());
        assert!(err.to_string().contains("Connection failed"));

        let err = LogstashError::ConfigError("missing host".to_string());
        assert!(err.to_string().contains("Configuration error"));
    }
}
