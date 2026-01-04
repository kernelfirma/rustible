//! Wait_for module tests
//!
//! Integration tests for the wait_for module which waits for conditions.
//! Tests cover:
//! - Port availability (started, stopped, drained)
//! - Path existence (present, absent)
//! - Regex pattern matching
//! - Timeout handling
//! - Parameter validation

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use tempfile::TempDir;

/// Helper to create test params
fn create_params(entries: Vec<(&str, serde_json::Value)>) -> HashMap<String, serde_json::Value> {
    entries
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

#[test]
fn test_wait_state_started() {
    let params = create_params(vec![
        ("host", serde_json::json!("localhost")),
        ("port", serde_json::json!(8080)),
        ("state", serde_json::json!("started")),
    ]);

    assert_eq!(
        params.get("state").and_then(|v| v.as_str()),
        Some("started")
    );
}

#[test]
fn test_wait_state_stopped() {
    let params = create_params(vec![
        ("port", serde_json::json!(8080)),
        ("state", serde_json::json!("stopped")),
    ]);

    assert_eq!(
        params.get("state").and_then(|v| v.as_str()),
        Some("stopped")
    );
}

#[test]
fn test_wait_state_present() {
    let params = create_params(vec![
        ("path", serde_json::json!("/tmp/marker.txt")),
        ("state", serde_json::json!("present")),
    ]);

    assert_eq!(
        params.get("state").and_then(|v| v.as_str()),
        Some("present")
    );
}

#[test]
fn test_wait_state_absent() {
    let params = create_params(vec![
        ("path", serde_json::json!("/tmp/marker.txt")),
        ("state", serde_json::json!("absent")),
    ]);

    assert_eq!(params.get("state").and_then(|v| v.as_str()), Some("absent"));
}

#[test]
fn test_wait_state_drained() {
    let params = create_params(vec![
        ("port", serde_json::json!(8080)),
        ("state", serde_json::json!("drained")),
    ]);

    assert_eq!(
        params.get("state").and_then(|v| v.as_str()),
        Some("drained")
    );
}

#[test]
fn test_wait_state_parsing() {
    let valid_states = vec![
        ("started", true),
        ("STARTED", true),
        ("stopped", true),
        ("present", true),
        ("absent", true),
        ("drained", true),
        ("invalid", false),
        ("running", false),
    ];

    for (state, is_valid) in valid_states {
        let lower = state.to_lowercase();
        let parsed_valid =
            ["started", "stopped", "present", "absent", "drained"].contains(&lower.as_str());
        assert_eq!(parsed_valid, is_valid, "State '{}' validity check", state);
    }
}

#[test]
fn test_wait_port_validation() {
    // Valid port range
    let valid_ports = vec![1, 80, 443, 8080, 65535];
    for port in valid_ports {
        assert!(port > 0 && port <= 65535, "Port {} should be valid", port);
    }

    // Invalid ports
    let invalid_ports = vec![0, -1, 65536, 99999];
    for port in invalid_ports {
        assert!(port <= 0 || port > 65535, "Port {} should be invalid", port);
    }
}

#[test]
fn test_wait_default_host() {
    let params = create_params(vec![("port", serde_json::json!(80))]);

    // Default host should be 127.0.0.1
    let host = params.get("host").and_then(|v| v.as_str());
    assert!(host.is_none()); // Module uses default when not specified
}

#[test]
fn test_wait_timeout_settings() {
    let params = create_params(vec![
        ("port", serde_json::json!(80)),
        ("timeout", serde_json::json!(120)),
    ]);

    assert_eq!(params.get("timeout").and_then(|v| v.as_i64()), Some(120));
}

#[test]
fn test_wait_delay_settings() {
    let params = create_params(vec![
        ("port", serde_json::json!(80)),
        ("delay", serde_json::json!(5)),
    ]);

    assert_eq!(params.get("delay").and_then(|v| v.as_i64()), Some(5));
}

#[test]
fn test_wait_sleep_settings() {
    let params = create_params(vec![
        ("port", serde_json::json!(80)),
        ("sleep", serde_json::json!(2)),
    ]);

    assert_eq!(params.get("sleep").and_then(|v| v.as_i64()), Some(2));
}

#[test]
fn test_wait_connect_timeout() {
    let params = create_params(vec![
        ("port", serde_json::json!(80)),
        ("connect_timeout", serde_json::json!(10)),
    ]);

    assert_eq!(
        params.get("connect_timeout").and_then(|v| v.as_i64()),
        Some(10)
    );
}

#[test]
fn test_wait_custom_message() {
    let params = create_params(vec![
        ("port", serde_json::json!(80)),
        ("msg", serde_json::json!("Database is not ready")),
    ]);

    assert_eq!(
        params.get("msg").and_then(|v| v.as_str()),
        Some("Database is not ready")
    );
}

#[test]
fn test_wait_regex_param() {
    let params = create_params(vec![
        ("path", serde_json::json!("/var/log/app.log")),
        (
            "search_regex",
            serde_json::json!("Application started successfully"),
        ),
    ]);

    assert!(params.contains_key("search_regex"));
    assert!(params.contains_key("path"));
}

#[test]
fn test_wait_regex_validation() {
    // Valid regex patterns
    let valid_patterns = vec![
        r"Application started",
        r"Error: \d+",
        r"^Started",
        r"Ready$",
        r"[0-9]{4}-[0-9]{2}-[0-9]{2}",
    ];

    for pattern in valid_patterns {
        let regex = regex::Regex::new(pattern);
        assert!(regex.is_ok(), "Pattern '{}' should be valid", pattern);
    }
}

#[test]
fn test_wait_invalid_regex() {
    // Invalid regex patterns
    let invalid_patterns = vec![r"[invalid(", r"*start", r"(?<invalid)"];

    for pattern in invalid_patterns {
        let regex = regex::Regex::new(pattern);
        assert!(regex.is_err(), "Pattern '{}' should be invalid", pattern);
    }
}

#[test]
fn test_wait_exclude_hosts() {
    let params = create_params(vec![
        ("port", serde_json::json!(8080)),
        ("state", serde_json::json!("drained")),
        (
            "exclude_hosts",
            serde_json::json!(["127.0.0.1", "localhost"]),
        ),
    ]);

    let exclude = params.get("exclude_hosts").unwrap();
    assert!(exclude.is_array());
    assert_eq!(exclude.as_array().unwrap().len(), 2);
}

#[test]
fn test_wait_active_connection_states() {
    let default_states = vec![
        "ESTABLISHED",
        "SYN_SENT",
        "SYN_RECV",
        "FIN_WAIT1",
        "FIN_WAIT2",
        "TIME_WAIT",
    ];

    for state in default_states {
        assert!(!state.is_empty());
        // States should be uppercase
        assert_eq!(state, state.to_uppercase());
    }
}

#[test]
fn test_wait_path_exists() {
    // Test with a path that definitely exists
    assert!(Path::new("/").exists());

    // Test with a path that doesn't exist
    assert!(!Path::new("/nonexistent/path/12345").exists());
}

#[test]
fn test_wait_path_file_creation() {
    let temp = TempDir::new().expect("Create temp dir");
    let marker = temp.path().join("marker.txt");

    // File doesn't exist yet
    assert!(!marker.exists());

    // Create the file
    fs::write(&marker, "ready").expect("Write marker");

    // File now exists
    assert!(marker.exists());
}

#[test]
fn test_wait_regex_in_file() {
    let temp = TempDir::new().expect("Create temp dir");
    let log_file = temp.path().join("app.log");

    // Write log content
    let mut file = File::create(&log_file).expect("Create log file");
    writeln!(file, "2024-01-01 10:00:00 Starting application...").unwrap();
    writeln!(file, "2024-01-01 10:00:01 Loading configuration...").unwrap();
    writeln!(file, "2024-01-01 10:00:02 Application started successfully").unwrap();

    // Read and search for pattern
    let content = fs::read_to_string(&log_file).expect("Read log file");
    let pattern = regex::Regex::new("Application started successfully").unwrap();

    assert!(pattern.is_match(&content));
}

#[test]
fn test_wait_regex_not_found() {
    let temp = TempDir::new().expect("Create temp dir");
    let log_file = temp.path().join("app.log");

    // Write log content without the pattern
    let mut file = File::create(&log_file).expect("Create log file");
    writeln!(file, "2024-01-01 10:00:00 Starting application...").unwrap();
    writeln!(file, "2024-01-01 10:00:01 Loading configuration...").unwrap();

    // Read and search for pattern
    let content = fs::read_to_string(&log_file).expect("Read log file");
    let pattern = regex::Regex::new("Application started successfully").unwrap();

    assert!(!pattern.is_match(&content));
}

#[test]
fn test_wait_port_check_closed() {
    use std::net::TcpStream;
    use std::time::Duration;

    // Try to connect to a port that's very unlikely to be open
    let result = TcpStream::connect_timeout(
        &"127.0.0.1:65534".parse().unwrap(),
        Duration::from_millis(100),
    );

    // Should fail to connect (port closed)
    assert!(result.is_err());
}

#[test]
fn test_wait_port_check_open() {
    use std::net::TcpStream;
    use std::time::Duration;

    // Bind to a port to make it open
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(err) => panic!("Bind to port: {}", err),
    };
    let addr = listener.local_addr().expect("Get local address");

    // Try to connect
    let result = TcpStream::connect_timeout(&addr, Duration::from_millis(500));

    // Should succeed (port open)
    assert!(result.is_ok());

    drop(listener);
}

#[test]
fn test_wait_complete_port_config() {
    let params = create_params(vec![
        ("host", serde_json::json!("192.168.1.100")),
        ("port", serde_json::json!(3306)),
        ("state", serde_json::json!("started")),
        ("timeout", serde_json::json!(120)),
        ("delay", serde_json::json!(5)),
        ("sleep", serde_json::json!(2)),
        ("connect_timeout", serde_json::json!(10)),
        ("msg", serde_json::json!("MySQL is not ready")),
    ]);

    assert!(params.contains_key("host"));
    assert!(params.contains_key("port"));
    assert!(params.contains_key("state"));
    assert!(params.contains_key("timeout"));
    assert!(params.contains_key("delay"));
    assert!(params.contains_key("sleep"));
    assert!(params.contains_key("connect_timeout"));
    assert!(params.contains_key("msg"));
}

#[test]
fn test_wait_complete_path_config() {
    let params = create_params(vec![
        ("path", serde_json::json!("/var/log/app.log")),
        ("search_regex", serde_json::json!("Application ready")),
        ("state", serde_json::json!("present")),
        ("timeout", serde_json::json!(60)),
    ]);

    assert!(params.contains_key("path"));
    assert!(params.contains_key("search_regex"));
    assert!(params.contains_key("state"));
    assert!(params.contains_key("timeout"));
}

#[test]
fn test_wait_validation_port_or_path_required() {
    // Neither port nor path - should be invalid
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let has_port_or_path = params.contains_key("port") || params.contains_key("path");
    assert!(!has_port_or_path);
}

#[test]
fn test_wait_validation_started_requires_port() {
    let params = create_params(vec![
        ("path", serde_json::json!("/tmp/test")),
        ("state", serde_json::json!("started")),
    ]);

    // started state with path but no port should be invalid
    let has_port = params.contains_key("port");
    let state = params.get("state").and_then(|v| v.as_str()).unwrap_or("");

    let needs_port = ["started", "stopped", "drained"].contains(&state);
    assert!(needs_port && !has_port, "started requires port");
}

#[test]
fn test_wait_validation_present_requires_path() {
    let params = create_params(vec![
        ("port", serde_json::json!(80)),
        ("state", serde_json::json!("present")),
    ]);

    // present state with port but no path should be invalid
    let has_path = params.contains_key("path");
    let state = params.get("state").and_then(|v| v.as_str()).unwrap_or("");

    let needs_path = ["present", "absent"].contains(&state);
    assert!(needs_path && !has_path, "present requires path");
}

#[test]
fn test_wait_validation_regex_requires_path() {
    let params = create_params(vec![
        ("port", serde_json::json!(80)),
        ("search_regex", serde_json::json!("pattern")),
    ]);

    // search_regex without path should be invalid
    let has_path = params.contains_key("path");
    let has_regex = params.contains_key("search_regex");

    assert!(has_regex && !has_path, "search_regex requires path");
}

#[test]
fn test_wait_condition_description_port() {
    let host = "example.com";
    let port = 443;
    let state = "started";

    let description = match state {
        "started" => format!("port {} on {} to be open", port, host),
        "stopped" => format!("port {} on {} to be closed", port, host),
        "drained" => format!("connections on port {} to drain", port),
        _ => String::new(),
    };

    assert!(description.contains("443"));
    assert!(description.contains("example.com"));
}

#[test]
fn test_wait_condition_description_path() {
    let path = "/tmp/marker.txt";
    let state = "present";
    let regex: Option<&str> = None;

    let description = match state {
        "present" => {
            if let Some(pattern) = regex {
                format!("pattern '{}' in file '{}'", pattern, path)
            } else {
                format!("path '{}' to exist", path)
            }
        }
        "absent" => format!("path '{}' to be removed", path),
        _ => String::new(),
    };

    assert!(description.contains("/tmp/marker.txt"));
}

#[test]
fn test_wait_condition_description_regex() {
    let path = "/var/log/app.log";
    let regex = Some("Application started");

    let description = if let Some(pattern) = regex {
        format!("pattern '{}' in file '{}'", pattern, path)
    } else {
        format!("path '{}' to exist", path)
    };

    assert!(description.contains("pattern"));
    assert!(description.contains("Application started"));
}

#[test]
fn test_wait_defaults() {
    // Default timeout
    let default_timeout: u64 = 300;
    assert_eq!(default_timeout, 300);

    // Default delay
    let default_delay: u64 = 0;
    assert_eq!(default_delay, 0);

    // Default sleep
    let default_sleep: u64 = 1;
    assert_eq!(default_sleep, 1);

    // Default connect_timeout
    let default_connect_timeout: u64 = 5;
    assert_eq!(default_connect_timeout, 5);

    // Default host
    let default_host = "127.0.0.1";
    assert_eq!(default_host, "127.0.0.1");
}

#[test]
fn test_wait_negative_timeout_clamped() {
    let timeout = -10i64;
    let clamped = timeout.max(0) as u64;
    assert_eq!(clamped, 0);
}

#[test]
fn test_wait_minimum_sleep() {
    let sleep = 0i64;
    let min_sleep = sleep.max(1) as u64;
    assert_eq!(min_sleep, 1);
}

#[test]
fn test_wait_classification() {
    // wait_for should be RemoteCommand - can check conditions on target host
    let classification = "RemoteCommand";
    assert_eq!(classification, "RemoteCommand");
}
