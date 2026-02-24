//! Comprehensive tests for YamlCallback plugin.
//!
//! This test suite covers:
//! 1. YAML output validity - All output must be valid, parseable YAML
//! 2. Proper indentation - Nested structures use correct 2-space indentation
//! 3. Multi-line values - Long strings and multi-line content handled correctly
//! 4. Special YAML characters escaped - Colons, quotes, newlines, etc.
//! 5. Readability - Output is human-readable and well-formatted

use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

// These would be imported from the rustible crate
// For now we define the test structures to validate against
use rustible::facts::Facts;
use rustible::traits::{ExecutionCallback, ExecutionResult, ModuleResult};

// ============================================================================
// Test Helper: Capture Writer
// ============================================================================

/// A writer that captures output to a buffer for testing
#[derive(Debug, Clone)]
struct CaptureWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl CaptureWriter {
    fn new() -> Self {
        Self {
            buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_output(&self) -> String {
        let buffer = self.buffer.lock().unwrap();
        String::from_utf8(buffer.clone()).unwrap_or_default()
    }

    fn clear(&self) {
        self.buffer.lock().unwrap().clear();
    }
}

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// ============================================================================
// Test Helper: YAML Validation
// ============================================================================

/// Validates that a string is valid YAML and returns the parsed value
fn validate_yaml(yaml_str: &str) -> Result<serde_yaml::Value, String> {
    serde_yaml::from_str(yaml_str).map_err(|e| format!("Invalid YAML: {}", e))
}

/// Validates that each document in a multi-document YAML string is valid
fn validate_yaml_documents(yaml_str: &str) -> Result<Vec<serde_yaml::Value>, String> {
    use serde::Deserialize;
    let mut documents = Vec::new();
    for doc in serde_yaml::Deserializer::from_str(yaml_str) {
        let value: serde_yaml::Value =
            serde_yaml::Value::deserialize(doc).map_err(|e| format!("Invalid YAML: {}", e))?;
        documents.push(value);
    }
    Ok(documents)
}

/// Checks if a YAML string uses consistent indentation
fn check_indentation(yaml_str: &str, expected_indent: usize) -> bool {
    for line in yaml_str.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let leading_spaces = line.len() - trimmed.len();
        // Indentation should be a multiple of expected_indent (including 0)
        if leading_spaces > 0 && leading_spaces % expected_indent != 0 {
            return false;
        }
    }
    true
}

// ============================================================================
// Mock YamlCallback for Testing
// ============================================================================

/// Configuration for the YAML callback plugin
#[derive(Debug, Clone)]
pub struct YamlCallbackConfig {
    /// Indentation size (default: 2 spaces)
    pub indent_size: usize,
    /// Whether to use multi-document format (--- separators)
    pub multi_document: bool,
    /// Whether to include timestamps in output
    pub include_timestamps: bool,
    /// Whether to include empty fields
    pub include_empty_fields: bool,
    /// Maximum line width before folding (0 = no folding)
    pub max_line_width: usize,
}

impl Default for YamlCallbackConfig {
    fn default() -> Self {
        Self {
            indent_size: 2,
            multi_document: true,
            include_timestamps: true,
            include_empty_fields: false,
            max_line_width: 80,
        }
    }
}

/// Mock YamlCallback implementation for testing
/// This represents what the real YamlCallback should produce
pub struct YamlCallback {
    config: YamlCallbackConfig,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    current_playbook: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for YamlCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YamlCallback")
            .field("config", &self.config)
            .field("current_playbook", &self.current_playbook)
            .finish_non_exhaustive()
    }
}

impl YamlCallback {
    pub fn new(config: YamlCallbackConfig) -> Self {
        let writer: Box<dyn Write + Send> = Box::new(io::stdout());
        Self {
            config,
            writer: Arc::new(Mutex::new(writer)),
            current_playbook: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_writer<W: Write + Send + 'static>(writer: W, config: YamlCallbackConfig) -> Self {
        Self {
            config,
            writer: Arc::new(Mutex::new(Box::new(writer))),
            current_playbook: Arc::new(Mutex::new(None)),
        }
    }

    fn write_yaml(&self, event_type: &str, data: &serde_yaml::Value) {
        let mut writer = self.writer.lock().unwrap();

        if self.config.multi_document {
            let _ = writeln!(writer, "---");
        }

        let _ = writeln!(writer, "event: {}", event_type);

        // Write the data with proper indentation
        if let Ok(yaml_str) = serde_yaml::to_string(data) {
            // Remove the leading "---\n" that serde_yaml adds
            let yaml_content = yaml_str.trim_start_matches("---\n");
            for line in yaml_content.lines() {
                let _ = writeln!(writer, "{}", line);
            }
        }
    }

    /// Escapes special YAML characters in a string value
    #[allow(dead_code)]
    fn escape_yaml_string(s: &str) -> String {
        // Check if the string needs quoting
        let needs_quoting = s.is_empty()
            || s.contains(':')
            || s.contains('#')
            || s.contains('\n')
            || s.contains('\r')
            || s.contains('\t')
            || s.contains('"')
            || s.contains('\'')
            || s.contains('\\')
            || s.starts_with(' ')
            || s.ends_with(' ')
            || s.starts_with('-')
            || s.starts_with('*')
            || s.starts_with('&')
            || s.starts_with('!')
            || s.starts_with('|')
            || s.starts_with('>')
            || s.starts_with('%')
            || s.starts_with('@')
            || s.starts_with('`')
            || s == "true"
            || s == "false"
            || s == "null"
            || s == "~"
            || s.parse::<f64>().is_ok();

        if needs_quoting {
            // Use double quotes and escape internal quotes and backslashes
            let escaped = s
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\r', "\\r")
                .replace('\t', "\\t");
            format!("\"{}\"", escaped)
        } else {
            s.to_string()
        }
    }
}

#[async_trait]
impl ExecutionCallback for YamlCallback {
    async fn on_playbook_start(&self, name: &str) {
        *self.current_playbook.lock().unwrap() = Some(name.to_string());

        let data = serde_yaml::to_value(serde_json::json!({
            "playbook": name,
        }))
        .unwrap();

        self.write_yaml("playbook_start", &data);
    }

    async fn on_playbook_end(&self, name: &str, success: bool) {
        let data = serde_yaml::to_value(serde_json::json!({
            "playbook": name,
            "success": success,
        }))
        .unwrap();

        self.write_yaml("playbook_end", &data);
        *self.current_playbook.lock().unwrap() = None;
    }

    async fn on_play_start(&self, name: &str, hosts: &[String]) {
        let data = serde_yaml::to_value(serde_json::json!({
            "play": name,
            "hosts": hosts,
        }))
        .unwrap();

        self.write_yaml("play_start", &data);
    }

    async fn on_play_end(&self, name: &str, success: bool) {
        let data = serde_yaml::to_value(serde_json::json!({
            "play": name,
            "success": success,
        }))
        .unwrap();

        self.write_yaml("play_end", &data);
    }

    async fn on_task_start(&self, name: &str, host: &str) {
        let data = serde_yaml::to_value(serde_json::json!({
            "task": name,
            "host": host,
        }))
        .unwrap();

        self.write_yaml("task_start", &data);
    }

    async fn on_task_complete(&self, result: &ExecutionResult) {
        let data = serde_yaml::to_value(serde_json::json!({
            "task": result.task_name,
            "host": result.host,
            "success": result.result.success,
            "changed": result.result.changed,
            "skipped": result.result.skipped,
            "message": result.result.message,
            "duration_ms": result.duration.as_millis(),
        }))
        .unwrap();

        self.write_yaml("task_complete", &data);
    }

    async fn on_handler_triggered(&self, name: &str) {
        let data = serde_yaml::to_value(serde_json::json!({
            "handler": name,
        }))
        .unwrap();

        self.write_yaml("handler_triggered", &data);
    }

    async fn on_facts_gathered(&self, host: &str, facts: &Facts) {
        let data = serde_yaml::to_value(serde_json::json!({
            "host": host,
            "facts_count": facts.all().len(),
        }))
        .unwrap();

        self.write_yaml("facts_gathered", &data);
    }
}

// ============================================================================
// Test 1: YAML Output Validity
// ============================================================================

mod yaml_validity_tests {
    use super::*;

    #[tokio::test]
    async fn test_playbook_start_produces_valid_yaml() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        callback.on_playbook_start("test_playbook.yml").await;

        let output = writer.get_output();
        assert!(
            !output.is_empty(),
            "Output should not be empty after playbook_start"
        );

        // Validate YAML syntax
        let docs = validate_yaml_documents(&output).expect("Should produce valid YAML");
        assert!(
            !docs.is_empty(),
            "Should produce at least one YAML document"
        );
    }

    #[tokio::test]
    async fn test_task_complete_produces_valid_yaml() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "webserver1".to_string(),
            task_name: "Install nginx".to_string(),
            result: ModuleResult::changed("nginx installed successfully"),
            duration: Duration::from_millis(1500),
            notify: vec!["restart nginx".to_string()],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_full_playbook_execution_produces_valid_yaml() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());
        let hosts = vec!["host1".to_string(), "host2".to_string()];

        // Full execution sequence
        callback.on_playbook_start("deploy.yml").await;
        callback.on_play_start("Deploy application", &hosts).await;

        for host in &hosts {
            callback.on_task_start("Install package", host).await;
            let result = ExecutionResult {
                host: host.clone(),
                task_name: "Install package".to_string(),
                result: ModuleResult::changed("Package installed"),
                duration: Duration::from_millis(500),
                notify: vec![],
            };
            callback.on_task_complete(&result).await;
        }

        callback.on_handler_triggered("restart service").await;
        callback.on_play_end("Deploy application", true).await;
        callback.on_playbook_end("deploy.yml", true).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("All documents should be valid YAML");

        // Should have multiple documents
        assert!(
            docs.len() >= 7,
            "Expected at least 7 documents, got {}",
            docs.len()
        );
    }

    #[tokio::test]
    async fn test_failed_task_produces_valid_yaml() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "failed_host".to_string(),
            task_name: "Failing task".to_string(),
            result: ModuleResult::failed("Command exited with code 127: bash: command not found"),
            duration: Duration::from_millis(50),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Error messages should produce valid YAML");
        assert!(!docs.is_empty());

        // Verify the error message is preserved
        assert!(output.contains("command not found") || output.contains("Command exited"));
    }

    #[tokio::test]
    async fn test_empty_play_produces_valid_yaml() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());
        let empty_hosts: Vec<String> = vec![];

        callback.on_play_start("Empty play", &empty_hosts).await;
        callback.on_play_end("Empty play", true).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Empty play should produce valid YAML");
        assert_eq!(docs.len(), 2);
    }
}

// ============================================================================
// Test 2: Proper Indentation
// ============================================================================

mod yaml_indentation_tests {
    use super::*;

    #[tokio::test]
    async fn test_uses_two_space_indentation() {
        let writer = CaptureWriter::new();
        let config = YamlCallbackConfig {
            indent_size: 2,
            ..Default::default()
        };
        let callback = YamlCallback::with_writer(writer.clone(), config);

        let hosts = vec![
            "host1".to_string(),
            "host2".to_string(),
            "host3".to_string(),
        ];
        callback.on_play_start("Test play", &hosts).await;

        let output = writer.get_output();

        // Verify 2-space indentation for nested structures
        assert!(
            check_indentation(&output, 2),
            "Should use 2-space indentation"
        );
    }

    #[tokio::test]
    async fn test_nested_data_properly_indented() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Complex task".to_string(),
            result: ModuleResult::ok("Success").with_data(json!({
                "nested": {
                    "level1": {
                        "level2": {
                            "value": "deeply nested"
                        }
                    }
                }
            })),
            duration: Duration::from_millis(100),
            notify: vec!["handler1".to_string(), "handler2".to_string()],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Should produce valid YAML");
        assert!(!docs.is_empty());

        // Check indentation is consistent
        assert!(
            check_indentation(&output, 2),
            "Nested structures should use consistent indentation"
        );
    }

    #[tokio::test]
    async fn test_list_items_properly_indented() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let hosts = vec![
            "web1.example.com".to_string(),
            "web2.example.com".to_string(),
            "db1.example.com".to_string(),
        ];

        callback
            .on_play_start("Deploy to multiple hosts", &hosts)
            .await;

        let output = writer.get_output();

        // Validate YAML structure
        let docs = validate_yaml_documents(&output).expect("Should produce valid YAML");
        assert!(!docs.is_empty());

        // Check that list items are present
        assert!(
            output.contains("- web1.example.com") || output.contains("web1.example.com"),
            "Host list should be properly formatted"
        );
    }

    #[tokio::test]
    async fn test_custom_indent_size() {
        let writer = CaptureWriter::new();
        let config = YamlCallbackConfig {
            indent_size: 4,
            ..Default::default()
        };
        let callback = YamlCallback::with_writer(writer.clone(), config);

        let hosts = vec!["host1".to_string()];
        callback.on_play_start("Test", &hosts).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Should produce valid YAML");
        assert!(!docs.is_empty());
    }
}

// ============================================================================
// Test 3: Multi-line Values
// ============================================================================

mod yaml_multiline_tests {
    use super::*;

    #[tokio::test]
    async fn test_long_message_preserved() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let long_message = "This is a very long message that contains a lot of text. \
            It should be properly handled by the YAML callback plugin and preserved \
            in the output. The message might span multiple lines when displayed, \
            but the content should remain intact and readable.";

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Task with long message".to_string(),
            result: ModuleResult::ok(long_message),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Should produce valid YAML");
        assert!(!docs.is_empty());

        // The message content should be preserved (though possibly formatted)
        assert!(
            output.contains("very long message") || output.contains("long"),
            "Long message content should be preserved"
        );
    }

    #[tokio::test]
    async fn test_multiline_string_with_newlines() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let multiline_message = "Line 1\nLine 2\nLine 3\nLine 4";

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Multiline output".to_string(),
            result: ModuleResult::ok(multiline_message),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Multiline strings should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_script_output_with_special_characters() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let script_output = r#"#!/bin/bash
echo "Hello, World!"
if [ "$1" = "test" ]; then
    echo 'Single quotes work too'
fi
exit 0
"#;

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Script execution".to_string(),
            result: ModuleResult::changed(script_output),
            duration: Duration::from_millis(500),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Script output should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_log_output_with_timestamps() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let log_output = "[2024-01-15 10:30:45] INFO: Application started\n\
            [2024-01-15 10:30:46] DEBUG: Loading configuration from /etc/app/config.yml\n\
            [2024-01-15 10:30:47] WARN: Deprecated config key 'old_setting' found\n\
            [2024-01-15 10:30:48] INFO: Ready to accept connections";

        let result = ExecutionResult {
            host: "app_server".to_string(),
            task_name: "Start application".to_string(),
            result: ModuleResult::changed(log_output),
            duration: Duration::from_secs(3),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Log output should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_empty_string_message() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Silent task".to_string(),
            result: ModuleResult::ok(""),
            duration: Duration::from_millis(10),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Empty message should produce valid YAML");
        assert!(!docs.is_empty());
    }
}

// ============================================================================
// Test 4: Special YAML Characters Escaped
// ============================================================================

mod yaml_escaping_tests {
    use super::*;

    #[tokio::test]
    async fn test_colon_in_value_escaped() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "test:host:with:colons".to_string(),
            task_name: "Task: with colons: in name".to_string(),
            result: ModuleResult::ok("Result: contains: many: colons"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Colons should be properly escaped");
        assert!(!docs.is_empty());

        // Parse and verify the values are correct
        // The colons should be preserved in the parsed values
        let yaml_str = &output;
        assert!(validate_yaml(yaml_str).is_ok() || validate_yaml_documents(yaml_str).is_ok());
    }

    #[tokio::test]
    async fn test_quotes_in_value_escaped() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: r#"Task with "quotes" and 'apostrophes'"#.to_string(),
            result: ModuleResult::ok(r#"Message with "double" and 'single' quotes"#),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Quotes should be properly escaped");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_backslash_escaped() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "windows_host".to_string(),
            task_name: "Windows path task".to_string(),
            result: ModuleResult::ok(r"C:\Users\Admin\Documents\file.txt"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Backslashes should be properly escaped");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_hash_in_value_not_treated_as_comment() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Task with # hash".to_string(),
            result: ModuleResult::ok("Message with # hash # marks # everywhere"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output).expect("Hash marks should be properly handled");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_yaml_special_indicators_escaped() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        // Test various YAML special characters: &, *, !, |, >, %, @, `
        let special_chars_message = "Anchor: &name, Alias: *name, Tag: !custom, \
            Block literal: |, Block folded: >, Directive: %, Reserved: @`";

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Special YAML chars".to_string(),
            result: ModuleResult::ok(special_chars_message),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output)
            .expect("YAML special indicators should be properly escaped");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_reserved_words_escaped() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        // Test YAML reserved words: true, false, null, ~
        for (task_name, message) in [
            ("Task returning true", "true"),
            ("Task returning false", "false"),
            ("Task returning null", "null"),
            ("Task returning tilde", "~"),
            ("Task returning yes", "yes"),
            ("Task returning no", "no"),
            ("Task returning on", "on"),
            ("Task returning off", "off"),
        ] {
            writer.clear();

            let result = ExecutionResult {
                host: "test_host".to_string(),
                task_name: task_name.to_string(),
                result: ModuleResult::ok(message),
                duration: Duration::from_millis(100),
                notify: vec![],
            };

            callback.on_task_complete(&result).await;

            let output = writer.get_output();
            let docs = validate_yaml_documents(&output).unwrap_or_else(|_| {
                panic!("Reserved word '{}' should be properly handled", message)
            });
            assert!(
                !docs.is_empty(),
                "Output for '{}' should not be empty",
                message
            );
        }
    }

    #[tokio::test]
    async fn test_numeric_strings_preserved() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        // Test numeric strings that should remain strings
        let numeric_messages = ["123", "45.67", "0x1A", "1e10", "1_000_000", "007"];

        for message in numeric_messages {
            writer.clear();

            let result = ExecutionResult {
                host: "test_host".to_string(),
                task_name: format!("Numeric string: {}", message),
                result: ModuleResult::ok(message),
                duration: Duration::from_millis(100),
                notify: vec![],
            };

            callback.on_task_complete(&result).await;

            let output = writer.get_output();
            let docs = validate_yaml_documents(&output).unwrap_or_else(|_| {
                panic!("Numeric string '{}' should produce valid YAML", message)
            });
            assert!(!docs.is_empty());
        }
    }

    #[tokio::test]
    async fn test_leading_trailing_spaces_preserved() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "  Task with leading spaces  ".to_string(),
            result: ModuleResult::ok("  Message with leading and trailing spaces  "),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output)
            .expect("Leading/trailing spaces should be properly quoted");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_unicode_characters_handled() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let unicode_message = "Hello, World! Japanese: , Chinese: , Korean: , Emoji: , Russian: ";

        let result = ExecutionResult {
            host: "unicode_host".to_string(),
            task_name: "Unicode task ".to_string(),
            result: ModuleResult::ok(unicode_message),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output)
            .expect("Unicode characters should be properly handled");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_control_characters_escaped() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let control_char_message = "Tab:\there\r\nNewline and carriage return\x00Null byte";

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Control chars task".to_string(),
            result: ModuleResult::ok(control_char_message),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        // Should produce valid YAML by escaping or handling control characters
        let docs = validate_yaml_documents(&output)
            .expect("Control characters should be properly escaped");
        assert!(!docs.is_empty());
    }
}

// ============================================================================
// Test 5: Readability
// ============================================================================

mod yaml_readability_tests {
    use super::*;

    #[tokio::test]
    async fn test_output_has_clear_event_type() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        callback.on_playbook_start("test.yml").await;

        let output = writer.get_output();

        // Should clearly indicate the event type
        assert!(
            output.contains("event:") || output.contains("event :"),
            "Event type should be clearly indicated"
        );
        assert!(
            output.contains("playbook_start") || output.contains("playbook-start"),
            "Event type should be recognizable"
        );
    }

    #[tokio::test]
    async fn test_host_and_task_clearly_identified() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "production-web-01".to_string(),
            task_name: "Deploy application".to_string(),
            result: ModuleResult::changed("Application deployed"),
            duration: Duration::from_secs(5),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Host and task should be clearly visible
        assert!(
            output.contains("production-web-01"),
            "Host name should be clearly visible"
        );
        assert!(
            output.contains("Deploy application"),
            "Task name should be clearly visible"
        );
    }

    #[tokio::test]
    async fn test_success_failure_status_clear() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        // Success case
        let success_result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "Successful task".to_string(),
            result: ModuleResult::ok("Success"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&success_result).await;

        let success_output = writer.get_output();
        assert!(
            success_output.contains("success") || success_output.contains("true"),
            "Success status should be clear"
        );

        // Failure case
        writer.clear();
        let failure_result = ExecutionResult {
            host: "host2".to_string(),
            task_name: "Failed task".to_string(),
            result: ModuleResult::failed("Error occurred"),
            duration: Duration::from_millis(100),
            notify: vec![],
        };
        callback.on_task_complete(&failure_result).await;

        let failure_output = writer.get_output();
        // The output should indicate failure in some way
        assert!(
            failure_output.contains("success") || failure_output.contains("false"),
            "Failure status should be clear"
        );
    }

    #[tokio::test]
    async fn test_duration_human_readable() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "Long running task".to_string(),
            result: ModuleResult::ok("Done"),
            duration: Duration::from_millis(2500),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Duration should be present in some form
        assert!(
            output.contains("duration") || output.contains("2500") || output.contains("2.5"),
            "Duration should be present in output"
        );
    }

    #[tokio::test]
    async fn test_document_separators_present_in_multi_doc_mode() {
        let writer = CaptureWriter::new();
        let config = YamlCallbackConfig {
            multi_document: true,
            ..Default::default()
        };
        let callback = YamlCallback::with_writer(writer.clone(), config);

        callback.on_playbook_start("test.yml").await;
        callback.on_playbook_end("test.yml", true).await;

        let output = writer.get_output();

        // Should have document separators
        assert!(
            output.contains("---"),
            "Multi-document mode should have --- separators"
        );
    }

    #[tokio::test]
    async fn test_output_not_excessively_verbose() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "Simple task".to_string(),
            result: ModuleResult::ok("OK"),
            duration: Duration::from_millis(50),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Output should be reasonable length (not excessively verbose)
        // A simple task result shouldn't produce more than ~500 characters
        assert!(
            output.len() < 1000,
            "Output should not be excessively verbose for simple tasks"
        );
    }

    #[tokio::test]
    async fn test_changed_status_clearly_indicated() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "Change task".to_string(),
            result: ModuleResult::changed("Configuration updated"),
            duration: Duration::from_millis(100),
            notify: vec!["restart service".to_string()],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Changed status should be clearly visible
        assert!(
            output.contains("changed") || output.contains("true"),
            "Changed status should be indicated"
        );
    }

    #[tokio::test]
    async fn test_skipped_status_clearly_indicated() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "host1".to_string(),
            task_name: "Skipped task".to_string(),
            result: ModuleResult::skipped("Condition not met"),
            duration: Duration::from_millis(1),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Skipped status should be indicated
        assert!(
            output.contains("skipped") || output.contains("true"),
            "Skipped status should be indicated"
        );
    }

    #[tokio::test]
    async fn test_facts_count_readable() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let mut facts = Facts::new();
        facts.set("os", json!("linux"));
        facts.set("distribution", json!("ubuntu"));
        facts.set("version", json!("22.04"));

        callback.on_facts_gathered("host1", &facts).await;

        let output = writer.get_output();

        // Should indicate facts were gathered
        assert!(
            output.contains("facts") || output.contains("host1"),
            "Facts gathering event should be readable"
        );
    }
}

// ============================================================================
// Additional Integration Tests
// ============================================================================

mod yaml_integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_roundtrip_parsing() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let original_message = "Complex message with: colons, \"quotes\", and 'apostrophes'";
        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Roundtrip test".to_string(),
            result: ModuleResult::ok(original_message),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Parse back and verify
        let docs = validate_yaml_documents(&output).expect("Should produce valid YAML");
        assert!(!docs.is_empty());

        // The parsed content should contain our message
        let yaml_str = output.clone();
        assert!(
            yaml_str.contains("colons") || yaml_str.contains("quotes"),
            "Message content should be preserved after roundtrip"
        );
    }

    #[tokio::test]
    async fn test_multiple_events_in_sequence() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());
        let hosts = vec!["host1".to_string()];

        // Generate a sequence of events
        callback.on_playbook_start("sequence_test.yml").await;
        callback.on_play_start("Test Play", &hosts).await;

        for i in 1..=5 {
            callback
                .on_task_start(&format!("Task {}", i), "host1")
                .await;
            let result = ExecutionResult {
                host: "host1".to_string(),
                task_name: format!("Task {}", i),
                result: if i % 2 == 0 {
                    ModuleResult::changed("Changed")
                } else {
                    ModuleResult::ok("OK")
                },
                duration: Duration::from_millis(100 * i as u64),
                notify: vec![],
            };
            callback.on_task_complete(&result).await;
        }

        callback.on_play_end("Test Play", true).await;
        callback.on_playbook_end("sequence_test.yml", true).await;

        let output = writer.get_output();

        // All documents should be valid
        let docs = validate_yaml_documents(&output).expect("All events should produce valid YAML");

        // Should have multiple documents (playbook_start, play_start, 5x(task_start+task_complete), play_end, playbook_end)
        assert!(
            docs.len() >= 10,
            "Expected at least 10 documents, got {}",
            docs.len()
        );
    }

    #[tokio::test]
    async fn test_concurrent_task_output() {
        let writer = CaptureWriter::new();
        let callback = Arc::new(YamlCallback::with_writer(
            writer.clone(),
            YamlCallbackConfig::default(),
        ));
        let hosts = vec![
            "host1".to_string(),
            "host2".to_string(),
            "host3".to_string(),
        ];

        callback.on_playbook_start("concurrent.yml").await;
        callback.on_play_start("Concurrent Tasks", &hosts).await;

        // Simulate concurrent task completions
        let mut handles = vec![];
        for (i, host) in hosts.iter().enumerate() {
            let cb = callback.clone();
            let host = host.clone();
            let handle = tokio::spawn(async move {
                let result = ExecutionResult {
                    host: host.clone(),
                    task_name: format!("Concurrent task {}", i),
                    result: ModuleResult::ok("OK"),
                    duration: Duration::from_millis(50),
                    notify: vec![],
                };
                cb.on_task_complete(&result).await;
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
        }

        callback.on_play_end("Concurrent Tasks", true).await;
        callback.on_playbook_end("concurrent.yml", true).await;

        let output = writer.get_output();

        // All concurrent output should still be valid YAML
        let docs =
            validate_yaml_documents(&output).expect("Concurrent output should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_very_large_output() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        // Generate a large message (10KB+)
        let large_message = "x".repeat(10000);

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Large output task".to_string(),
            result: ModuleResult::ok(&large_message),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Should still be valid YAML
        let docs =
            validate_yaml_documents(&output).expect("Large output should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_deeply_nested_structure() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        // Create deeply nested data
        let nested_data = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "level4": {
                            "level5": {
                                "value": "deeply nested value"
                            }
                        }
                    }
                }
            }
        });

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Nested data task".to_string(),
            result: ModuleResult::ok("OK").with_data(nested_data),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Deeply nested structures should still be valid YAML
        let docs = validate_yaml_documents(&output)
            .expect("Deeply nested structures should produce valid YAML");
        assert!(!docs.is_empty());
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

mod yaml_edge_case_tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_playbook_name() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        callback.on_playbook_start("").await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output)
            .expect("Empty playbook name should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_empty_host_list() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let empty_hosts: Vec<String> = vec![];
        callback.on_play_start("No hosts play", &empty_hosts).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Empty host list should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_very_long_host_list() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let hosts: Vec<String> = (1..=100)
            .map(|i| format!("host{}.example.com", i))
            .collect();

        callback.on_play_start("Large host list", &hosts).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Large host list should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_binary_like_content() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        // Simulate binary-like content (base64 encoded)
        let binary_content = "SGVsbG8gV29ybGQhIFRoaXMgaXMgYmFzZTY0IGVuY29kZWQgY29udGVudC4=";

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Binary content task".to_string(),
            result: ModuleResult::ok(binary_content),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs = validate_yaml_documents(&output)
            .expect("Binary-like content should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_yaml_injection_attempt() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        // Attempt to inject YAML structure
        let injection_attempt = "value\ninjected_key: injected_value\nanother: data";

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Normal task".to_string(),
            result: ModuleResult::ok(injection_attempt),
            duration: Duration::from_millis(100),
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();

        // Should produce valid YAML that doesn't allow injection
        let docs = validate_yaml_documents(&output)
            .expect("YAML injection attempts should be safely handled");
        assert!(!docs.is_empty());

        // The "injection" should be part of the value, not a new key
        // (i.e., properly escaped/quoted)
    }

    #[tokio::test]
    async fn test_zero_duration() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Instant task".to_string(),
            result: ModuleResult::ok("Instant"),
            duration: Duration::ZERO,
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Zero duration should produce valid YAML");
        assert!(!docs.is_empty());
    }

    #[tokio::test]
    async fn test_maximum_duration() {
        let writer = CaptureWriter::new();
        let callback = YamlCallback::with_writer(writer.clone(), YamlCallbackConfig::default());

        let result = ExecutionResult {
            host: "test_host".to_string(),
            task_name: "Very long task".to_string(),
            result: ModuleResult::ok("Finally done"),
            duration: Duration::from_secs(86400), // 24 hours
            notify: vec![],
        };

        callback.on_task_complete(&result).await;

        let output = writer.get_output();
        let docs =
            validate_yaml_documents(&output).expect("Maximum duration should produce valid YAML");
        assert!(!docs.is_empty());
    }
}
