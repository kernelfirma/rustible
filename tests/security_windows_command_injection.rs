
#[cfg(test)]
mod tests {
    use rustible::modules::validate_command_args;

    #[test]
    fn test_windows_injection_patterns_now_rejected() {
        // % should now be rejected (Windows environment variable expansion)
        assert!(validate_command_args("echo %USERNAME%").is_err(), "Expected %USERNAME% to be rejected");

        // ^ should now be rejected (Windows escape character)
        assert!(validate_command_args("echo ^").is_err(), "Expected ^ to be rejected");
    }
}
