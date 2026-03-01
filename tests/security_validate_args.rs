
#[cfg(test)]
mod tests {
    use rustible::modules::validate_command_args;

    #[test]
    fn test_validate_command_args_rejects_percent() {
        // New behavior: % is rejected (Windows variable expansion)
        assert!(validate_command_args("echo %USERNAME%").is_err(), "Should reject %");
    }

    #[test]
    fn test_validate_command_args_rejects_caret() {
        // New behavior: ^ is rejected (Windows cmd.exe escape character)
        assert!(validate_command_args("echo ^N").is_err(), "Should reject ^");
    }
}
