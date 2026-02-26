use rustible::modules::validate_command_args;

#[test]
fn test_windows_variable_expansion_injection() {
    // Current behavior: This returns Ok(()) because % is considered safe
    // Desired behavior: This should return Err because % allows command injection on Windows
    let result = validate_command_args("echo %USERNAME%");

    // With the fix, this should now return an error
    assert!(result.is_err(), "Should detect % injection");
}

#[test]
fn test_windows_caret_injection() {
    // Current behavior: This returns Ok(()) because ^ is not in dangerous list
    // Desired behavior: This should return Err because ^ allows escaping on Windows
    let result = validate_command_args("echo ^& whoami");

    // With the fix, this should now return an error (also & is dangerous)
    assert!(result.is_err(), "Should detect ^ or & injection");
}

#[test]
fn test_windows_caret_only_injection() {
    // ^ by itself
    let result = validate_command_args("echo ^");
    assert!(result.is_err(), "Should detect ^ injection");
}
