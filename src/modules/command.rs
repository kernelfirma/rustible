//! Command module - Execute arbitrary commands
//!
//! This module executes commands directly without going through a shell.
//! For shell commands (pipes, redirects, etc.), use the shell module.
//!
//! Supports both local execution (using std::process::Command) and remote
//! execution via async connections (SSH, Docker, etc.).

use super::{
    validate_env_var_name, validate_path_param, Module, ModuleClassification, ModuleContext,
    ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::{cmd_escape, powershell_escape, shell_escape};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

/// Module for executing commands directly
pub struct CommandModule;

impl CommandModule {
    /// Build the command string from params (for display and remote execution)
    fn get_command_string(&self, params: &ModuleParams) -> ModuleResult<String> {
        let cmd = params.get_string("cmd")?;
        let argv = params.get_vec_string("argv")?;
        let shell_type = params
            .get_string("shell_type")?
            .unwrap_or_else(|| "posix".to_string());

        if let Some(argv) = argv {
            if argv.is_empty() {
                return Err(ModuleError::InvalidParameter(
                    "argv cannot be empty".to_string(),
                ));
            }

            // Join argv with proper escaping for shell
            let escaped_args: Vec<std::borrow::Cow<'_, str>> = argv
                .iter()
                .map(|arg| match shell_type.as_str() {
                    "cmd" => cmd_escape(arg),
                    "powershell" => powershell_escape(arg),
                    "posix" | "sh" | "bash" => shell_escape(arg),
                    _ => shell_escape(arg), // Default to POSIX for safety/backward compatibility
                })
                .collect();

            Ok(escaped_args.join(" "))
        } else if let Some(cmd) = cmd {
            if cmd.trim().is_empty() {
                return Err(ModuleError::InvalidParameter(
                    "cmd cannot be empty".to_string(),
                ));
            }
            Ok(cmd)
        } else {
            Err(ModuleError::MissingParameter(
                "Either 'cmd' or 'argv' must be provided".to_string(),
            ))
        }
    }

    /// Build a std::process::Command for local execution
    fn build_command(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<Command> {
        let cmd = params.get_string("cmd")?;
        let argv = params.get_vec_string("argv")?;

        let mut command = if let Some(argv) = argv {
            // If argv is provided, use the first element as the command
            if argv.is_empty() {
                return Err(ModuleError::InvalidParameter(
                    "argv cannot be empty".to_string(),
                ));
            }
            let mut cmd = Command::new(&argv[0]);
            if argv.len() > 1 {
                cmd.args(&argv[1..]);
            }
            cmd
        } else if let Some(cmd) = cmd {
            // Parse the command string into arguments
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if parts.is_empty() {
                return Err(ModuleError::InvalidParameter(
                    "cmd cannot be empty".to_string(),
                ));
            }
            let mut cmd = Command::new(parts[0]);
            if parts.len() > 1 {
                cmd.args(&parts[1..]);
            }
            cmd
        } else {
            return Err(ModuleError::MissingParameter(
                "Either 'cmd' or 'argv' must be provided".to_string(),
            ));
        };

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

        Ok(command)
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

    /// Execute command locally using std::process::Command
    fn execute_local(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Check creates/removes conditions
        if let Some(output) = self.check_creates_removes_local(params)? {
            return Ok(output);
        }

        // In check mode, return what would happen
        if context.check_mode {
            let cmd = self.get_command_string(params)?;
            return Ok(ModuleOutput::changed(format!("Would execute: {}", cmd)));
        }

        let mut command = self.build_command(params, context)?;
        let cmd_display = self.get_command_string(params)?;

        // Execute the command
        let output = command.output().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to execute '{}': {}", cmd_display, e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let rc = output.status.code().unwrap_or(-1);

        // Check if command succeeded
        let warn_on_stderr = params.get_bool_or("warn", true);

        if output.status.success() {
            let mut result =
                ModuleOutput::changed(format!("Command '{}' executed successfully", cmd_display))
                    .with_command_output(Some(stdout), Some(stderr.clone()), Some(rc));

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

    /// Execute command on remote host using async connection
    fn execute_remote(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let params_clone = params.clone();
        let check_mode = context.check_mode;
        let cmd_display = self.get_command_string(params)?;
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
                                "Would execute: {}",
                                cmd_display
                            )));
                        }

                        // Execute via connection
                        let result = connection
                            .execute(&cmd_display, Some(options))
                            .await
                            .map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to execute '{}': {}",
                                    cmd_display, e
                                ))
                            })?;

                        if result.success {
                            let mut output = ModuleOutput::changed(format!(
                                "Command '{}' executed successfully",
                                cmd_display
                            ))
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

impl Module for CommandModule {
    fn name(&self) -> &'static str {
        "command"
    }

    fn description(&self) -> &'static str {
        "Execute commands without going through a shell"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn required_params(&self) -> &[&'static str] {
        // Either 'cmd' or 'argv' is required, validation is done in validate_params
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Must have either cmd or argv
        if params.get("cmd").is_none() && params.get("argv").is_none() {
            return Err(ModuleError::MissingParameter(
                "Either 'cmd' or 'argv' must be provided".to_string(),
            ));
        }
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_command_echo() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo hello"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.stdout.as_ref().unwrap().contains("hello"));
        assert_eq!(result.rc, Some(0));
    }

    #[test]
    fn test_command_with_argv() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "argv".to_string(),
            serde_json::json!(["echo", "hello", "world"]),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.stdout.as_ref().unwrap().contains("hello world"));
    }

    #[test]
    fn test_command_creates_exists() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("echo hello"));
        params.insert("creates".to_string(), serde_json::json!("/"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Skipped"));
    }

    #[test]
    fn test_command_check_mode() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("rm -rf /"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would execute"));
    }

    #[test]
    fn test_command_fails() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), serde_json::json!("false"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context);

        assert!(result.is_err());
        if let Err(ModuleError::CommandFailed { code, .. }) = result {
            assert_ne!(code, 0);
        } else {
            panic!("Expected CommandFailed error");
        }
    }

    #[test]
    fn test_command_windows_cmd_escaping() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "argv".to_string(),
            serde_json::json!(["echo", "hello & calc.exe"]),
        );
        params.insert("shell_type".to_string(), serde_json::json!("cmd"));

        let cmd = module.get_command_string(&params).unwrap();
        // Should use double quotes for cmd.exe
        assert_eq!(cmd, "\"echo\" \"hello & calc.exe\"");
    }

    #[test]
    fn test_command_windows_powershell_escaping() {
        let module = CommandModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "argv".to_string(),
            serde_json::json!(["echo", "hello'world"]),
        );
        params.insert("shell_type".to_string(), serde_json::json!("powershell"));

        let cmd = module.get_command_string(&params).unwrap();
        // Should use single quotes with doubled single quotes for PowerShell
        assert_eq!(cmd, "'echo' 'hello''world'");
    }
}
