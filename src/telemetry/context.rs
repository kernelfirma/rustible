//! Trace context and propagation for distributed tracing.
//!
//! This module provides types for managing trace context across
//! distributed systems and propagating context through different
//! transport mechanisms (HTTP headers, SSH metadata, etc.).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// A unique identifier for a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId([u8; 16]);

impl TraceId {
    /// Create a new random trace ID.
    pub fn new() -> Self {
        let mut bytes = [0u8; 16];
        for byte in &mut bytes {
            *byte = rand::random();
        }
        Self(bytes)
    }

    /// Create a trace ID from bytes.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Get the bytes of the trace ID.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Check if the trace ID is valid (non-zero).
    pub fn is_valid(&self) -> bool {
        self.0.iter().any(|&b| b != 0)
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl std::str::FromStr for TraceId {
    type Err = ParseTraceIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 32 {
            return Err(ParseTraceIdError::InvalidLength);
        }

        let mut bytes = [0u8; 16];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hex_str =
                std::str::from_utf8(chunk).map_err(|_| ParseTraceIdError::InvalidHex)?;
            bytes[i] = u8::from_str_radix(hex_str, 16).map_err(|_| ParseTraceIdError::InvalidHex)?;
        }
        Ok(Self(bytes))
    }
}

/// Error parsing a trace ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseTraceIdError {
    InvalidLength,
    InvalidHex,
}

impl fmt::Display for ParseTraceIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseTraceIdError::InvalidLength => write!(f, "invalid trace ID length"),
            ParseTraceIdError::InvalidHex => write!(f, "invalid hex in trace ID"),
        }
    }
}

impl std::error::Error for ParseTraceIdError {}

/// A unique identifier for a span within a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId([u8; 8]);

impl SpanId {
    /// Create a new random span ID.
    pub fn new() -> Self {
        let mut bytes = [0u8; 8];
        for byte in &mut bytes {
            *byte = rand::random();
        }
        Self(bytes)
    }

    /// Create a span ID from bytes.
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    /// Get the bytes of the span ID.
    pub fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }

    /// Check if the span ID is valid (non-zero).
    pub fn is_valid(&self) -> bool {
        self.0.iter().any(|&b| b != 0)
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl std::str::FromStr for SpanId {
    type Err = ParseSpanIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 {
            return Err(ParseSpanIdError::InvalidLength);
        }

        let mut bytes = [0u8; 8];
        for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
            let hex_str =
                std::str::from_utf8(chunk).map_err(|_| ParseSpanIdError::InvalidHex)?;
            bytes[i] = u8::from_str_radix(hex_str, 16).map_err(|_| ParseSpanIdError::InvalidHex)?;
        }
        Ok(Self(bytes))
    }
}

/// Error parsing a span ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseSpanIdError {
    InvalidLength,
    InvalidHex,
}

impl fmt::Display for ParseSpanIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseSpanIdError::InvalidLength => write!(f, "invalid span ID length"),
            ParseSpanIdError::InvalidHex => write!(f, "invalid hex in span ID"),
        }
    }
}

impl std::error::Error for ParseSpanIdError {}

/// Trace context containing the current trace and span information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    /// The trace ID
    pub trace_id: TraceId,

    /// The current span ID
    pub span_id: SpanId,

    /// The parent span ID (if any)
    pub parent_span_id: Option<SpanId>,

    /// Trace flags
    pub trace_flags: TraceFlags,

    /// Trace state (vendor-specific key-value pairs)
    pub trace_state: TraceState,
}

impl TraceContext {
    /// Create a new trace context with a random trace and span ID.
    pub fn new() -> Self {
        Self {
            trace_id: TraceId::new(),
            span_id: SpanId::new(),
            parent_span_id: None,
            trace_flags: TraceFlags::SAMPLED,
            trace_state: TraceState::default(),
        }
    }

    /// Create a child context with a new span ID.
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id,
            span_id: SpanId::new(),
            parent_span_id: Some(self.span_id),
            trace_flags: self.trace_flags,
            trace_state: self.trace_state.clone(),
        }
    }

    /// Check if this context is sampled.
    pub fn is_sampled(&self) -> bool {
        self.trace_flags.contains(TraceFlags::SAMPLED)
    }

    /// Check if this context is valid.
    pub fn is_valid(&self) -> bool {
        self.trace_id.is_valid() && self.span_id.is_valid()
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Trace flags as defined by W3C Trace Context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TraceFlags(u8);

impl TraceFlags {
    /// No flags set.
    pub const NONE: Self = Self(0);

    /// The trace is sampled.
    pub const SAMPLED: Self = Self(0x01);

    /// Check if the given flag is set.
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Get the raw byte value.
    pub fn as_byte(self) -> u8 {
        self.0
    }

    /// Create from a byte value.
    pub fn from_byte(byte: u8) -> Self {
        Self(byte)
    }
}

/// Trace state for vendor-specific trace context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceState {
    /// Key-value pairs in the trace state.
    entries: Vec<(String, String)>,
}

impl TraceState {
    /// Create a new empty trace state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a value from the trace state.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Insert a value into the trace state.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k == &key) {
            entry.1 = value.into();
        } else {
            self.entries.push((key, value.into()));
        }
    }

    /// Remove a value from the trace state.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == key) {
            Some(self.entries.remove(pos).1)
        } else {
            None
        }
    }

    /// Iterate over entries.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

impl fmt::Display for TraceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<_> = self.entries.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        write!(f, "{}", parts.join(","))
    }
}

/// Span context for correlating spans across service boundaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanContext {
    /// The trace context
    pub trace_context: TraceContext,

    /// Span name
    pub name: String,

    /// Span kind
    pub kind: SpanKind,

    /// Start time (Unix timestamp in nanoseconds)
    pub start_time_ns: u64,

    /// Span attributes
    pub attributes: HashMap<String, SpanValue>,
}

impl SpanContext {
    /// Create a new span context.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            trace_context: TraceContext::new(),
            name: name.into(),
            kind: SpanKind::Internal,
            start_time_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            attributes: HashMap::new(),
        }
    }

    /// Create a span context with an existing trace context.
    pub fn with_trace_context(mut self, ctx: TraceContext) -> Self {
        self.trace_context = ctx;
        self
    }

    /// Set the span kind.
    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    /// Add an attribute.
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<SpanValue>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }
}

/// Span kind as defined by OpenTelemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpanKind {
    /// Internal span (default)
    Internal,
    /// Server span (handling incoming request)
    Server,
    /// Client span (making outgoing request)
    Client,
    /// Producer span (creating message)
    Producer,
    /// Consumer span (receiving message)
    Consumer,
}

impl Default for SpanKind {
    fn default() -> Self {
        Self::Internal
    }
}

/// Span attribute value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SpanValue {
    String(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    StringArray(Vec<String>),
    IntArray(Vec<i64>),
    FloatArray(Vec<f64>),
    BoolArray(Vec<bool>),
}

impl From<String> for SpanValue {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<&str> for SpanValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<i64> for SpanValue {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

impl From<i32> for SpanValue {
    fn from(i: i32) -> Self {
        Self::Int(i64::from(i))
    }
}

impl From<f64> for SpanValue {
    fn from(f: f64) -> Self {
        Self::Float(f)
    }
}

impl From<bool> for SpanValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

/// Trace context propagator for injecting/extracting context from carriers.
pub trait TraceContextPropagator {
    /// The carrier type (e.g., HashMap for headers).
    type Carrier;

    /// Inject the trace context into the carrier.
    fn inject(&self, context: &TraceContext, carrier: &mut Self::Carrier);

    /// Extract the trace context from the carrier.
    fn extract(&self, carrier: &Self::Carrier) -> Option<TraceContext>;
}

/// W3C Trace Context propagator.
pub struct W3CTraceContextPropagator;

impl TraceContextPropagator for W3CTraceContextPropagator {
    type Carrier = HashMap<String, String>;

    fn inject(&self, context: &TraceContext, carrier: &mut Self::Carrier) {
        // traceparent header: version-trace_id-parent_id-flags
        let traceparent = format!(
            "00-{}-{}-{:02x}",
            context.trace_id,
            context.span_id,
            context.trace_flags.as_byte()
        );
        carrier.insert("traceparent".to_string(), traceparent);

        // tracestate header (if non-empty)
        let tracestate = context.trace_state.to_string();
        if !tracestate.is_empty() {
            carrier.insert("tracestate".to_string(), tracestate);
        }
    }

    fn extract(&self, carrier: &Self::Carrier) -> Option<TraceContext> {
        let traceparent = carrier.get("traceparent")?;
        let parts: Vec<&str> = traceparent.split('-').collect();

        if parts.len() != 4 || parts[0] != "00" {
            return None;
        }

        let trace_id: TraceId = parts[1].parse().ok()?;
        let span_id: SpanId = parts[2].parse().ok()?;
        let flags = u8::from_str_radix(parts[3], 16).ok()?;

        let mut trace_state = TraceState::new();
        if let Some(state_str) = carrier.get("tracestate") {
            for entry in state_str.split(',') {
                if let Some((key, value)) = entry.split_once('=') {
                    trace_state.insert(key.trim(), value.trim());
                }
            }
        }

        Some(TraceContext {
            trace_id,
            span_id,
            parent_span_id: None,
            trace_flags: TraceFlags::from_byte(flags),
            trace_state,
        })
    }
}

/// B3 (Zipkin) propagator.
pub struct B3Propagator {
    /// Use single header format (B3: {TraceId}-{SpanId}-{SamplingState}-{ParentSpanId})
    pub single_header: bool,
}

impl Default for B3Propagator {
    fn default() -> Self {
        Self {
            single_header: false,
        }
    }
}

impl TraceContextPropagator for B3Propagator {
    type Carrier = HashMap<String, String>;

    fn inject(&self, context: &TraceContext, carrier: &mut Self::Carrier) {
        if self.single_header {
            let mut b3 = format!(
                "{}-{}-{}",
                context.trace_id,
                context.span_id,
                if context.is_sampled() { "1" } else { "0" }
            );
            if let Some(parent) = &context.parent_span_id {
                b3.push('-');
                b3.push_str(&parent.to_string());
            }
            carrier.insert("b3".to_string(), b3);
        } else {
            carrier.insert("X-B3-TraceId".to_string(), context.trace_id.to_string());
            carrier.insert("X-B3-SpanId".to_string(), context.span_id.to_string());
            carrier.insert(
                "X-B3-Sampled".to_string(),
                if context.is_sampled() {
                    "1".to_string()
                } else {
                    "0".to_string()
                },
            );
            if let Some(parent) = &context.parent_span_id {
                carrier.insert("X-B3-ParentSpanId".to_string(), parent.to_string());
            }
        }
    }

    fn extract(&self, carrier: &Self::Carrier) -> Option<TraceContext> {
        // Try single header first
        if let Some(b3) = carrier.get("b3").or_else(|| carrier.get("B3")) {
            let parts: Vec<&str> = b3.split('-').collect();
            if parts.len() >= 3 {
                let trace_id: TraceId = parts[0].parse().ok()?;
                let span_id: SpanId = parts[1].parse().ok()?;
                let sampled = parts[2] == "1" || parts[2] == "true";
                let parent_span_id = if parts.len() > 3 {
                    parts[3].parse().ok()
                } else {
                    None
                };

                return Some(TraceContext {
                    trace_id,
                    span_id,
                    parent_span_id,
                    trace_flags: if sampled {
                        TraceFlags::SAMPLED
                    } else {
                        TraceFlags::NONE
                    },
                    trace_state: TraceState::default(),
                });
            }
        }

        // Try multi-header format
        let trace_id: TraceId = carrier
            .get("X-B3-TraceId")
            .or_else(|| carrier.get("x-b3-traceid"))?
            .parse()
            .ok()?;
        let span_id: SpanId = carrier
            .get("X-B3-SpanId")
            .or_else(|| carrier.get("x-b3-spanid"))?
            .parse()
            .ok()?;
        let sampled = carrier
            .get("X-B3-Sampled")
            .or_else(|| carrier.get("x-b3-sampled"))
            .map(|s| s == "1" || s == "true")
            .unwrap_or(true);
        let parent_span_id = carrier
            .get("X-B3-ParentSpanId")
            .or_else(|| carrier.get("x-b3-parentspanid"))
            .and_then(|s| s.parse().ok());

        Some(TraceContext {
            trace_id,
            span_id,
            parent_span_id,
            trace_flags: if sampled {
                TraceFlags::SAMPLED
            } else {
                TraceFlags::NONE
            },
            trace_state: TraceState::default(),
        })
    }
}

use crate::utils::shell_escape;

/// SSH-friendly context propagator for embedding trace context in SSH commands.
pub struct SshContextPropagator;

impl SshContextPropagator {
    /// Environment variable name for trace parent.
    pub const TRACEPARENT_VAR: &'static str = "RUSTIBLE_TRACEPARENT";

    /// Environment variable name for trace state.
    pub const TRACESTATE_VAR: &'static str = "RUSTIBLE_TRACESTATE";

    /// Create environment variable assignments for the trace context.
    pub fn to_env_vars(context: &TraceContext) -> Vec<(String, String)> {
        let mut vars = vec![(
            Self::TRACEPARENT_VAR.to_string(),
            format!(
                "00-{}-{}-{:02x}",
                context.trace_id,
                context.span_id,
                context.trace_flags.as_byte()
            ),
        )];

        let tracestate = context.trace_state.to_string();
        if !tracestate.is_empty() {
            vars.push((Self::TRACESTATE_VAR.to_string(), tracestate));
        }

        vars
    }

    /// Create a shell command prefix with trace context.
    pub fn command_prefix(context: &TraceContext) -> String {
        let vars = Self::to_env_vars(context);
        vars.into_iter()
            .map(|(k, v)| format!("{}={}", k, shell_escape(&v)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Parse trace context from environment variables.
    pub fn from_env() -> Option<TraceContext> {
        let traceparent = std::env::var(Self::TRACEPARENT_VAR).ok()?;
        let parts: Vec<&str> = traceparent.split('-').collect();

        if parts.len() != 4 || parts[0] != "00" {
            return None;
        }

        let trace_id: TraceId = parts[1].parse().ok()?;
        let span_id: SpanId = parts[2].parse().ok()?;
        let flags = u8::from_str_radix(parts[3], 16).ok()?;

        let mut trace_state = TraceState::new();
        if let Ok(state_str) = std::env::var(Self::TRACESTATE_VAR) {
            for entry in state_str.split(',') {
                if let Some((key, value)) = entry.split_once('=') {
                    trace_state.insert(key.trim(), value.trim());
                }
            }
        }

        Some(TraceContext {
            trace_id,
            span_id,
            parent_span_id: None,
            trace_flags: TraceFlags::from_byte(flags),
            trace_state,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_id_roundtrip() {
        let id = TraceId::new();
        let s = id.to_string();
        let parsed: TraceId = s.parse().unwrap();
        assert_eq!(id.as_bytes(), parsed.as_bytes());
    }

    #[test]
    fn test_span_id_roundtrip() {
        let id = SpanId::new();
        let s = id.to_string();
        let parsed: SpanId = s.parse().unwrap();
        assert_eq!(id.as_bytes(), parsed.as_bytes());
    }

    #[test]
    fn test_w3c_propagator() {
        let propagator = W3CTraceContextPropagator;
        let context = TraceContext::new();

        let mut carrier = HashMap::new();
        propagator.inject(&context, &mut carrier);

        assert!(carrier.contains_key("traceparent"));

        let extracted = propagator.extract(&carrier).unwrap();
        assert_eq!(
            context.trace_id.as_bytes(),
            extracted.trace_id.as_bytes()
        );
    }

    #[test]
    fn test_b3_propagator() {
        let propagator = B3Propagator::default();
        let context = TraceContext::new();

        let mut carrier = HashMap::new();
        propagator.inject(&context, &mut carrier);

        assert!(carrier.contains_key("X-B3-TraceId"));
        assert!(carrier.contains_key("X-B3-SpanId"));

        let extracted = propagator.extract(&carrier).unwrap();
        assert_eq!(
            context.trace_id.as_bytes(),
            extracted.trace_id.as_bytes()
        );
    }

    #[test]
    fn test_child_context() {
        let parent = TraceContext::new();
        let child = parent.child();

        assert_eq!(parent.trace_id.as_bytes(), child.trace_id.as_bytes());
        assert_ne!(parent.span_id.as_bytes(), child.span_id.as_bytes());
        assert_eq!(Some(parent.span_id), child.parent_span_id);
    }

    #[test]
    fn test_ssh_context_propagator() {
        let context = TraceContext::new();
        let vars = SshContextPropagator::to_env_vars(&context);

        assert!(!vars.is_empty());
        assert!(vars
            .iter()
            .any(|(k, _)| k == SshContextPropagator::TRACEPARENT_VAR));
    }
}
