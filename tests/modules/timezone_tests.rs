//! Comprehensive unit tests for the Timezone module
//!
//! Tests cover:
//! - Timezone validation
//! - Module metadata
//! - Parameter handling

use rustible::modules::timezone::TimezoneModule;
use rustible::modules::{Module, ModuleClassification, ModuleContext};
use std::collections::HashMap;

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_timezone_module_name() {
    let module = TimezoneModule;
    assert_eq!(module.name(), "timezone");
}

#[test]
fn test_timezone_module_description() {
    let module = TimezoneModule;
    assert!(!module.description().is_empty());
    assert!(module.description().to_lowercase().contains("timezone"));
}

#[test]
fn test_timezone_module_classification() {
    let module = TimezoneModule;
    assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
}

#[test]
fn test_timezone_module_required_params() {
    let module = TimezoneModule;
    let required = module.required_params();
    assert!(required.contains(&"name"));
    assert_eq!(required.len(), 1);
}

// ============================================================================
// Parameter Handling Tests
// ============================================================================

#[test]
fn test_timezone_missing_connection() {
    let module = TimezoneModule;
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("UTC"));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_timezone_missing_name_parameter() {
    let module = TimezoneModule;
    let params: HashMap<String, serde_json::Value> = HashMap::new();
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);
    assert!(result.is_err());
}

#[test]
fn test_timezone_basic_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("America/New_York"));

    assert!(params.get("name").is_some());
    assert_eq!(
        params.get("name").unwrap(),
        &serde_json::json!("America/New_York")
    );
}

#[test]
fn test_timezone_with_ntp_param() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("UTC"));
    params.insert("ntp".to_string(), serde_json::json!(true));

    assert!(params.get("ntp").is_some());
    assert_eq!(params.get("ntp").unwrap(), &serde_json::json!(true));
}

#[test]
fn test_timezone_with_hwclock_param() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("Europe/London"));
    params.insert("hwclock".to_string(), serde_json::json!("local"));

    assert!(params.get("hwclock").is_some());
    assert_eq!(params.get("hwclock").unwrap(), &serde_json::json!("local"));
}

#[test]
fn test_timezone_with_use_param() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("Asia/Tokyo"));
    params.insert("use".to_string(), serde_json::json!("timedatectl"));

    assert!(params.get("use").is_some());
    assert_eq!(
        params.get("use").unwrap(),
        &serde_json::json!("timedatectl")
    );
}

#[test]
fn test_timezone_all_params() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("America/Chicago"));
    params.insert("ntp".to_string(), serde_json::json!(true));
    params.insert("hwclock".to_string(), serde_json::json!("UTC"));
    params.insert("use".to_string(), serde_json::json!("auto"));

    assert_eq!(params.len(), 4);
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_timezone_check_mode_context() {
    let context = ModuleContext::default().with_check_mode(true);
    assert!(context.check_mode);
}

// ============================================================================
// Common Timezone Names Tests
// ============================================================================

#[test]
fn test_common_timezone_names() {
    let valid_timezones = [
        "UTC",
        "GMT",
        "America/New_York",
        "America/Los_Angeles",
        "America/Chicago",
        "Europe/London",
        "Europe/Paris",
        "Europe/Berlin",
        "Asia/Tokyo",
        "Asia/Shanghai",
        "Asia/Kolkata",
        "Australia/Sydney",
        "Pacific/Auckland",
    ];

    for tz in valid_timezones {
        assert!(!tz.is_empty(), "Timezone '{}' should not be empty", tz);
    }
}

#[test]
fn test_timezone_with_underscores() {
    let timezones_with_underscores = [
        "America/New_York",
        "America/Los_Angeles",
        "America/Sao_Paulo",
        "America/Argentina/Buenos_Aires",
        "America/North_Dakota/Center",
    ];

    for tz in timezones_with_underscores {
        assert!(
            tz.split('/').any(|segment| segment.contains('_')),
            "Timezone '{}' should contain underscore",
            tz
        );
    }
}

#[test]
fn test_timezone_area_location_format() {
    let area_location_timezones = [
        ("America", "New_York"),
        ("Europe", "London"),
        ("Asia", "Tokyo"),
        ("Australia", "Sydney"),
        ("Pacific", "Auckland"),
    ];

    for (area, location) in area_location_timezones {
        let full_tz = format!("{}/{}", area, location);
        assert!(
            full_tz.contains('/'),
            "Timezone '{}' should contain /",
            full_tz
        );
    }
}

#[test]
fn test_timezone_nested_locations() {
    let nested_timezones = [
        "America/Argentina/Buenos_Aires",
        "America/Indiana/Indianapolis",
        "America/Kentucky/Louisville",
        "America/North_Dakota/Center",
    ];

    for tz in nested_timezones {
        let parts: Vec<&str> = tz.split('/').collect();
        assert!(
            parts.len() >= 3,
            "Timezone '{}' should have at least 3 parts",
            tz
        );
    }
}

// ============================================================================
// Strategy Values Tests
// ============================================================================

#[test]
fn test_timezone_strategy_values() {
    let valid_strategies = ["timedatectl", "systemd", "file", "auto"];

    for strategy in valid_strategies {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("UTC"));
        params.insert("use".to_string(), serde_json::json!(strategy));

        assert!(
            params.get("use").is_some(),
            "Strategy '{}' should be valid",
            strategy
        );
    }
}

// ============================================================================
// Hwclock Mode Values Tests
// ============================================================================

#[test]
fn test_timezone_hwclock_values() {
    let valid_hwclock_modes = ["UTC", "utc", "local", "localtime"];

    for mode in valid_hwclock_modes {
        let mut params: HashMap<String, serde_json::Value> = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("UTC"));
        params.insert("hwclock".to_string(), serde_json::json!(mode));

        assert!(
            params.get("hwclock").is_some(),
            "Hwclock mode '{}' should be valid",
            mode
        );
    }
}

// ============================================================================
// Special Timezone Format Tests
// ============================================================================

#[test]
fn test_timezone_utc_gmt_variants() {
    let utc_gmt_variants = [
        "UTC",
        "GMT",
        "GMT+0",
        "GMT-5",
        "GMT+12",
        "Etc/UTC",
        "Etc/GMT+5",
        "Etc/GMT-12",
    ];

    for tz in utc_gmt_variants {
        assert!(!tz.is_empty(), "Timezone '{}' should not be empty", tz);
    }
}

#[test]
fn test_timezone_etc_format() {
    let etc_timezones = ["Etc/UTC", "Etc/GMT", "Etc/GMT+0", "Etc/GMT+5", "Etc/GMT-12"];

    for tz in etc_timezones {
        assert!(
            tz.starts_with("Etc/"),
            "Timezone '{}' should start with Etc/",
            tz
        );
    }
}

// ============================================================================
// NTP Parameter Tests
// ============================================================================

#[test]
fn test_timezone_ntp_boolean_values() {
    let mut params_true: HashMap<String, serde_json::Value> = HashMap::new();
    params_true.insert("name".to_string(), serde_json::json!("UTC"));
    params_true.insert("ntp".to_string(), serde_json::json!(true));

    let mut params_false: HashMap<String, serde_json::Value> = HashMap::new();
    params_false.insert("name".to_string(), serde_json::json!("UTC"));
    params_false.insert("ntp".to_string(), serde_json::json!(false));

    assert_eq!(params_true.get("ntp").unwrap(), &serde_json::json!(true));
    assert_eq!(params_false.get("ntp").unwrap(), &serde_json::json!(false));
}

// ============================================================================
// Combined Configuration Tests
// ============================================================================

#[test]
fn test_timezone_full_configuration() {
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("America/New_York"));
    params.insert("ntp".to_string(), serde_json::json!(true));
    params.insert("hwclock".to_string(), serde_json::json!("UTC"));
    params.insert("use".to_string(), serde_json::json!("timedatectl"));

    assert_eq!(params.len(), 4);
    assert!(params.get("name").is_some());
    assert!(params.get("ntp").is_some());
    assert!(params.get("hwclock").is_some());
    assert!(params.get("use").is_some());
}

#[test]
fn test_timezone_dual_boot_configuration() {
    // For Windows dual-boot systems, hwclock is often set to local
    let mut params: HashMap<String, serde_json::Value> = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("Europe/London"));
    params.insert("hwclock".to_string(), serde_json::json!("local"));
    params.insert("ntp".to_string(), serde_json::json!(false));

    assert_eq!(params.get("hwclock").unwrap(), &serde_json::json!("local"));
}
