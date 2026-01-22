//! Pause module - Pause playbook execution for a specified time or prompt
//!
//! This module pauses playbook execution for a specified time period or until
//! a user confirms continuation. It supports both timed pauses (seconds/minutes)
//! and interactive prompts for user input.
//!
//! In non-interactive mode (no TTY), the module will skip waiting for user input
//! and only respect timed pauses.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::Duration;

/// Module for pausing playbook execution
pub struct PauseModule;

impl PauseModule {
    /// Check if we're running in an interactive terminal
    fn is_interactive(&self) -> bool {
        // Check if stdin is a TTY (interactive terminal)
        use is_terminal::IsTerminal;
        std::io::stdin().is_terminal()
    }

    /// Read user input from stdin with optional echo control
    fn read_input(&self, echo: bool) -> io::Result<String> {
        if echo {
            // Normal input with echo
            let stdin = io::stdin();
            let mut line = String::new();
            stdin.lock().read_line(&mut line)?;
            Ok(line.trim_end().to_string())
        } else {
            // Input without echo (for sensitive data)
            // Note: Full no-echo support would require termios on Unix
            // For now, we use rpassword-like behavior or fall back to normal input
            #[cfg(unix)]
            {
                self.read_input_no_echo_unix()
            }
            #[cfg(not(unix))]
            {
                // On non-Unix systems, fall back to normal input with a warning
                let stdin = io::stdin();
                let mut line = String::new();
                stdin.lock().read_line(&mut line)?;
                Ok(line.trim_end().to_string())
            }
        }
    }

    /// Read input without echo on Unix systems
    #[cfg(unix)]
    fn read_input_no_echo_unix(&self) -> io::Result<String> {
        use std::os::unix::io::AsFd;

        let stdin = io::stdin();
        let fd = stdin.as_fd();

        // Get current terminal settings
        let mut termios = match nix::sys::termios::tcgetattr(fd) {
            Ok(t) => t,
            Err(_) => {
                // Fall back to normal input if we can't get termios
                let mut line = String::new();
                stdin.lock().read_line(&mut line)?;
                return Ok(line.trim_end().to_string());
            }
        };

        // Disable echo
        termios
            .local_flags
            .remove(nix::sys::termios::LocalFlags::ECHO);

        // Apply settings
        if nix::sys::termios::tcsetattr(fd, nix::sys::termios::SetArg::TCSANOW, &termios).is_err() {
            // Fall back to normal input
            let mut line = String::new();
            stdin.lock().read_line(&mut line)?;
            return Ok(line.trim_end().to_string());
        }

        // Read input
        let mut line = String::new();
        let result = stdin.lock().read_line(&mut line);

        // Restore echo (get fresh termios to restore)
        if let Ok(mut restore_termios) = nix::sys::termios::tcgetattr(fd) {
            restore_termios
                .local_flags
                .insert(nix::sys::termios::LocalFlags::ECHO);
            let _ = nix::sys::termios::tcsetattr(
                fd,
                nix::sys::termios::SetArg::TCSANOW,
                &restore_termios,
            );
        }

        // Print newline since echo was disabled
        println!();

        result?;
        Ok(line.trim_end().to_string())
    }

    /// Calculate total pause duration in seconds
    fn calculate_duration(&self, params: &ModuleParams) -> ModuleResult<Option<u64>> {
        let seconds = params.get_i64("seconds")?.map(|s| s.max(0) as u64);
        let minutes = params.get_i64("minutes")?.map(|m| m.max(0) as u64);

        match (seconds, minutes) {
            (Some(s), Some(m)) => Ok(Some(s + m * 60)),
            (Some(s), None) => Ok(Some(s)),
            (None, Some(m)) => Ok(Some(m * 60)),
            (None, None) => Ok(None),
        }
    }
}

impl Module for PauseModule {
    fn name(&self) -> &'static str {
        "pause"
    }

    fn description(&self) -> &'static str {
        "Pause playbook execution for a specified time or prompt for user input"
    }

    fn classification(&self) -> ModuleClassification {
        // LocalLogic because this runs entirely on the control node
        // It doesn't need to connect to remote hosts
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // GlobalExclusive because interactive prompts should only happen once
        // and timed pauses should be synchronized across all hosts
        ParallelizationHint::GlobalExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        // All parameters are optional
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate seconds if present
        if let Some(seconds) = params.get("seconds") {
            match seconds {
                Value::Number(n) => {
                    if n.as_f64().is_none_or(|v| v < 0.0) {
                        return Err(ModuleError::InvalidParameter(
                            "seconds must be a non-negative number".to_string(),
                        ));
                    }
                }
                Value::String(s) => {
                    if s.parse::<i64>().map_or(true, |v| v < 0) {
                        return Err(ModuleError::InvalidParameter(
                            "seconds must be a non-negative integer".to_string(),
                        ));
                    }
                }
                _ => {
                    return Err(ModuleError::InvalidParameter(
                        "seconds must be a number".to_string(),
                    ));
                }
            }
        }

        // Validate minutes if present
        if let Some(minutes) = params.get("minutes") {
            match minutes {
                Value::Number(n) => {
                    if n.as_f64().is_none_or(|v| v < 0.0) {
                        return Err(ModuleError::InvalidParameter(
                            "minutes must be a non-negative number".to_string(),
                        ));
                    }
                }
                Value::String(s) => {
                    if s.parse::<i64>().map_or(true, |v| v < 0) {
                        return Err(ModuleError::InvalidParameter(
                            "minutes must be a non-negative integer".to_string(),
                        ));
                    }
                }
                _ => {
                    return Err(ModuleError::InvalidParameter(
                        "minutes must be a number".to_string(),
                    ));
                }
            }
        }

        // Validate echo if present
        if let Some(echo) = params.get("echo") {
            match echo {
                Value::Bool(_) => {}
                Value::String(s) => {
                    let lower = s.to_lowercase();
                    if !["true", "false", "yes", "no", "1", "0"].contains(&lower.as_str()) {
                        return Err(ModuleError::InvalidParameter(
                            "echo must be a boolean value (yes/no/true/false)".to_string(),
                        ));
                    }
                }
                _ => {
                    return Err(ModuleError::InvalidParameter(
                        "echo must be a boolean value".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // In check mode, just report what would happen
        if context.check_mode {
            let prompt = params.get_string("prompt")?;
            let duration = self.calculate_duration(params)?;
            let is_interactive = self.is_interactive();

            let message = match (duration, &prompt, is_interactive) {
                (Some(secs), None, _) if secs > 0 => {
                    if secs >= 60 {
                        format!(
                            "Would pause for {} minute(s) and {} second(s)",
                            secs / 60,
                            secs % 60
                        )
                    } else {
                        format!("Would pause for {} second(s)", secs)
                    }
                }
                (duration_opt, Some(prompt_text), true) => {
                    if let Some(secs) = duration_opt {
                        format!(
                            "Would prompt user with '{}' and pause for {} seconds",
                            prompt_text.trim(),
                            secs
                        )
                    } else {
                        format!("Would prompt user with '{}'", prompt_text.trim())
                    }
                }
                (duration_opt, Some(_), false) => {
                    if let Some(secs) = duration_opt {
                        format!(
                            "Would skip prompt (non-interactive) and pause for {} seconds",
                            secs
                        )
                    } else {
                        "Would skip prompt (non-interactive mode)".to_string()
                    }
                }
                (None, None, true) => "Would wait for user to press Enter".to_string(),
                (None, None, false) => "Would skip pause (non-interactive mode)".to_string(),
                (Some(0), None, _) => "No pause required (0 seconds)".to_string(),
                // Catch-all for any other cases
                _ => "Pause would complete".to_string(),
            };

            return Ok(ModuleOutput::ok(message));
        }

        let prompt = params.get_string("prompt")?;
        let echo = params.get_bool("echo")?.unwrap_or(true);
        let duration = self.calculate_duration(params)?;
        let is_interactive = self.is_interactive();

        let mut output_data: HashMap<String, Value> = HashMap::new();
        let message: String;

        match (duration, &prompt, is_interactive) {
            // Timed pause only (no prompt)
            (Some(secs), None, _) if secs > 0 => {
                output_data.insert("seconds".to_string(), Value::Number(secs.into()));

                // Display pause message
                let display_time = if secs >= 60 {
                    format!("{} minute(s) and {} second(s)", secs / 60, secs % 60)
                } else {
                    format!("{} second(s)", secs)
                };

                eprintln!("Pausing for {}...", display_time);
                io::stderr().flush().ok();

                // Sleep for the specified duration
                thread::sleep(Duration::from_secs(secs));

                message = format!("Paused for {}", display_time);
            }

            // Interactive prompt with optional timeout
            (duration_opt, Some(prompt_text), true) => {
                // Display prompt
                eprint!("{}", prompt_text);
                if !prompt_text.ends_with(' ') && !prompt_text.ends_with(':') {
                    eprint!(": ");
                }
                io::stderr().flush().ok();

                // Read user input
                // Note: Timeout handling would require async or threading, for simplicity
                // we ignore timeout during interactive prompts (like Ansible does by default)
                let user_input = self.read_input(echo).map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to read user input: {}", e))
                })?;

                output_data.insert("user_input".to_string(), Value::String(user_input.clone()));

                // If there's also a timed component, wait that too
                if let Some(secs) = duration_opt {
                    if secs > 0 {
                        output_data.insert("seconds".to_string(), Value::Number(secs.into()));
                        thread::sleep(Duration::from_secs(secs));
                    }
                }

                if user_input.is_empty() {
                    message = "Paused for user confirmation".to_string();
                } else {
                    message = format!("User input received: {} characters", user_input.len());
                }
            }

            // Non-interactive mode with prompt - skip waiting for input
            (duration_opt, Some(prompt_text), false) => {
                eprintln!(
                    "Skipping interactive prompt (no TTY): {}",
                    prompt_text.trim()
                );
                output_data.insert("skipped".to_string(), Value::Bool(true));
                output_data.insert("user_input".to_string(), Value::String(String::new()));

                // Still respect any timed duration
                if let Some(secs) = duration_opt {
                    if secs > 0 {
                        output_data.insert("seconds".to_string(), Value::Number(secs.into()));
                        thread::sleep(Duration::from_secs(secs));
                        message = format!(
                            "Skipped prompt (non-interactive), paused for {} seconds",
                            secs
                        );
                    } else {
                        message = "Skipped prompt (non-interactive mode)".to_string();
                    }
                } else {
                    message = "Skipped prompt (non-interactive mode)".to_string();
                }
            }

            // No duration and no prompt - default to waiting for Enter (interactive only)
            (None, None, true) => {
                eprintln!("Press Enter to continue...");
                io::stderr().flush().ok();

                let user_input = self.read_input(echo).map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to read user input: {}", e))
                })?;

                output_data.insert("user_input".to_string(), Value::String(user_input));
                message = "Paused for user confirmation".to_string();
            }

            // Non-interactive with no duration and no prompt - just skip
            (None, None, false) => {
                eprintln!("Skipping pause (non-interactive mode, no duration specified)");
                output_data.insert("skipped".to_string(), Value::Bool(true));
                message = "Skipped pause (non-interactive mode)".to_string();
            }

            // Zero duration - no actual pause needed
            (Some(0), None, _) => {
                message = "No pause required (0 seconds)".to_string();
            }

            // Catch-all for any other cases (shouldn't happen but satisfies exhaustiveness)
            _ => {
                message = "Pause completed".to_string();
            }
        }

        output_data.insert("echo".to_string(), Value::Bool(echo));

        let mut output = ModuleOutput::ok(message);
        output.data = output_data;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pause_module_name() {
        let module = PauseModule;
        assert_eq!(module.name(), "pause");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_pause_module_classification() {
        let module = PauseModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::GlobalExclusive
        );
    }

    #[test]
    fn test_pause_validate_valid_seconds() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("seconds".to_string(), Value::Number(30.into()));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_validate_valid_minutes() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("minutes".to_string(), Value::Number(5.into()));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_validate_valid_both() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("seconds".to_string(), Value::Number(30.into()));
        params.insert("minutes".to_string(), Value::Number(2.into()));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_validate_string_seconds() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("seconds".to_string(), Value::String("45".to_string()));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_validate_invalid_negative_seconds() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("seconds".to_string(), Value::Number((-10).into()));

        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-negative"));
    }

    #[test]
    fn test_pause_validate_invalid_negative_minutes() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("minutes".to_string(), Value::Number((-5).into()));

        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-negative"));
    }

    #[test]
    fn test_pause_validate_echo_boolean() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "prompt".to_string(),
            Value::String("Enter value".to_string()),
        );
        params.insert("echo".to_string(), Value::Bool(false));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_validate_echo_string_yes() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("echo".to_string(), Value::String("yes".to_string()));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_validate_echo_string_no() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("echo".to_string(), Value::String("no".to_string()));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_validate_invalid_echo() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("echo".to_string(), Value::String("invalid".to_string()));

        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("boolean"));
    }

    #[test]
    fn test_pause_calculate_duration_seconds_only() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("seconds".to_string(), Value::Number(45.into()));

        let duration = module.calculate_duration(&params).unwrap();
        assert_eq!(duration, Some(45));
    }

    #[test]
    fn test_pause_calculate_duration_minutes_only() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("minutes".to_string(), Value::Number(2.into()));

        let duration = module.calculate_duration(&params).unwrap();
        assert_eq!(duration, Some(120));
    }

    #[test]
    fn test_pause_calculate_duration_both() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("seconds".to_string(), Value::Number(30.into()));
        params.insert("minutes".to_string(), Value::Number(2.into()));

        let duration = module.calculate_duration(&params).unwrap();
        assert_eq!(duration, Some(150)); // 2*60 + 30
    }

    #[test]
    fn test_pause_calculate_duration_none() {
        let module = PauseModule;
        let params: ModuleParams = HashMap::new();

        let duration = module.calculate_duration(&params).unwrap();
        assert_eq!(duration, None);
    }

    #[test]
    fn test_pause_check_mode_with_seconds() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("seconds".to_string(), Value::Number(30.into()));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, super::super::ModuleStatus::Ok);
        assert!(result.msg.contains("Would pause"));
        assert!(result.msg.contains("30"));
    }

    #[test]
    fn test_pause_check_mode_with_minutes() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("minutes".to_string(), Value::Number(5.into()));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Would pause"));
        assert!(result.msg.contains("5 minute"));
    }

    #[test]
    fn test_pause_check_mode_with_prompt() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "prompt".to_string(),
            Value::String("Enter your name".to_string()),
        );

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        // Message depends on whether running interactively
        assert!(result.msg.contains("Would prompt") || result.msg.contains("Would skip prompt"));
    }

    #[test]
    fn test_pause_empty_params() {
        let module = PauseModule;
        let params: ModuleParams = HashMap::new();

        // Validation should pass with empty params (all are optional)
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_with_prompt_and_echo() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "prompt".to_string(),
            Value::String("Enter password".to_string()),
        );
        params.insert("echo".to_string(), Value::Bool(false));

        // Validation should pass
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pause_zero_seconds() {
        let module = PauseModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("seconds".to_string(), Value::Number(0.into()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("No pause required") || result.msg.contains("0"));
    }
}
