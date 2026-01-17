//! Pipe Lookup Plugin
//!
//! Executes a command and returns its output. Similar to Ansible's `pipe` lookup plugin.
//!
//! # Usage
//!
//! ```yaml
//! # Execute a simple command
//! hostname: "{{ lookup('pipe', 'hostname') }}"
//!
//! # Execute a more complex command
//! date: "{{ lookup('pipe', 'date +%Y-%m-%d') }}"
//!
//! # With shell features
//! files: "{{ lookup('pipe', 'ls -la /tmp | head -5') }}"
//! ```
//!
//! # Security
//!
//! This lookup plugin executes arbitrary commands. For safety:
//! - Set `allow_unsafe=true` in the context to enable command execution
//! - Commands are executed via `/bin/sh -c` on Unix systems
//! - Be careful with user-provided input in commands
//!
//! # Options
//!
//! - `cwd` (string): Working directory for command execution
//! - `executable` (string): Shell to use (default: /bin/sh on Unix)

use super::{Lookup, LookupContext, LookupError, LookupResult};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Default command timeout in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Pipe lookup plugin for executing commands
#[derive(Debug, Clone, Default)]
pub struct PipeLookup;

impl PipeLookup {
    /// Create a new PipeLookup instance
    pub fn new() -> Self {
        Self
    }

    /// Validate a command string for basic safety
    fn validate_command(&self, cmd: &str) -> LookupResult<()> {
        if cmd.is_empty() {
            return Err(LookupError::InvalidArguments(
                "Command cannot be empty".to_string(),
            ));
        }

        // Check for null bytes
        if cmd.contains('\0') {
            return Err(LookupError::InvalidArguments(
                "Command contains null byte".to_string(),
            ));
        }

        Ok(())
    }

    /// Execute a command and capture its output
    fn execute_command(
        &self,
        cmd: &str,
        cwd: Option<&str>,
        executable: Option<&str>,
        timeout: Duration,
    ) -> LookupResult<String> {
        // Determine the shell to use
        #[cfg(unix)]
        let (shell, shell_arg) = {
            let shell = executable.unwrap_or("/bin/sh");
            (shell, "-c")
        };

        #[cfg(windows)]
        let (shell, shell_arg) = {
            let shell = executable.unwrap_or("cmd.exe");
            (shell, "/C")
        };

        let mut command = Command::new(shell);
        command
            .arg(shell_arg)
            .arg(cmd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set working directory if specified
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }

        // Execute the command with timeout enforcement
        let mut child = command.spawn().map_err(|e| {
            LookupError::CommandFailed(format!("Failed to execute command: {}", e))
        })?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let stdout_handle = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(mut out) = stdout {
                let _ = out.read_to_end(&mut buffer);
            }
            buffer
        });

        let stderr_handle = std::thread::spawn(move || {
            let mut buffer = Vec::new();
            if let Some(mut err) = stderr {
                let _ = err.read_to_end(&mut buffer);
            }
            buffer
        });

        let mut timed_out = false;
        let status = if timeout.is_zero() {
            child.wait().map_err(|e| {
                LookupError::CommandFailed(format!("Failed to wait for command: {}", e))
            })?
        } else {
            let start = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => {
                        if start.elapsed() >= timeout {
                            timed_out = true;
                            let _ = child.kill();
                            break child.wait().map_err(|e| {
                                LookupError::CommandFailed(format!(
                                    "Failed to wait for killed command: {}",
                                    e
                                ))
                            })?;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => {
                        return Err(LookupError::CommandFailed(format!(
                            "Failed to wait for command: {}",
                            e
                        )));
                    }
                }
            }
        };

        let stdout = stdout_handle
            .join()
            .map_err(|_| LookupError::CommandFailed("Failed to capture stdout".to_string()))?;
        let stderr = stderr_handle
            .join()
            .map_err(|_| LookupError::CommandFailed("Failed to capture stderr".to_string()))?;

        if timed_out {
            return Err(LookupError::Timeout(timeout.as_secs()));
        }

        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr);
            let exit_code = status.code().unwrap_or(-1);
            return Err(LookupError::CommandFailed(format!(
                "Command failed with exit code {}: {}",
                exit_code,
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8(stdout).map_err(|e| {
            LookupError::ParseError(format!("Failed to parse command output as UTF-8: {}", e))
        })?;

        Ok(stdout)
    }
}

impl Lookup for PipeLookup {
    fn name(&self) -> &'static str {
        "pipe"
    }

    fn description(&self) -> &'static str {
        "Executes a command and returns its output"
    }

    fn lookup(&self, args: &[&str], context: &LookupContext) -> LookupResult<Vec<String>> {
        // Check if unsafe operations are allowed
        if !context.allow_unsafe {
            return Err(LookupError::PermissionDenied(
                "Pipe lookup requires allow_unsafe=true in context for security".to_string(),
            ));
        }

        // Find the command (first non-option argument)
        let cmd = args
            .iter()
            .find(|arg| !arg.contains('='))
            .ok_or_else(|| LookupError::MissingArgument("command required".to_string()))?;

        // Validate the command
        self.validate_command(cmd)?;

        // Parse options
        let options = self.parse_options(args);
        let cwd = options.get("cwd").map(|s| s.as_str());
        let executable = options.get("executable").map(|s| s.as_str());

        // Calculate timeout
        let timeout = Duration::from_secs(context.timeout_secs);

        // Execute the command
        let output = self.execute_command(cmd, cwd, executable, timeout)?;

        Ok(vec![output])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::process::Command;

    fn unsafe_context() -> LookupContext {
        LookupContext::new().with_allow_unsafe(true)
    }

    #[cfg(unix)]
    fn hostname_command() -> String {
        let available = Command::new("sh")
            .arg("-c")
            .arg("command -v hostname")
            .status()
            .map(|status| status.success())
            .unwrap_or(false);

        if available {
            "hostname".to_string()
        } else {
            "uname -n".to_string()
        }
    }

    #[cfg(windows)]
    fn hostname_command() -> String {
        "hostname".to_string()
    }

    #[test]
    fn test_pipe_lookup_simple_command() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        let result = lookup.lookup(&["echo hello"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].trim(), "hello");
    }

    #[test]
    fn test_pipe_lookup_hostname() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        let cmd = hostname_command();
        let result = lookup.lookup(&[cmd.as_str()], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert!(!values[0].trim().is_empty());
    }

    #[test]
    fn test_pipe_lookup_with_pipe() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        let result = lookup.lookup(&["echo 'line1\nline2\nline3' | head -1"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert!(values[0].contains("line1"));
    }

    #[test]
    fn test_pipe_lookup_with_cwd() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        let result = lookup.lookup(&["pwd", "cwd=/tmp"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert!(values[0].trim().contains("tmp") || values[0].trim().contains("/private/tmp"));
    }

    #[test]
    fn test_pipe_lookup_command_not_found() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        let result = lookup.lookup(&["nonexistent_command_12345"], &context);
        assert!(matches!(result, Err(LookupError::CommandFailed(_))));
    }

    #[test]
    fn test_pipe_lookup_without_unsafe() {
        let lookup = PipeLookup::new();
        let context = LookupContext::default(); // allow_unsafe is false by default

        let result = lookup.lookup(&["echo test"], &context);
        assert!(matches!(result, Err(LookupError::PermissionDenied(_))));
    }

    #[test]
    fn test_pipe_lookup_missing_command() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        let result = lookup.lookup(&["cwd=/tmp"], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    #[test]
    fn test_pipe_lookup_empty_command() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        // Empty args
        let result = lookup.lookup(&[], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    #[test]
    fn test_pipe_lookup_exit_code() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        // Command that exits with non-zero status
        let result = lookup.lookup(&["exit 1"], &context);
        assert!(matches!(result, Err(LookupError::CommandFailed(_))));
    }

    #[test]
    fn test_pipe_lookup_environment() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        // Environment variable expansion
        let result = lookup.lookup(&["echo $HOME"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert!(!values[0].trim().is_empty());
    }

    #[test]
    fn test_pipe_lookup_multiline_output() {
        let lookup = PipeLookup::new();
        let context = unsafe_context();

        let result = lookup.lookup(&["echo -e 'line1\\nline2\\nline3'"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        let lines: Vec<&str> = values[0].lines().collect();
        assert!(lines.len() >= 1);
    }

    #[test]
    fn test_validate_command() {
        let lookup = PipeLookup::new();

        // Valid commands
        assert!(lookup.validate_command("echo hello").is_ok());
        assert!(lookup.validate_command("ls -la").is_ok());
        assert!(lookup.validate_command("cat /etc/passwd | grep root").is_ok());

        // Invalid commands
        assert!(lookup.validate_command("").is_err());
        assert!(lookup.validate_command("echo\0null").is_err());
    }
}
