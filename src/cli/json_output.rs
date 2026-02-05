//! JSON output module for Rustible
//!
//! Provides structured JSON output for scripting and automation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{self, BufWriter, Write};

/// JSON output mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JsonOutputMode {
    /// Pretty-printed JSON (default)
    #[default]
    Pretty,
    /// Compact single-line JSON
    Compact,
    /// JSON Lines format (one object per line)
    Lines,
}

/// JSON output writer
pub struct JsonOutput {
    mode: JsonOutputMode,
    buffer: Vec<JsonEvent>,
    writer: Box<dyn Write + Send>,
}

impl JsonOutput {
    /// Create a new JSON output writer
    pub fn new(mode: JsonOutputMode) -> Self {
        Self::new_with_writer(mode, Box::new(BufWriter::new(io::stdout())))
    }

    /// Create a new JSON output writer with a custom destination.
    pub fn new_with_writer(mode: JsonOutputMode, writer: Box<dyn Write + Send>) -> Self {
        Self {
            mode,
            buffer: Vec::new(),
            writer,
        }
    }

    /// Write an event to the output
    pub fn write_event(&mut self, event: JsonEvent) {
        match self.mode {
            JsonOutputMode::Lines => {
                // Write immediately for streaming
                if let Ok(json) = serde_json::to_string(&event) {
                    let _ = writeln!(self.writer, "{}", json);
                    let _ = self.writer.flush();
                }
            }
            _ => {
                // Buffer for final output
                self.buffer.push(event);
            }
        }
    }

    /// Flush the buffered output
    pub fn flush(&mut self) -> io::Result<()> {
        if self.mode == JsonOutputMode::Lines {
            // Already written
            return Ok(());
        }

        let json = match self.mode {
            JsonOutputMode::Pretty => serde_json::to_string_pretty(&self.buffer),
            JsonOutputMode::Compact => serde_json::to_string(&self.buffer),
            JsonOutputMode::Lines => unreachable!(),
        };

        if let Ok(output) = json {
            self.writer.write_all(output.as_bytes())?;
            self.writer.write_all(b"\n")?;
            self.writer.flush()?;
        }

        Ok(())
    }

    /// Create a playbook result
    pub fn playbook_result(&self) -> PlaybookResult {
        PlaybookResult::from_events(&self.buffer)
    }
}

impl Default for JsonOutput {
    fn default() -> Self {
        Self::new(JsonOutputMode::default())
    }
}

/// JSON event types for output
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JsonEvent {
    /// Playbook started
    PlaybookStart { playbook: String, timestamp: String },

    /// Playbook finished
    PlaybookEnd {
        playbook: String,
        timestamp: String,
        duration_ms: u64,
        success: bool,
    },

    /// Play started
    PlayStart {
        name: String,
        hosts: Vec<String>,
        timestamp: String,
    },

    /// Play finished
    PlayEnd {
        name: String,
        timestamp: String,
        duration_ms: u64,
    },

    /// Task started
    TaskStart {
        name: String,
        module: String,
        timestamp: String,
    },

    /// Task result for a host
    TaskResult {
        task: String,
        host: String,
        status: String,
        changed: bool,
        message: Option<String>,
        diff: Option<JsonDiffOutput>,
        timestamp: String,
        duration_ms: u64,
    },

    /// Handler triggered
    HandlerTriggered {
        name: String,
        triggered_by: String,
        timestamp: String,
    },

    /// Error occurred
    Error {
        message: String,
        task: Option<String>,
        host: Option<String>,
        timestamp: String,
    },

    /// Warning message
    Warning { message: String, timestamp: String },

    /// Debug message
    Debug {
        message: String,
        verbosity: u8,
        timestamp: String,
    },

    /// Fact gathered
    FactGathered {
        host: String,
        facts: HashMap<String, serde_json::Value>,
        timestamp: String,
    },

    /// Variable set
    VariableSet {
        host: String,
        name: String,
        value: serde_json::Value,
        timestamp: String,
    },

    /// Recap statistics
    Recap {
        hosts: HashMap<String, HostStats>,
        timestamp: String,
        duration_ms: u64,
    },
}

/// Host statistics for recap
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HostStats {
    pub ok: u32,
    pub changed: u32,
    pub unreachable: u32,
    pub failed: u32,
    pub skipped: u32,
    pub rescued: u32,
    pub ignored: u32,
}

/// Diff output for JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonDiffOutput {
    pub before: String,
    pub after: String,
    pub before_header: String,
    pub after_header: String,
}

/// Complete playbook execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookResult {
    pub playbook: String,
    pub success: bool,
    pub plays: Vec<PlayResult>,
    pub stats: HashMap<String, HostStats>,
    pub start_time: String,
    pub end_time: String,
    pub duration_ms: u64,
}

impl PlaybookResult {
    /// Create a playbook result from events
    fn from_events(events: &[JsonEvent]) -> Self {
        let mut result = PlaybookResult {
            playbook: String::new(),
            success: true,
            plays: Vec::new(),
            stats: HashMap::new(),
            start_time: String::new(),
            end_time: String::new(),
            duration_ms: 0,
        };

        let mut current_play: Option<PlayResult> = None;
        let mut current_task: Option<TaskResult> = None;

        for event in events {
            match event {
                JsonEvent::PlaybookStart {
                    playbook,
                    timestamp,
                } => {
                    result.playbook = playbook.clone();
                    result.start_time = timestamp.clone();
                }
                JsonEvent::PlaybookEnd {
                    timestamp,
                    duration_ms,
                    success,
                    ..
                } => {
                    result.end_time = timestamp.clone();
                    result.duration_ms = *duration_ms;
                    result.success = *success;
                }
                JsonEvent::PlayStart {
                    name,
                    hosts,
                    timestamp,
                } => {
                    // Save previous play if exists
                    if let Some(mut play) = current_play.take() {
                        if let Some(task) = current_task.take() {
                            play.tasks.push(task);
                        }
                        result.plays.push(play);
                    }

                    current_play = Some(PlayResult {
                        name: name.clone(),
                        hosts: hosts.clone(),
                        tasks: Vec::new(),
                        start_time: timestamp.clone(),
                        end_time: String::new(),
                        duration_ms: 0,
                    });
                }
                JsonEvent::PlayEnd {
                    timestamp,
                    duration_ms,
                    ..
                } => {
                    if let Some(ref mut play) = current_play {
                        if let Some(task) = current_task.take() {
                            play.tasks.push(task);
                        }
                        play.end_time = timestamp.clone();
                        play.duration_ms = *duration_ms;
                    }
                }
                JsonEvent::TaskStart {
                    name,
                    module,
                    timestamp,
                } => {
                    // Save previous task if exists
                    if let Some(ref mut play) = current_play {
                        if let Some(task) = current_task.take() {
                            play.tasks.push(task);
                        }
                    }

                    current_task = Some(TaskResult {
                        name: name.clone(),
                        module: module.clone(),
                        hosts: Vec::new(),
                        start_time: timestamp.clone(),
                    });
                }
                JsonEvent::TaskResult {
                    host,
                    status,
                    changed,
                    message,
                    diff,
                    duration_ms,
                    ..
                } => {
                    if let Some(ref mut task) = current_task {
                        task.hosts.push(HostResult {
                            host: host.clone(),
                            status: status.clone(),
                            changed: *changed,
                            message: message.clone(),
                            diff: diff.clone(),
                            duration_ms: *duration_ms,
                        });
                    }
                }
                JsonEvent::Recap { hosts, .. } => {
                    result.stats = hosts.clone();
                }
                JsonEvent::Error { .. } => {
                    result.success = false;
                }
                _ => {}
            }
        }

        // Save remaining play and task
        if let Some(mut play) = current_play {
            if let Some(task) = current_task {
                play.tasks.push(task);
            }
            result.plays.push(play);
        }

        result
    }
}

/// Play execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayResult {
    pub name: String,
    pub hosts: Vec<String>,
    pub tasks: Vec<TaskResult>,
    pub start_time: String,
    pub end_time: String,
    pub duration_ms: u64,
}

/// Task execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub name: String,
    pub module: String,
    pub hosts: Vec<HostResult>,
    pub start_time: String,
}

/// Host-level task result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostResult {
    pub host: String,
    pub status: String,
    pub changed: bool,
    pub message: Option<String>,
    pub diff: Option<JsonDiffOutput>,
    pub duration_ms: u64,
}

/// Get current timestamp in ISO 8601 format
pub fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Create a simple JSON error response
pub fn json_error(message: &str) -> String {
    let event = JsonEvent::Error {
        message: message.to_string(),
        task: None,
        host: None,
        timestamp: timestamp(),
    };
    serde_json::to_string_pretty(&event)
        .unwrap_or_else(|_| format!("{{\"error\": \"{}\"}}", message.replace('"', "\\\"")))
}

/// Create a simple JSON success response
pub fn json_success(message: &str) -> String {
    let result = serde_json::json!({
        "status": "success",
        "message": message,
        "timestamp": timestamp()
    });
    serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
        format!(
            "{{\"status\": \"success\", \"message\": \"{}\"}}",
            message.replace('"', "\\\"")
        )
    })
}

/// Wrapper for list output in JSON format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonListOutput<T> {
    pub count: usize,
    pub items: Vec<T>,
    pub timestamp: String,
}

impl<T: Serialize> JsonListOutput<T> {
    /// Create a new list output
    pub fn new(items: Vec<T>) -> Self {
        Self {
            count: items.len(),
            items,
            timestamp: timestamp(),
        }
    }

    /// Convert to JSON string
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Convert to compact JSON string
    pub fn to_json_compact(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Inventory host in JSON format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonHost {
    pub name: String,
    pub groups: Vec<String>,
    pub vars: HashMap<String, serde_json::Value>,
}

/// Inventory group in JSON format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonGroup {
    pub name: String,
    pub hosts: Vec<String>,
    pub children: Vec<String>,
    pub vars: HashMap<String, serde_json::Value>,
}

/// Task in JSON format for list-tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTask {
    pub name: String,
    pub module: String,
    pub tags: Vec<String>,
    pub when: Option<String>,
    pub play: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_event_serialization() {
        let event = JsonEvent::PlaybookStart {
            playbook: "test.yml".to_string(),
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("playbook_start"));
        assert!(json.contains("test.yml"));
    }

    #[test]
    fn test_json_output_buffer() {
        let mut output = JsonOutput::new(JsonOutputMode::Pretty);

        output.write_event(JsonEvent::PlaybookStart {
            playbook: "test.yml".to_string(),
            timestamp: timestamp(),
        });

        output.write_event(JsonEvent::PlaybookEnd {
            playbook: "test.yml".to_string(),
            timestamp: timestamp(),
            duration_ms: 1000,
            success: true,
        });

        assert_eq!(output.buffer.len(), 2);
    }

    #[test]
    fn test_json_error() {
        let error = json_error("Test error message");
        assert!(error.contains("error"));
        assert!(error.contains("Test error message"));
    }

    #[test]
    fn test_json_success() {
        let success = json_success("Operation completed");
        assert!(success.contains("success"));
        assert!(success.contains("Operation completed"));
    }

    #[test]
    fn test_json_list_output() {
        let list = JsonListOutput::new(vec!["item1", "item2", "item3"]);
        assert_eq!(list.count, 3);

        let json = list.to_json();
        assert!(json.contains("\"count\": 3"));
    }

    #[test]
    fn test_host_stats_default() {
        let stats = HostStats::default();
        assert_eq!(stats.ok, 0);
        assert_eq!(stats.changed, 0);
        assert_eq!(stats.failed, 0);
    }
}
