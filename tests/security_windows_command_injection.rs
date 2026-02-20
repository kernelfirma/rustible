use rustible::modules::{validate_command_args, ModuleError};

#[test]
fn test_validate_command_args_blocks_windows_caret() {
    // ^ is the escape character in cmd.exe.
    // "echo h^ello" -> prints "hello"
    // "who^ami" -> runs "whoami"
    //
    // validate_command_args should now reject this.

    let payload = "echo h^ello";

    let result = validate_command_args(payload);

    assert!(result.is_err(), "Expected payload with ^ to be rejected");
    match result {
        Err(ModuleError::InvalidParameter(msg)) => {
            assert!(
                msg.contains("shell escape ^"),
                "Error message should mention shell escape ^"
            );
        }
        _ => panic!("Expected InvalidParameter error"),
    }
}

#[test]
fn test_validate_command_args_blocks_variable_expansion() {
    // %VAR% is expanded by cmd.exe.
    // "echo %OS%"

    let payload = "echo %OS%";

    // validate_command_args should now reject this.
    let result = validate_command_args(payload);

    assert!(result.is_err(), "Expected payload with % to be rejected");
    match result {
        Err(ModuleError::InvalidParameter(msg)) => {
            assert!(
                msg.contains("variable expansion %"),
                "Error message should mention variable expansion %"
            );
        }
        _ => panic!("Expected InvalidParameter error"),
    }
}
