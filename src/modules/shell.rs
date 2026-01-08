//! Shell module - Execute shell commands
//!
//! This module executes commands through a shell, enabling shell features
//! like pipes, redirects, environment variable expansion, etc.
//!
//! Supports both local execution (using std::process::Command) and remote
//! execution via async connections (SSH, Docker, etc.).

use super::{
    validate_env_var_name, validate_path_param, Diff, Module, ModuleClassification, ModuleContext,
    ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::{cmd_escape, shell_escape};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Module for executing shell commands
pub struct ShellModule;

impl ShellModule {
    /// Get shell executable and flag for local execution
    fn get_shell(&self, params: &ModuleParams) -> ModuleResult<(String, String)> {
        // Get shell executable
        let executable = params
            .get_string("executable")?
            .unwrap_or_else(|| "/bin/sh".to_string());

        // Different shells have different syntax for running commands
        let flag = if executable.ends_with("fish") {
            "-c".to_string()
        } else if executable.ends_with("cmd.exe") || executable.ends_with("cmd") {
            "/c".to_string()
        } else {
            "-c".to_string()
        };

        Ok((executable, flag))
    }

    /// Build the full shell command for remote execution
    fn build_shell_command(&self, cmd: &str, params: &ModuleParams) -> ModuleResult<String> {
        let executable = params
            .get_string("executable")?
            .unwrap_or_else(|| "/bin/sh".to_string());

        // Escape the command for shell execution
        if executable.ends_with("cmd.exe") || executable.ends_with("cmd") {
            // Windows cmd.exe does not respect single quotes.
            // We use double quotes and escape internal double quotes with "".
            // Logic extracted to crate::utils::cmd_escape, but here we need to escape the whole command line
            // for the /c argument.
            // The previous logic was: cmd.replace('"', "\"\"") wrapped in quotes.
            // cmd_escape does exactly that.
            Ok(format!("{} /c {}", executable, cmd_escape(cmd)))
        } else {
            // Unix-like shells (sh, bash, zsh, fish)
            // Use shell_escape to correctly quote/escape the command string
            // shell_escape guarantees a single safe token (quoted if necessary)
            Ok(format!("{} -c {}", executable, shell_escape(cmd)))
        }
    }

    /// Build ExecuteOptions from params for remote execution
    fn build_execute_options(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ExecuteOptions> {
        let mut options = ExecuteOptions::new();

        // Set working directory
        if let Some(chdir) = params.get_string("chdir")? {
            options = options.with_cwd(chdir);
        } else if let Some(ref work_dir) = context.work_dir {
            options = options.with_cwd(work_dir.clone());
        }

        // Set environment variables (with validation)
        if let Some(serde_json::Value::Object(env)) = params.get("env") {
            for (key, value) in env {
                // Validate environment variable name for security
                validate_env_var_name(key)?;
                if let serde_json::Value::String(v) = value {
                    options = options.with_env(key, v);
                }
            }
        }

        // Set timeout
        if let Some(timeout) = params.get_i64("timeout")? {
            if timeout > 0 {
                options = options.with_timeout(timeout as u64);
            }
        }

        // Handle privilege escalation from context
        if context.r#become {
            options.escalate = true;
            options.escalate_user = context.become_user.clone();
            options.escalate_method = context.become_method.clone();
        }

        Ok(options)
    }

    /// Check creates/removes conditions locally
    fn check_creates_removes_local(
        &self,
        params: &ModuleParams,
    ) -> ModuleResult<Option<ModuleOutput>> {
        // Check 'creates' - skip if file exists
        if let Some(creates) = params.get_string("creates")? {
            // Validate the path for security
            validate_path_param(&creates, "creates")?;
            if Path::new(&creates).exists() {
                return Ok(Some(ModuleOutput::ok(format!(
                    "Skipped, '{}' exists",
                    creates
                ))));
            }
        }

        // Check 'removes' - skip if file doesn't exist
        if let Some(removes) = params.get_string("removes")? {
            // Validate the path for security
            validate_path_param(&removes, "removes")?;
            if !Path::new(&removes).exists() {
                return Ok(Some(ModuleOutput::ok(format!(
                    "Skipped, '{}' does not exist",
                    removes
                ))));
            }
        }

        Ok(None)
    }

    /// Check creates/removes conditions on remote host
    async fn check_creates_removes_remote(
        &self,
        params: &ModuleParams,
        connection: &Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<Option<ModuleOutput>> {
        // Check 'creates' - skip if file exists
        if let Some(creates) = params.get_string("creates")? {
            // Validate the path for security
            validate_path_param(&creates, "creates")?;
            let exists = connection
                .path_exists(Path::new(&creates))
                .await
                .unwrap_or(false);
            if exists {
                return Ok(Some(ModuleOutput::ok(format!(
                    "Skipped, '{}' exists",
                    creates
                ))));
            }
        }

        // Check 'removes' - skip if file doesn't exist
        if let Some(removes) = params.get_string("removes")? {
            // Validate the path for security
            validate_path_param(&removes, "removes")?;
            let exists = connection
                .path_exists(Path::new(&removes))
                .await
                .unwrap_or(false);
            if !exists {
                return Ok(Some(ModuleOutput::ok(format!(
                    "Skipped, '{}' does not exist",
                    removes
                ))));
            }
        }

        Ok(None)
    }

    /// Execute shell command locally using std::process::Command
    fn execute_local(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Check creates/removes conditions
        if let Some(output) = self.check_creates_removes_local(params)? {
            return Ok(output);
        }

        let cmd = params.get_string_required("cmd")?;

        // In check mode, return what would happen
        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would execute shell command: {}",
                cmd
            )));
        }

        let (shell, flag) = self.get_shell(params)?;

        let mut command = Command::new(&shell);
        command.arg(&flag).arg(&cmd);

        // Set working directory
        if let Some(chdir) = params.get_string("chdir")? {
            command.current_dir(&chdir);
        } else if let Some(ref work_dir) = context.work_dir {
            command.current_dir(work_dir);
        }

        // Set environment variables (with validation)
        if let Some(serde_json::Value::Object(env)) = params.get("env") {
            for (key, value) in env {
                // Validate environment variable name for security
                validate_env_var_name(key)?;
                if let serde_json::Value::String(v) = value {
                    command.env(key, v);
                }
            }
        }

        // Handle stdin
        if let Some(stdin_data) = params.get_string("stdin")? {
            use std::io::Write;
            use std::process::Stdio;

            command.stdin(Stdio::piped());
            command.stdout(Stdio::piped());
            command.stderr(Stdio::piped());

            let mut child = command.spawn().map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to spawn shell: {}", e))
            })?;

            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(stdin_data.as_bytes()).map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to write to stdin: {}", e))
                })?;
            }

            let output = child.wait_with_output().map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to wait for command: {}", e))
            })?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let rc = output.status.code().unwrap_or(-1);

            if output.status.success() {
                return Ok(ModuleOutput::changed(
                    "Shell command executed successfully".to_string(),
                )
                .with_command_output(Some(stdout), Some(stderr), Some(rc)));
            } else {
                return Err(ModuleError::CommandFailed {
                    code: rc,
                    message: if stderr.is_empty() { stdout } else { stderr },
                });
            }
        }

        // Execute the command
        let output = command.output().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to execute shell command: {}", e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let rc = output.status.code().unwrap_or(-1);

        // Check if command succeeded
        if output.status.success() {
            let mut result =
                ModuleOutput::changed("Shell command executed successfully".to_string())
                    .with_command_output(Some(stdout), Some(stderr.clone()), Some(rc));

            let warn_on_stderr = params.get_bool_or("warn", true);
            if warn_on_stderr && !stderr.is_empty() {
                result
                    .data
                    .insert("warnings".to_string(), serde_json::json!([stderr]));
            }

            Ok(result)
        } else {
            Err(ModuleError::CommandFailed {
                code: rc,
                message: if stderr.is_empty() { stdout } else { stderr },
            })
        }
    }

    /// Execute shell command on remote host using async connection
    fn execute_remote(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let params_clone = params.clone();
        let check_mode = context.check_mode;
        let cmd = params.get_string_required("cmd")?;
        let shell_cmd = self.build_shell_command(&cmd, params)?;
        let options = self.build_execute_options(params, context)?;
        let warn_on_stderr = params.get_bool_or("warn", true);

        // Use scoped thread with a NEW runtime to avoid blocking the parent tokio runtime
        // This prevents deadlock when called from within an async context
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    // Create a new runtime in this thread - this avoids nesting
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| {
                            ModuleError::ExecutionFailed(format!("Failed to create runtime: {}", e))
                        })?;

                    rt.block_on(async {
                        // Check creates/removes conditions on remote
                        if let Some(output) = self
                            .check_creates_removes_remote(&params_clone, &connection)
                            .await?
                        {
                            return Ok(output);
                        }

                        // In check mode, return what would happen
                        if check_mode {
                            return Ok(ModuleOutput::changed(format!(
                                "Would execute shell command: {}",
                                cmd
                            )));
                        }

                        // Execute via connection
                        // Note: The connection.execute() runs through a shell anyway,
                        // but we wrap with explicit shell call for consistency and to
                        // support custom shell executables
                        let result = connection
                            .execute(&shell_cmd, Some(options))
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to execute shell command '{}': {}",
                                    cmd, e
                                ))
                            })?;

                        if result.success {
                            let mut output = ModuleOutput::changed(
                                "Shell command executed successfully".to_string(),
                            )
                            .with_command_output(
                                Some(result.stdout.clone()),
                                Some(result.stderr.clone()),
                                Some(result.exit_code),
                            );

                            if warn_on_stderr && !result.stderr.is_empty() {
                                output.data.insert(
                                    "warnings".to_string(),
                                    serde_json::json!([result.stderr]),
                                );
                            }

                            Ok(output)
                        } else {
                            Err(ModuleError::CommandFailed {
                                code: result.exit_code,
                                message: if result.stderr.is_empty() {
                                    result.stdout
                                } else {
                                    result.stderr
                                },
                            })
                        }
                    })
                })
                .join()
                .map_err(|_| ModuleError::ExecutionFailed("Thread panicked".to_string()))?
        })
    }
}

impl Module for ShellModule {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn description(&self) -> &'static str {
        "Execute shell commands with full shell features"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn required_params(&self) -> &[&'static str] {
        &["cmd"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Dispatch to local or remote execution based on connection
        if let Some(ref connection) = context.connection {
            self.execute_remote(params, context, connection.clone())
        } else {
            self.execute_local(params, context)
        }
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        // For check mode, we run execute with check_mode=true in context
        // The execute methods already handle check_mode internally
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        let mut output = self.execute(params, &check_context)?;

        // Add diff to show what would be executed
        if let Some(diff) = self.diff(params, context)? {
            output.diff = Some(diff);
        }
        Ok(output)
    }

    fn diff(&self, params: &ModuleParams, _context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let cmd = params.get_string_required("cmd")?;
        Ok(Some(Diff::new("(none)", format!("Execute: {}", cmd))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_shell_echo() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo hello"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.stdout.as_ref().unwrap().contains("hello"));
    }

    #[test]
    fn test_shell_pipe() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "cmd".to_string(),
            serde_json::json!("echo 'hello world' | grep hello"),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.stdout.as_ref().unwrap().contains("hello"));
    }

    #[test]
    fn test_shell_env_expansion() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo $HOME"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        // HOME should be expanded to a path
        assert!(!result.stdout.as_ref().unwrap().contains("$HOME"));
    }

    #[test]
    fn test_shell_check_mode() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("rm -rf /"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would execute"));
    }

    #[test]
    fn test_shell_creates_exists() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo hello"));
        params.insert("creates".to_string(), serde_json::json!("/"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Skipped"));
    }

    #[test]
    fn test_shell_with_stdin() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("cat"));
        params.insert("stdin".to_string(), serde_json::json!("hello from stdin"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.stdout.as_ref().unwrap().contains("hello from stdin"));
    }

    #[test]
    fn test_shell_cmd_exe_escaping() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("executable".to_string(), serde_json::json!("cmd.exe"));

        let cmd = "echo hello & calc.exe";
        let result = module.build_shell_command(cmd, &params).unwrap();

        // Should be: cmd.exe /c "echo hello & calc.exe"
        assert_eq!(result, "cmd.exe /c \"echo hello & calc.exe\"");
    }

    #[test]
    fn test_build_shell_command_posix() {
        let module = ShellModule;
        let params: ModuleParams = HashMap::new();
        // default executable is /bin/sh

        let cmd = "echo 'hello'";
        let result = module.build_shell_command(cmd, &params).unwrap();

        // shell_escape will wrap it in single quotes and escape the internal quotes
        // Should be: /bin/sh -c 'echo '\''hello'\'''
        assert_eq!(result, "/bin/sh -c 'echo '\\''hello'\\'''");

        let cmd = "echo hello & whoami";
        let result = module.build_shell_command(cmd, &params).unwrap();

        // shell_escape will wrap in single quotes because of spaces and &
        assert_eq!(result, "/bin/sh -c 'echo hello & whoami'");

        // Test safe string (might not be quoted by shell_escape)
        let cmd = "ls";
        let result = module.build_shell_command(cmd, &params).unwrap();
        // shell_escape("ls") -> "ls" (no quotes needed)
        assert_eq!(result, "/bin/sh -c ls");
    }

    #[test]
    fn test_shell_cmd_exe_escaping_quotes() {
        let module = ShellModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("executable".to_string(), serde_json::json!("cmd.exe"));

        let cmd = "echo \"hello\"";
        let result = module.build_shell_command(cmd, &params).unwrap();

        // Should escape quotes with ""
        assert_eq!(result, "cmd.exe /c \"echo \"\"hello\"\"\"");
    }
}
