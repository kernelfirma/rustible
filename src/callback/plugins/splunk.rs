//! Splunk Callback Plugin for Rustible.
//!
//! This plugin sends execution events to Splunk via the HTTP Event Collector
//! (HEC) for log aggregation, analysis, and SIEM integration.
//!
//! # Features
//!
//! - HTTP Event Collector (HEC) integration
//! - Configurable source, sourcetype, and index
//! - Event batching for improved performance
//! - TLS support with certificate verification options
//! - Automatic retry with exponential backoff
//! - CIM (Common Information Model) compatible events
//!
//! # Configuration
//!
//! Configuration via environment variables:
//!
//! - `RUSTIBLE_SPLUNK_HEC_URL`: HEC endpoint URL (required)
//! - `RUSTIBLE_SPLUNK_HEC_TOKEN`: HEC authentication token (required)
//! - `RUSTIBLE_SPLUNK_INDEX`: Target index (default: "main")
//! - `RUSTIBLE_SPLUNK_SOURCE`: Event source (default: "rustible")
//! - `RUSTIBLE_SPLUNK_SOURCETYPE`: Event sourcetype (default: "rustible:execution")
//! - `RUSTIBLE_SPLUNK_HOST`: Host field override (default: local hostname)
//! - `RUSTIBLE_SPLUNK_VERIFY_TLS`: Verify TLS certificates (default: true)
//! - `RUSTIBLE_SPLUNK_BATCH_SIZE`: Events per batch (default: 50)
//! - `RUSTIBLE_SPLUNK_FLUSH_INTERVAL`: Flush interval in ms (default: 5000)
//!
//! # Splunk Configuration
//!
//! Enable HTTP Event Collector in Splunk:
//!
//! 1. Settings > Data inputs > HTTP Event Collector
//! 2. Create a new token with appropriate index permissions
//! 3. Note the token value and HEC endpoint URL
//!
//! # Usage
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::callback::prelude::*;
//! use rustible::callback::plugins::{SplunkCallback, SplunkConfig};
//!
//! // From environment variables
//! let callback = SplunkCallback::from_env()?;
//!
//! // With custom configuration
//! let config = SplunkConfig::builder()
//!     .hec_url("https://splunk.example.com:8088/services/collector")
//!     .token("your-hec-token")
//!     .index("automation")
//!     .build();
//! let callback = SplunkCallback::new(config)?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::facts::Facts;
use crate::traits::{ExecutionCallback, ExecutionResult};

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during Splunk operations.
#[derive(Debug, thiserror::Error)]
pub enum SplunkError {
    /// Missing required configuration
    #[error("Missing required configuration: {0}")]
    MissingConfig(String),

    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    /// Splunk HEC returned an error
    #[error("Splunk HEC error: code={code}, text={text}")]
    HecError { code: i32, text: String },

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Result type for Splunk operations.
pub type SplunkResult<T> = Result<T, SplunkError>;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for the Splunk callback plugin.
#[derive(Debug, Clone)]
pub struct SplunkConfig {
    /// HEC endpoint URL (e.g., https://splunk:8088/services/collector)
    pub hec_url: String,
    /// HEC authentication token
    pub token: String,
    /// Target index
    pub index: String,
    /// Event source
    pub source: String,
    /// Event sourcetype
    pub sourcetype: String,
    /// Host field (defaults to local hostname)
    pub host: Option<String>,
    /// Verify TLS certificates
    pub verify_tls: bool,
    /// Batch size for event collection
    pub batch_size: usize,
    /// Flush interval in milliseconds
    pub flush_interval_ms: u64,
    /// Connection timeout in seconds
    pub timeout_secs: u64,
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Additional static fields for all events
    pub extra_fields: HashMap<String, String>,
    /// Whether to log task start events
    pub log_task_start: bool,
    /// Whether to log facts gathered events
    pub log_facts: bool,
    /// Include full result data in events
    pub include_result_data: bool,
}

impl Default for SplunkConfig {
    fn default() -> Self {
        Self {
            hec_url: String::new(),
            token: String::new(),
            index: "main".to_string(),
            source: "rustible".to_string(),
            sourcetype: "rustible:execution".to_string(),
            host: None,
            verify_tls: true,
            batch_size: 50,
            flush_interval_ms: 5000,
            timeout_secs: 30,
            max_retries: 3,
            extra_fields: HashMap::new(),
            log_task_start: false,
            log_facts: false,
            include_result_data: true,
        }
    }
}

impl SplunkConfig {
    /// Create a new configuration builder.
    pub fn builder() -> SplunkConfigBuilder {
        SplunkConfigBuilder::default()
    }

    /// Load configuration from environment variables.
    pub fn from_env() -> SplunkResult<Self> {
        let hec_url = env::var("RUSTIBLE_SPLUNK_HEC_URL")
            .map_err(|_| SplunkError::MissingConfig("RUSTIBLE_SPLUNK_HEC_URL".to_string()))?;

        let token = env::var("RUSTIBLE_SPLUNK_HEC_TOKEN")
            .map_err(|_| SplunkError::MissingConfig("RUSTIBLE_SPLUNK_HEC_TOKEN".to_string()))?;

        Ok(Self {
            hec_url,
            token,
            index: env::var("RUSTIBLE_SPLUNK_INDEX").unwrap_or_else(|_| "main".to_string()),
            source: env::var("RUSTIBLE_SPLUNK_SOURCE").unwrap_or_else(|_| "rustible".to_string()),
            sourcetype: env::var("RUSTIBLE_SPLUNK_SOURCETYPE")
                .unwrap_or_else(|_| "rustible:execution".to_string()),
            host: env::var("RUSTIBLE_SPLUNK_HOST").ok(),
            verify_tls: env::var("RUSTIBLE_SPLUNK_VERIFY_TLS")
                .map(|v| !v.eq_ignore_ascii_case("false") && v != "0")
                .unwrap_or(true),
            batch_size: env::var("RUSTIBLE_SPLUNK_BATCH_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50),
            flush_interval_ms: env::var("RUSTIBLE_SPLUNK_FLUSH_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5000),
            timeout_secs: env::var("RUSTIBLE_SPLUNK_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            max_retries: env::var("RUSTIBLE_SPLUNK_RETRIES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
            extra_fields: HashMap::new(),
            log_task_start: env::var("RUSTIBLE_SPLUNK_LOG_TASK_START")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            log_facts: env::var("RUSTIBLE_SPLUNK_LOG_FACTS")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(false),
            include_result_data: env::var("RUSTIBLE_SPLUNK_INCLUDE_DATA")
                .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
                .unwrap_or(true),
        })
    }

    /// Validate the configuration.
    pub fn validate(&self) -> SplunkResult<()> {
        if self.hec_url.is_empty() {
            return Err(SplunkError::MissingConfig("hec_url".to_string()));
        }
        if self.token.is_empty() {
            return Err(SplunkError::MissingConfig("token".to_string()));
        }
        if !self.hec_url.starts_with("http://") && !self.hec_url.starts_with("https://") {
            return Err(SplunkError::InvalidConfig(
                "hec_url must start with http:// or https://".to_string(),
            ));
        }
        Ok(())
    }
}

/// Builder for SplunkConfig.
#[derive(Debug, Default)]
pub struct SplunkConfigBuilder {
    config: SplunkConfig,
}

impl SplunkConfigBuilder {
    /// Set the HEC URL.
    pub fn hec_url(mut self, url: impl Into<String>) -> Self {
        self.config.hec_url = url.into();
        self
    }

    /// Set the HEC token.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.config.token = token.into();
        self
    }

    /// Set the target index.
    pub fn index(mut self, index: impl Into<String>) -> Self {
        self.config.index = index.into();
        self
    }

    /// Set the event source.
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.config.source = source.into();
        self
    }

    /// Set the event sourcetype.
    pub fn sourcetype(mut self, sourcetype: impl Into<String>) -> Self {
        self.config.sourcetype = sourcetype.into();
        self
    }

    /// Set the host field.
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.config.host = Some(host.into());
        self
    }

    /// Enable or disable TLS verification.
    pub fn verify_tls(mut self, verify: bool) -> Self {
        self.config.verify_tls = verify;
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

    /// Add an extra field.
    pub fn extra_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.extra_fields.insert(key.into(), value.into());
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

    /// Enable or disable result data inclusion.
    pub fn include_result_data(mut self, include: bool) -> Self {
        self.config.include_result_data = include;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> SplunkConfig {
        self.config
    }
}

// ============================================================================
// Splunk HEC Event Structure
// ============================================================================

/// Splunk HEC event wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HecEvent {
    /// Event timestamp (Unix epoch time)
    time: f64,
    /// Host field
    host: String,
    /// Source field
    source: String,
    /// Sourcetype field
    sourcetype: String,
    /// Index field
    index: String,
    /// Event data
    event: SplunkEventData,
}

/// Splunk event data (CIM-compatible).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SplunkEventData {
    /// Event category
    event_category: String,
    /// Event action
    action: String,
    /// Status/outcome
    status: String,
    /// Human-readable message
    message: String,
    /// Severity level
    severity: String,
    /// Playbook name
    #[serde(skip_serializing_if = "Option::is_none")]
    playbook: Option<String>,
    /// Play name
    #[serde(skip_serializing_if = "Option::is_none")]
    play: Option<String>,
    /// Task name
    #[serde(skip_serializing_if = "Option::is_none")]
    task: Option<String>,
    /// Target host
    #[serde(skip_serializing_if = "Option::is_none")]
    target_host: Option<String>,
    /// Whether changes were made
    #[serde(skip_serializing_if = "Option::is_none")]
    changed: Option<bool>,
    /// Duration in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    /// Handler name
    #[serde(skip_serializing_if = "Option::is_none")]
    handler: Option<String>,
    /// Result data
    #[serde(skip_serializing_if = "Option::is_none")]
    result_data: Option<serde_json::Value>,
    /// Additional fields
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

/// HEC response structure.
#[derive(Debug, Deserialize)]
struct HecResponse {
    /// Response text
    text: String,
    /// Response code
    code: i32,
}

// ============================================================================
// Internal State
// ============================================================================

/// Internal execution state.
#[derive(Debug, Default)]
struct SplunkState {
    /// Current playbook
    playbook: Option<String>,
    /// Current play
    play: Option<String>,
    /// Playbook start time
    playbook_start: Option<Instant>,
    /// Event buffer for batching
    buffer: Vec<HecEvent>,
    /// Last flush time
    last_flush: Option<Instant>,
    /// Hostname cache
    hostname: String,
}

// ============================================================================
// Splunk Callback Implementation
// ============================================================================

/// Splunk callback plugin for sending events to Splunk HEC.
///
/// This callback integrates Rustible with Splunk for log aggregation,
/// SIEM integration, and operational analytics.
#[derive(Debug)]
pub struct SplunkCallback {
    /// Configuration
    config: SplunkConfig,
    /// HTTP client
    client: Client,
    /// Internal state
    state: Arc<RwLock<SplunkState>>,
}

impl SplunkCallback {
    /// Create a new Splunk callback with the given configuration.
    pub fn new(config: SplunkConfig) -> SplunkResult<Self> {
        config.validate()?;

        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .danger_accept_invalid_certs(!config.verify_tls)
            .build()?;

        let hostname = config.host.clone().unwrap_or_else(|| {
            hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "unknown".to_string())
        });

        Ok(Self {
            config,
            client,
            state: Arc::new(RwLock::new(SplunkState {
                hostname,
                ..Default::default()
            })),
        })
    }

    /// Create a Splunk callback from environment variables.
    pub fn from_env() -> SplunkResult<Self> {
        Self::new(SplunkConfig::from_env()?)
    }

    /// Create a base event.
    fn create_event(&self, action: &str, message: String, severity: &str) -> HecEvent {
        let state = self.state.read();
        let timestamp = Utc::now().timestamp() as f64
            + (Utc::now().timestamp_subsec_micros() as f64 / 1_000_000.0);

        let extra: HashMap<String, serde_json::Value> = self
            .config
            .extra_fields
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();

        HecEvent {
            time: timestamp,
            host: state.hostname.clone(),
            source: self.config.source.clone(),
            sourcetype: self.config.sourcetype.clone(),
            index: self.config.index.clone(),
            event: SplunkEventData {
                event_category: "automation".to_string(),
                action: action.to_string(),
                status: "info".to_string(),
                message,
                severity: severity.to_string(),
                playbook: state.playbook.clone(),
                play: state.play.clone(),
                task: None,
                target_host: None,
                changed: None,
                duration_ms: None,
                handler: None,
                result_data: None,
                extra,
            },
        }
    }

    /// Add event to buffer and flush if needed.
    async fn buffer_event(&self, event: HecEvent) {
        let should_flush = {
            let mut state = self.state.write();
            state.buffer.push(event);

            state.buffer.len() >= self.config.batch_size
                || state
                    .last_flush
                    .is_none_or(|t| t.elapsed().as_millis() as u64 >= self.config.flush_interval_ms)
        };

        if should_flush {
            self.flush_buffer().await;
        }
    }

    /// Flush buffered events to Splunk.
    async fn flush_buffer(&self) {
        let events = {
            let mut state = self.state.write();
            state.last_flush = Some(Instant::now());
            std::mem::take(&mut state.buffer)
        };

        if events.is_empty() {
            return;
        }

        // Build payload - one JSON object per line
        let payload: String = events
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n");

        for attempt in 0..self.config.max_retries {
            if attempt > 0 {
                let backoff = Duration::from_millis(100 * 2u64.pow(attempt));
                tokio::time::sleep(backoff).await;
            }

            match self.send_to_hec(&payload).await {
                Ok(()) => {
                    debug!("Sent {} events to Splunk HEC", events.len());
                    return;
                }
                Err(e) => {
                    warn!("Splunk HEC send attempt {} failed: {}", attempt + 1, e);
                }
            }
        }

        error!(
            "Failed to send {} events to Splunk after {} attempts",
            events.len(),
            self.config.max_retries
        );
    }

    /// Send payload to Splunk HEC.
    async fn send_to_hec(&self, payload: &str) -> SplunkResult<()> {
        let response = self
            .client
            .post(&self.config.hec_url)
            .header("Authorization", format!("Splunk {}", self.config.token))
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await?;

        let status = response.status();

        if status.is_success() {
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            if let Ok(hec_response) = serde_json::from_str::<HecResponse>(&body) {
                Err(SplunkError::HecError {
                    code: hec_response.code,
                    text: hec_response.text,
                })
            } else {
                Err(SplunkError::HecError {
                    code: status.as_u16() as i32,
                    text: body,
                })
            }
        }
    }
}

impl Clone for SplunkCallback {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            client: self.client.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

#[async_trait]
impl ExecutionCallback for SplunkCallback {
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
        );

        self.buffer_event(event).await;
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let duration_ms = self
            .state
            .read()
            .playbook_start
            .map(|t| t.elapsed().as_millis() as u64);

        let mut event = self.create_event(
            "playbook_end",
            format!(
                "Playbook {}: {}",
                if success { "completed" } else { "failed" },
                name
            ),
            if success { "info" } else { "error" },
        );

        event.event.status = if success {
            "success".to_string()
        } else {
            "failure".to_string()
        };
        event.event.duration_ms = duration_ms;

        self.buffer_event(event).await;

        // Flush remaining events
        self.flush_buffer().await;

        // Clear state
        let mut state = self.state.write();
        state.playbook = None;
        state.play = None;
        state.playbook_start = None;

        info!(
            "Splunk: playbook {} {} ({}ms)",
            name,
            if success { "completed" } else { "failed" },
            duration_ms.unwrap_or(0)
        );
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
        );

        event
            .event
            .extra
            .insert("host_count".to_string(), serde_json::json!(hosts.len()));

        self.buffer_event(event).await;
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
        );

        event.event.status = if success {
            "success".to_string()
        } else {
            "failure".to_string()
        };

        self.buffer_event(event).await;
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        if !self.config.log_task_start {
            return;
        }

        let mut event = self.create_event(
            "task_start",
            format!("Task started: {} on {}", name, host),
            "debug",
        );

        event.event.task = Some(name.to_string());
        event.event.target_host = Some(host.to_string());

        self.buffer_event(event).await;
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

        let severity = if !result.result.success {
            "error"
        } else if result.result.changed {
            "info"
        } else {
            "debug"
        };

        let mut event = self.create_event(
            "task_complete",
            format!("Task {}: {} on {}", status, result.task_name, result.host),
            severity,
        );

        event.event.task = Some(result.task_name.clone());
        event.event.target_host = Some(result.host.clone());
        event.event.status = status.to_string();
        event.event.changed = Some(result.result.changed);
        event.event.duration_ms = Some(result.duration.as_millis() as u64);

        if self.config.include_result_data {
            if let Some(ref data) = result.result.data {
                event.event.result_data = Some(data.clone());
            }
        }

        if !result.result.message.is_empty() {
            event.event.extra.insert(
                "task_message".to_string(),
                serde_json::Value::String(result.result.message.clone()),
            );
        }

        self.buffer_event(event).await;
    }

    async fn on_handler_triggered(&self, name: &str) {
        let mut event = self.create_event(
            "handler_triggered",
            format!("Handler triggered: {}", name),
            "debug",
        );

        event.event.handler = Some(name.to_string());

        self.buffer_event(event).await;
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        if !self.config.log_facts {
            return;
        }

        let mut event = self.create_event(
            "facts_gathered",
            format!("Facts gathered from {} ({} facts)", host, facts.all().len()),
            "debug",
        );

        event.event.target_host = Some(host.to_string());
        event.event.extra.insert(
            "fact_count".to_string(),
            serde_json::json!(facts.all().len()),
        );

        self.buffer_event(event).await;
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
        let config = SplunkConfig::builder()
            .hec_url("https://splunk:8088/services/collector")
            .token("test-token")
            .index("test-index")
            .source("test-source")
            .sourcetype("test:sourcetype")
            .host("test-host")
            .verify_tls(false)
            .batch_size(100)
            .extra_field("env", "test")
            .log_task_start(true)
            .build();

        assert_eq!(config.hec_url, "https://splunk:8088/services/collector");
        assert_eq!(config.token, "test-token");
        assert_eq!(config.index, "test-index");
        assert_eq!(config.source, "test-source");
        assert_eq!(config.sourcetype, "test:sourcetype");
        assert_eq!(config.host, Some("test-host".to_string()));
        assert!(!config.verify_tls);
        assert_eq!(config.batch_size, 100);
        assert_eq!(config.extra_fields.get("env"), Some(&"test".to_string()));
        assert!(config.log_task_start);
    }

    #[test]
    fn test_default_config() {
        let config = SplunkConfig::default();
        assert!(config.hec_url.is_empty());
        assert!(config.token.is_empty());
        assert_eq!(config.index, "main");
        assert_eq!(config.source, "rustible");
        assert_eq!(config.sourcetype, "rustible:execution");
        assert!(config.host.is_none());
        assert!(config.verify_tls);
        assert_eq!(config.batch_size, 50);
        assert_eq!(config.flush_interval_ms, 5000);
        assert!(!config.log_task_start);
    }

    #[test]
    fn test_config_validation() {
        let config = SplunkConfig::default();
        assert!(config.validate().is_err());

        let mut config = SplunkConfig::default();
        config.hec_url = "https://splunk:8088".to_string();
        assert!(config.validate().is_err());

        let mut config = SplunkConfig::default();
        config.hec_url = "https://splunk:8088".to_string();
        config.token = "test-token".to_string();
        assert!(config.validate().is_ok());

        let mut config = SplunkConfig::default();
        config.hec_url = "invalid-url".to_string();
        config.token = "test-token".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_hec_event_serialization() {
        let event = HecEvent {
            time: 1234567890.123,
            host: "test-host".to_string(),
            source: "rustible".to_string(),
            sourcetype: "rustible:execution".to_string(),
            index: "main".to_string(),
            event: SplunkEventData {
                event_category: "automation".to_string(),
                action: "task_complete".to_string(),
                status: "success".to_string(),
                message: "Test message".to_string(),
                severity: "info".to_string(),
                playbook: Some("test.yml".to_string()),
                play: Some("Test Play".to_string()),
                task: Some("Test Task".to_string()),
                target_host: Some("web1".to_string()),
                changed: Some(true),
                duration_ms: Some(1500),
                handler: None,
                result_data: None,
                extra: HashMap::new(),
            },
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"time\":1234567890.123"));
        assert!(json.contains("\"host\":\"test-host\""));
        assert!(json.contains("\"task_complete\""));
        assert!(json.contains("Test message"));
    }

    #[test]
    fn test_error_display() {
        let err = SplunkError::MissingConfig("test".to_string());
        assert!(err.to_string().contains("Missing required configuration"));

        let err = SplunkError::HecError {
            code: 400,
            text: "Bad request".to_string(),
        };
        assert!(err.to_string().contains("Splunk HEC error"));
        assert!(err.to_string().contains("400"));
    }
}
