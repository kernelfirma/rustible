use rustible::modules::cron::CronModule;
use rustible::modules::Module;
use std::collections::HashMap;

#[test]
fn test_cron_module_rejects_crlf_in_name() {
    let module = CronModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("job\nname"));
    params.insert("job".to_string(), serde_json::json!("echo hello"));

    // This should fail after fix
    let result = module.validate_params(&params);

    // For now (before fix), we assert that it is NOT an error, to verify the test runs.
    // BUT since I am implementing the fix, I want this test to pass IF it returns an error.
    // Since I haven't implemented the fix yet, this test will fail if I assert is_err().
    // I will write the test assuming the fix is present, so it will fail now, confirming the need for a fix.
    assert!(result.is_err(), "Should reject newlines in name");
}

#[test]
fn test_cron_module_rejects_crlf_in_job() {
    let module = CronModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("job"));
    params.insert("job".to_string(), serde_json::json!("echo hello\nrm -rf /"));

    // This should fail after fix
    let result = module.validate_params(&params);
    assert!(result.is_err(), "Should reject newlines in job");
}

#[test]
fn test_cron_module_rejects_crlf_in_user() {
    let module = CronModule;
    let mut params = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("job"));
    params.insert("job".to_string(), serde_json::json!("echo hello"));
    params.insert("user".to_string(), serde_json::json!("root\n"));

    // This should fail after fix
    let result = module.validate_params(&params);
    assert!(result.is_err(), "Should reject newlines in user");
}
