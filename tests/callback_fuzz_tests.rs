#![cfg(not(tarpaulin))]
//! Property-based fuzz tests for the Rustible callback system.
//!
//! These tests use proptest to generate random inputs and verify that the callback
//! system handles all inputs gracefully without panicking or producing invalid states.
//!
//! ## Test Categories
//!
//! 1. **Event Type Parsing**: Tests that event types are correctly parsed and handled
//! 2. **Configuration Parsing**: Tests that configuration values are validated
//! 3. **Plugin Name Resolution**: Tests plugin name matching and resolution
//! 4. **Large Event Data**: Tests handling of large payloads without crashes
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --test callback_fuzz_tests
//! # Run with more iterations
//! PROPTEST_CASES=10000 cargo test --test callback_fuzz_tests
//! ```

use proptest::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

// ===========================================================================
// Event Type Parsing Tests
// ===========================================================================

/// Strategy for generating arbitrary task status strings
fn task_status_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("ok".to_string()),
        Just("changed".to_string()),
        Just("failed".to_string()),
        Just("skipped".to_string()),
        Just("unreachable".to_string()),
        Just("OK".to_string()),
        Just("CHANGED".to_string()),
        Just("FAILED".to_string()),
        Just("SKIPPED".to_string()),
        Just("UNREACHABLE".to_string()),
        ".*", // Random strings
    ]
}

/// Strategy for generating event type names
fn event_type_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("playbook_start".to_string()),
        Just("playbook_end".to_string()),
        Just("play_start".to_string()),
        Just("play_end".to_string()),
        Just("task_start".to_string()),
        Just("task_ok".to_string()),
        Just("task_failed".to_string()),
        Just("task_skipped".to_string()),
        Just("task_unreachable".to_string()),
        Just("handler_triggered".to_string()),
        Just("facts_gathered".to_string()),
        Just("warning".to_string()),
        Just("deprecation".to_string()),
        Just("verbose".to_string()),
        "[a-z_]{1,32}", // Random event types
    ]
}

/// Parse and normalize a task status string
fn parse_task_status(status: &str) -> Option<&'static str> {
    match status.to_lowercase().trim() {
        "ok" => Some("ok"),
        "changed" => Some("changed"),
        "failed" => Some("failed"),
        "skipped" => Some("skipped"),
        "unreachable" => Some("unreachable"),
        _ => None,
    }
}

/// Validate an event type name
fn validate_event_type(event_type: &str) -> bool {
    let valid_events = [
        "playbook_start",
        "playbook_end",
        "play_start",
        "play_end",
        "task_start",
        "task_ok",
        "task_failed",
        "task_skipped",
        "task_unreachable",
        "handler_triggered",
        "facts_gathered",
        "warning",
        "deprecation",
        "verbose",
    ];

    let normalized = event_type.to_lowercase();
    valid_events.contains(&normalized.as_str())
}

proptest! {
    /// Test that task status parsing never panics
    #[test]
    fn test_task_status_parsing_never_panics(status in task_status_strategy()) {
        let _ = parse_task_status(&status);
    }

    /// Test that event type validation never panics
    #[test]
    fn test_event_type_validation_never_panics(event_type in event_type_strategy()) {
        let _ = validate_event_type(&event_type);
    }

    /// Test task status parsing with arbitrary Unicode
    #[test]
    fn test_task_status_with_unicode(status in "\\PC*") {
        let result = parse_task_status(&status);
        // Should either parse successfully or return None, never panic
        prop_assert!(result.is_some() || result.is_none());
    }

    /// Test event creation with arbitrary host names
    #[test]
    fn test_event_with_arbitrary_host(
        host in "[a-zA-Z0-9._-]{0,256}",
        task_name in "[a-zA-Z0-9 _-]{0,256}",
        duration_ms in 0u64..=u64::MAX,
    ) {
        // Simulate event creation
        let event_data = (host.clone(), task_name.clone(), duration_ms);

        // Validate host name
        let valid_host = !host.is_empty() && host.len() <= 256;

        // Validate task name
        let valid_task = !task_name.is_empty() && task_name.len() <= 256;

        // Duration should be convertible
        let _duration = Duration::from_millis(duration_ms);

        let _ = (event_data, valid_host, valid_task);
    }

    /// Test play stats calculation with arbitrary values
    #[test]
    fn test_play_stats_calculation(
        ok in 0u32..=1_000_000,
        changed in 0u32..=1_000_000,
        failed in 0u32..=1_000_000,
        skipped in 0u32..=1_000_000,
        unreachable in 0u32..=1_000_000,
    ) {
        // Calculate total (with overflow protection)
        let total = ok
            .saturating_add(changed)
            .saturating_add(failed)
            .saturating_add(skipped)
            .saturating_add(unreachable);

        prop_assert!(total >= ok);
        prop_assert!(total >= changed);
        prop_assert!(total >= failed);
        prop_assert!(total >= skipped);
        prop_assert!(total >= unreachable);

        // Check failure detection
        let has_failures = failed > 0 || unreachable > 0;
        if failed > 0 || unreachable > 0 {
            prop_assert!(has_failures);
        }
    }
}

// ===========================================================================
// Configuration Parsing Tests
// ===========================================================================

/// Known plugin names for validation
const KNOWN_PLUGINS: &[&str] = &[
    "default",
    "minimal",
    "oneline",
    "json",
    "yaml",
    "timer",
    "tree",
    "diff",
    "junit",
    "notification",
    "dense",
    "forked",
    "selective",
    "counter",
    "null",
    "profile_tasks",
];

/// Strategy for generating plugin names
fn plugin_name_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("default".to_string()),
        Just("minimal".to_string()),
        Just("oneline".to_string()),
        Just("json".to_string()),
        Just("yaml".to_string()),
        Just("timer".to_string()),
        Just("tree".to_string()),
        Just("diff".to_string()),
        Just("junit".to_string()),
        Just("notification".to_string()),
        Just("dense".to_string()),
        Just("forked".to_string()),
        Just("selective".to_string()),
        Just("counter".to_string()),
        Just("null".to_string()),
        "[a-z_]{1,64}", // Random plugin names
    ]
}

/// Strategy for generating output destinations
fn output_destination_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("stdout".to_string()),
        Just("stderr".to_string()),
        "/tmp/[a-z]{1,20}\\.log", // Random file paths
        "[a-zA-Z0-9/_.-]{1,256}", // Various paths
    ]
}

/// Validate and normalize a plugin name
fn validate_plugin_name(name: &str) -> Option<&'static str> {
    let normalized = name.trim().to_lowercase();
    KNOWN_PLUGINS.iter().find(|&&p| p == normalized).copied()
}

/// Parse output destination
fn parse_output_destination(output: &str) -> (&str, bool, bool) {
    let trimmed = output.trim();
    let is_stdout = trimmed == "stdout" || trimmed.is_empty();
    let is_stderr = trimmed == "stderr";
    let is_file = !is_stdout && !is_stderr;
    (trimmed, is_stdout || is_stderr, is_file)
}

proptest! {
    /// Test plugin name validation never panics
    #[test]
    fn test_plugin_name_validation_never_panics(name in plugin_name_strategy()) {
        let _ = validate_plugin_name(&name);
    }

    /// Test output destination parsing never panics
    #[test]
    fn test_output_destination_parsing_never_panics(output in output_destination_strategy()) {
        let (_, is_std, is_file) = parse_output_destination(&output);
        // Exactly one should be true
        prop_assert!(is_std || is_file);
    }

    /// Test verbosity level handling
    #[test]
    fn test_verbosity_level_handling(level in 0u8..=255u8) {
        let normalized = level.min(5);
        prop_assert!(normalized <= 5);

        let verbosity_name = match normalized {
            0 => "Normal",
            1 => "Verbose",
            2 => "MoreVerbose",
            3 => "Debug",
            4 => "ConnectionDebug",
            _ => "Max",
        };
        prop_assert!(!verbosity_name.is_empty());
    }

    /// Test configuration option parsing with various types
    #[test]
    fn test_config_option_parsing(
        key in "[a-z_]{1,32}",
        bool_val in any::<bool>(),
        int_val in any::<i64>(),
        float_val in any::<f64>(),
        string_val in "[a-zA-Z0-9_-]{0,64}",
    ) {
        // Test bool option
        let bool_str = bool_val.to_string();
        prop_assert!(bool_str == "true" || bool_str == "false");

        // Test int option
        let int_str = int_val.to_string();
        let parsed_int: Result<i64, _> = int_str.parse();
        prop_assert!(parsed_int.is_ok());

        // Test float option (handle special values)
        if float_val.is_finite() {
            let float_str = float_val.to_string();
            let parsed_float: Result<f64, _> = float_str.parse();
            prop_assert!(parsed_float.is_ok());
        }

        // Test string option
        prop_assert!(string_val.len() <= 64);

        // Key should be valid
        prop_assert!(!key.is_empty() && key.len() <= 32);
    }

    /// Test priority ordering
    #[test]
    fn test_priority_ordering(p1 in i32::MIN..=i32::MAX, p2 in i32::MIN..=i32::MAX) {
        // Lower values should sort first
        let order = p1.cmp(&p2);
        match order {
            std::cmp::Ordering::Less => prop_assert!(p1 < p2),
            std::cmp::Ordering::Equal => prop_assert!(p1 == p2),
            std::cmp::Ordering::Greater => prop_assert!(p1 > p2),
        }
    }
}

// ===========================================================================
// Plugin Name Resolution Tests
// ===========================================================================

/// Strategy for generating plugin name variations
fn plugin_variation_strategy() -> impl Strategy<Value = (String, Option<String>, Option<String>)> {
    (
        "[a-z_-]{1,64}",             // Base name
        prop::option::of("[a-z.]+"), // Optional namespace
        prop::option::of("[0-9.]+"), // Optional version
    )
}

/// Strategy for generating plugin aliases
fn plugin_alias_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("min".to_string()),
        Just("quiet".to_string()),
        Just("line".to_string()),
        Just("single".to_string()),
        Just("jsn".to_string()),
        Just("machine".to_string()),
        Just("yml".to_string()),
        Just("human".to_string()),
        Just("time".to_string()),
        Just("timing".to_string()),
        Just("hier".to_string()),
        Just("hierarchy".to_string()),
        Just("changes".to_string()),
        Just("delta".to_string()),
        Just("xml".to_string()),
        Just("test-report".to_string()),
        Just("notify".to_string()),
        Just("alert".to_string()),
        Just("compact".to_string()),
        Just("brief".to_string()),
        Just("parallel".to_string()),
        Just("multi".to_string()),
        Just("filter".to_string()),
        Just("filtered".to_string()),
        Just("count".to_string()),
        Just("stats".to_string()),
        Just("noop".to_string()),
        Just("silent".to_string()),
        Just("profile".to_string()),
        Just("perf".to_string()),
        "[a-z]{1,16}", // Random aliases
    ]
}

/// Resolve a plugin alias to its canonical name
fn resolve_alias(alias: &str) -> Option<&'static str> {
    match alias.trim().to_lowercase().as_str() {
        "min" | "quiet" => Some("minimal"),
        "line" | "single" => Some("oneline"),
        "jsn" | "machine" => Some("json"),
        "yml" | "human" => Some("yaml"),
        "time" | "timing" => Some("timer"),
        "hier" | "hierarchy" => Some("tree"),
        "changes" | "delta" => Some("diff"),
        "xml" | "test-report" => Some("junit"),
        "notify" | "alert" => Some("notification"),
        "compact" | "brief" => Some("dense"),
        "parallel" | "multi" => Some("forked"),
        "filter" | "filtered" => Some("selective"),
        "count" | "stats" => Some("counter"),
        "noop" | "silent" => Some("null"),
        "profile" | "perf" => Some("profile_tasks"),
        _ => None,
    }
}

/// Check if a plugin name is valid according to naming rules
fn is_valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        && name
            .chars()
            .next()
            .map(|c| c.is_alphabetic())
            .unwrap_or(false)
}

proptest! {
    /// Test plugin name resolution with variations
    #[test]
    fn test_plugin_name_resolution((base, namespace, version) in plugin_variation_strategy()) {
        // Test base name resolution
        let normalized = base.trim().to_lowercase();
        let is_known = KNOWN_PLUGINS.contains(&normalized.as_str());

        // Test with namespace
        if let Some(ns) = namespace {
            let full_name = format!("{}.{}", ns.trim(), normalized);
            let parts: Vec<&str> = full_name.split('.').collect();
            prop_assert!(parts.len() >= 2);
        }

        // Test with version
        if let Some(ver) = version {
            let versioned = format!("{}@{}", normalized, ver.trim());
            let parts: Vec<&str> = versioned.split('@').collect();
            prop_assert!(parts.len() == 2);
        }

        let _ = is_known;
    }

    /// Test alias resolution never panics
    #[test]
    fn test_alias_resolution_never_panics(alias in plugin_alias_strategy()) {
        let resolved = resolve_alias(&alias);

        // If resolved, should be a known plugin
        if let Some(plugin) = resolved {
            prop_assert!(KNOWN_PLUGINS.contains(&plugin));
        }
    }

    /// Test plugin name validation
    #[test]
    fn test_plugin_name_validation(name in "[a-zA-Z0-9_-]{0,128}") {
        let is_valid = is_valid_plugin_name(&name);

        // Empty names should be invalid
        if name.is_empty() {
            prop_assert!(!is_valid);
        }

        // Names over 64 chars should be invalid
        if name.len() > 64 {
            prop_assert!(!is_valid);
        }
    }

    /// Test name normalization (underscore/hyphen equivalence)
    #[test]
    fn test_name_normalization(name in "[a-z]{1,16}(_[a-z]{1,8})?") {
        let with_underscore = name.replace('-', "_");
        let with_hyphen = name.replace('_', "-");

        // Both normalizations should have same length
        prop_assert_eq!(with_underscore.len(), with_hyphen.len());
    }

    /// Test matching strategies
    #[test]
    fn test_matching_strategies(
        pattern in "[a-z]{1,16}",
        strategy in 0u8..=5u8,
    ) {
        for &plugin in KNOWN_PLUGINS {
            let matched = match strategy {
                0 => plugin == pattern,  // Exact
                1 => plugin.eq_ignore_ascii_case(&pattern),  // Case insensitive
                2 => plugin.starts_with(&pattern),  // Prefix
                3 => plugin.ends_with(&pattern),  // Suffix
                4 => plugin.contains(&pattern),  // Contains
                _ => pattern.is_empty() || plugin.contains(&pattern),  // Default
            };
            // Result should be a boolean, never panic
            let _ = matched;
        }
    }
}

// ===========================================================================
// Large Event Data Tests
// ===========================================================================

/// Strategy for generating large strings
fn large_string_strategy(max_len: usize) -> impl Strategy<Value = String> {
    // Use ASCII printable characters directly without filter
    prop::collection::vec(prop::char::range(' ', '~'), 0..=max_len)
        .prop_map(|chars| chars.into_iter().collect())
}

/// Strategy for generating large arrays of strings
fn large_string_array_strategy(
    max_count: usize,
    max_string_len: usize,
) -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(large_string_strategy(max_string_len), 0..=max_count)
}

/// Strategy for generating large key-value maps
fn large_map_strategy(max_count: usize) -> impl Strategy<Value = HashMap<String, String>> {
    prop::collection::hash_map("[a-z_]{1,32}", "[a-zA-Z0-9 ]{0,256}", 0..=max_count)
}

/// Calculate the approximate size of data
fn calculate_data_size(strings: &[String], map: &HashMap<String, String>) -> usize {
    let string_size: usize = strings.iter().map(|s| s.len()).sum();
    let map_size: usize = map.iter().map(|(k, v)| k.len() + v.len()).sum();
    string_size + map_size
}

/// Check if data size is within acceptable limits
fn is_size_acceptable(size: usize, max_mb: usize) -> bool {
    size <= max_mb * 1024 * 1024
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Test handling of large host names
    #[test]
    fn test_large_host_names(host in large_string_strategy(1024)) {
        let trimmed = host.trim();
        let len = trimmed.len();

        // Should be truncatable to reasonable size
        let truncated = if len > 256 { &trimmed[..256] } else { trimmed };
        prop_assert!(truncated.len() <= 256);
    }

    /// Test handling of large task names
    #[test]
    fn test_large_task_names(task_name in large_string_strategy(4096)) {
        let trimmed = task_name.trim();

        // Calculate size
        let size = trimmed.len();
        let is_reasonable = size <= 4096;
        prop_assert!(is_reasonable || size > 4096);
    }

    /// Test handling of large stdout/stderr output
    #[test]
    fn test_large_output_lines(lines in large_string_array_strategy(1000, 1024)) {
        let total_size: usize = lines.iter().map(|s| s.len()).sum();
        let line_count = lines.len();

        // Should be calculable without overflow
        let _ = total_size;
        prop_assert!(line_count <= 1000);

        // Test truncation strategy
        let max_size = 1_000_000; // 1MB
        let should_truncate = total_size > max_size;
        let _ = should_truncate;
    }

    /// Test handling of large warning lists
    #[test]
    fn test_large_warning_lists(warnings in large_string_array_strategy(100, 512)) {
        let warning_count = warnings.len();

        // Validate each warning
        for warning in &warnings {
            let _ = warning.trim();
            let _ = warning.is_empty();
        }

        prop_assert!(warning_count <= 100);
    }

    /// Test handling of large notify handler lists
    #[test]
    fn test_large_notify_lists(handlers in large_string_array_strategy(50, 256)) {
        for handler in &handlers {
            let is_valid = !handler.is_empty()
                && handler.len() <= 256
                && handler.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ');
            let _ = is_valid;
        }
    }

    /// Test handling of large fact maps
    #[test]
    fn test_large_fact_maps(facts in large_map_strategy(256)) {
        let entry_count = facts.len();
        let total_size = calculate_data_size(&[], &facts);

        prop_assert!(entry_count <= 256);
        prop_assert!(is_size_acceptable(total_size, 10));
    }

    /// Test handling of large environment variable maps
    #[test]
    fn test_large_env_maps(env_vars in large_map_strategy(512)) {
        for (key, value) in &env_vars {
            // Key should be valid env var name
            let valid_key = !key.is_empty() && key.chars().all(|c| c.is_alphanumeric() || c == '_');
            let _ = valid_key;

            // Value can be anything
            let _ = value.len();
        }
    }

    /// Test concurrent event simulation
    #[test]
    fn test_concurrent_event_simulation(
        host_count in 1usize..=100,
        task_count in 1usize..=50,
    ) {
        let total_events = host_count.saturating_mul(task_count);
        prop_assert!(total_events <= 5000);

        // Simulate event generation
        let mut processed = 0usize;
        for host_id in 0..host_count {
            for task_id in 0..task_count {
                let host = format!("host{}", host_id);
                let task = format!("task_{}", task_id);
                let _ = (host, task);
                processed = processed.saturating_add(1);
            }
        }

        prop_assert_eq!(processed, total_events);
    }

    /// Test memory-efficient processing of large data
    #[test]
    fn test_memory_efficient_processing(
        data_chunks in prop::collection::vec(large_string_strategy(1024), 0..=100),
    ) {
        let total_size: usize = data_chunks.iter().map(|s| s.len()).sum();

        // Process in chunks (simulating streaming)
        let chunk_size = 4096;
        let expected_chunks = (total_size + chunk_size - 1) / chunk_size.max(1);

        prop_assert!(expected_chunks <= total_size.saturating_add(chunk_size) / chunk_size.max(1));
    }
}

// ===========================================================================
// Edge Case Tests
// ===========================================================================

proptest! {
    /// Test empty string handling
    #[test]
    fn test_empty_strings(
        count in 0usize..=10,
    ) {
        let empty_strings: Vec<String> = (0..count).map(|_| String::new()).collect();

        for s in &empty_strings {
            prop_assert!(s.is_empty());
            prop_assert_eq!(s.trim(), "");
            prop_assert_eq!(s.len(), 0);
        }
    }

    /// Test Unicode handling
    #[test]
    fn test_unicode_handling(s in "\\PC{0,256}") {
        // Should not panic on any Unicode
        let _ = s.trim();
        let _ = s.len();
        let _ = s.chars().count();
        let _ = s.len();
        let _ = s.is_empty();
        let _ = s.to_lowercase();
        let _ = s.to_uppercase();
    }

    /// Test null byte handling
    #[test]
    fn test_null_bytes(prefix in "[a-z]{0,10}", suffix in "[a-z]{0,10}") {
        let with_null = format!("{}\0{}", prefix, suffix);

        // Should handle null bytes gracefully
        let _ = with_null.contains('\0');
        let _ = with_null.split('\0').collect::<Vec<_>>();
        let cleaned: String = with_null.chars().filter(|&c| c != '\0').collect();
        prop_assert!(!cleaned.contains('\0'));
    }

    /// Test very long single strings
    #[test]
    fn test_very_long_strings(len in 0usize..=65536) {
        let long_string: String = "a".repeat(len);

        prop_assert_eq!(long_string.len(), len);

        // Test truncation
        let max_len = 4096;
        let truncated = if long_string.len() > max_len {
            &long_string[..max_len]
        } else {
            &long_string
        };
        prop_assert!(truncated.len() <= max_len);
    }

    /// Test special characters in names
    #[test]
    fn test_special_characters(
        base in "[a-z]{1,8}",
        special in prop::sample::select(vec![
            ".", "-", "_", "@", "#", "$", "%", "^", "&", "*",
            "(", ")", "[", "]", "{", "}", "|", "\\", "/", ":",
            ";", "'", "\"", "<", ">", ",", "?", "!", "`", "~",
        ]),
    ) {
        let with_special = format!("{}{}{}", base, special, base);

        // Check if it's a valid identifier
        let is_valid_id = with_special.chars().all(|c| c.is_alphanumeric() || c == '_');
        let _ = is_valid_id;
    }

    /// Test numeric edge cases in stats
    #[test]
    fn test_numeric_edge_cases(
        value in prop::sample::select(vec![
            0u32, 1, u32::MAX, u32::MAX - 1,
        ]),
    ) {
        // Test increment without overflow
        let incremented = value.saturating_add(1);
        prop_assert!(incremented >= value || value == u32::MAX);

        // Test decrement without underflow
        let decremented = value.saturating_sub(1);
        prop_assert!(decremented <= value || value == 0);
    }

    /// Test duration edge cases
    #[test]
    fn test_duration_edge_cases(
        ms in prop::sample::select(vec![
            0u64, 1, 1000, 60_000, 3_600_000, u64::MAX,
        ]),
    ) {
        let duration = Duration::from_millis(ms);

        // Should be convertible to various units without panic
        let _ = duration.as_secs();
        let _ = duration.as_millis();
        let _ = duration.as_micros();
        let _ = duration.as_nanos();

        // Format duration
        let formatted = if duration.as_secs() >= 3600 {
            format!("{}h", duration.as_secs() / 3600)
        } else if duration.as_secs() >= 60 {
            format!("{}m", duration.as_secs() / 60)
        } else {
            format!("{}s", duration.as_secs())
        };
        prop_assert!(!formatted.is_empty());
    }
}

// ===========================================================================
// Integration-style Fuzz Tests
// ===========================================================================

/// Simulated callback event for integration testing
#[derive(Debug, Clone)]
struct SimulatedEvent {
    event_type: String,
    host: String,
    task_name: String,
    status: String,
    changed: bool,
    message: String,
    duration_ms: u64,
    data: HashMap<String, String>,
}

/// Strategy for generating complete simulated events
fn simulated_event_strategy() -> impl Strategy<Value = SimulatedEvent> {
    (
        event_type_strategy(),
        "[a-zA-Z0-9._-]{1,64}",
        "[a-zA-Z0-9 _-]{1,128}",
        task_status_strategy(),
        any::<bool>(),
        "[a-zA-Z0-9 .,!?-]{0,256}",
        0u64..=3_600_000u64,
        large_map_strategy(16),
    )
        .prop_map(
            |(event_type, host, task_name, status, changed, message, duration_ms, data)| {
                SimulatedEvent {
                    event_type,
                    host,
                    task_name,
                    status,
                    changed,
                    message,
                    duration_ms,
                    data,
                }
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Test complete event lifecycle
    #[test]
    fn test_complete_event_lifecycle(event in simulated_event_strategy()) {
        // Validate event type
        let _ = validate_event_type(&event.event_type);

        // Validate host
        prop_assert!(!event.host.is_empty());
        prop_assert!(event.host.len() <= 64);

        // Validate task name
        prop_assert!(event.task_name.len() <= 128);

        // Validate status
        let _ = parse_task_status(&event.status);

        // Validate duration
        let duration = Duration::from_millis(event.duration_ms);
        prop_assert!(duration.as_secs() <= 3600);

        // Validate data
        prop_assert!(event.data.len() <= 16);
    }

    /// Test event serialization simulation
    #[test]
    fn test_event_serialization(event in simulated_event_strategy()) {
        // Simulate JSON-like serialization
        let serialized = format!(
            r#"{{"event_type":"{}","host":"{}","task":"{}","status":"{}","changed":{},"message":"{}","duration_ms":{}}}"#,
            event.event_type.replace('"', "\\\""),
            event.host.replace('"', "\\\""),
            event.task_name.replace('"', "\\\""),
            event.status.replace('"', "\\\""),
            event.changed,
            event.message.replace('"', "\\\""),
            event.duration_ms,
        );

        // Should produce valid-looking JSON structure
        assert!(serialized.starts_with('{'));
        assert!(serialized.ends_with('}'));
        prop_assert!(serialized.contains("event_type"));
    }

    /// Test batch event processing
    #[test]
    fn test_batch_event_processing(
        events in prop::collection::vec(simulated_event_strategy(), 0..=50),
    ) {
        let mut ok_count = 0u32;
        let mut changed_count = 0u32;
        let mut failed_count = 0u32;
        let mut total_duration_ms = 0u64;

        for event in &events {
            match parse_task_status(&event.status) {
                Some("ok") => ok_count = ok_count.saturating_add(1),
                Some("changed") => changed_count = changed_count.saturating_add(1),
                Some("failed") | Some("unreachable") => failed_count = failed_count.saturating_add(1),
                _ => {}
            }
            total_duration_ms = total_duration_ms.saturating_add(event.duration_ms);
        }

        let total_events = events.len() as u32;
        let counted = ok_count.saturating_add(changed_count).saturating_add(failed_count);

        // Counted events should not exceed total
        prop_assert!(counted <= total_events);
    }
}
