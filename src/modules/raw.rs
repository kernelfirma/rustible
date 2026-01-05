//! Raw module - Execute raw commands without shell processing
//!
//! This module executes commands directly without using a shell. Unlike the
//! `command` or `shell` modules, `raw` is designed for special cases where:
//!
//! - Python is not available on the target system
//! - The system is being bootstrapped before Python installation
//! - A network device or other non-standard system is being managed
//!
//! The raw module sends the command directly to the remote system over SSH
//! without any additional processing, escaping, or shell wrapper.
//!
//! # Example
//!
//! ```yaml
//! - name: Bootstrap Python on minimal system
//!   raw: apt-get update && apt-get install -y python3
//!
//! - name: Execute raw command on network device
//!   raw: show running-config
//!
//! - name: Install Python on AIX
//!   raw: /usr/bin/rpm -ivh python3.rpm
//! ```
//!
//! # Parameters
//!
//! - `raw` / `free_form` / `cmd` - The command to execute (required)
//! - `executable` - Path to the shell/interpreter to use (optional)

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};

/// Module for executing raw commands without shell processing
pub struct RawModule;

impl RawModule {
    /// Get the command from parameters
    fn get_command(&self, params: &ModuleParams) -> ModuleResult<String> {
        // Try various parameter names for the command
        if let Some(cmd) = params.get_string("raw")? {
            return Ok(cmd);
        }

        if let Some(cmd) = params.get_string("free_form")? {
            return Ok(cmd);
        }

        if let Some(cmd) = params.get_string("cmd")? {
            return Ok(cmd);
        }

        if let Some(cmd) = params.get_string("_raw_params")? {
            return Ok(cmd);
        }

        Err(ModuleError::MissingParameter(
            "raw command is required. Specify command as the module argument.".to_string(),
        ))
    }

    /// Execute the raw command on the target
    async fn execute_raw(
        &self,
        command: &str,
        executable: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed("No connection available for raw command".to_string())
        })?;

        // Build the actual command to execute
        let actual_command = if let Some(exec) = executable {
            format!("{} -c '{}'", exec, command.replace('\'', "'\\''"))
        } else {
            command.to_string()
        };

        // Execute the command
        let result = connection.execute(&actual_command, None).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Raw command execution failed: {}", e))
        })?;

        // Build output
        let mut output = if result.success {
            ModuleOutput::changed("Raw command executed successfully")
        } else {
            ModuleOutput::failed(format!(
                "Raw command failed with exit code {}",
                result.exit_code
            ))
        };

        output.stdout = Some(result.stdout.clone());
        output.stderr = Some(result.stderr.clone());
        output.rc = Some(result.exit_code);

        // Add stdout_lines and stderr_lines for compatibility
        output = output.with_data(
            "stdout_lines",
            serde_json::json!(result.stdout.lines().collect::<Vec<_>>()),
        );
        output = output.with_data(
            "stderr_lines",
            serde_json::json!(result.stderr.lines().collect::<Vec<_>>()),
        );
        output = output.with_data("cmd", serde_json::json!(command));

        Ok(output)
    }
}

impl Module for RawModule {
    fn name(&self) -> &'static str {
        "raw"
    }

    fn description(&self) -> &'static str {
        "Execute raw command without shell processing"
    }

    fn classification(&self) -> ModuleClassification {
        // RemoteCommand because this executes on the remote system
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Raw commands can run in parallel across hosts
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        // Command is required but can come from different keys
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate that we have a command
        self.get_command(params)?;
        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let command = self.get_command(params)?;
        let executable = params.get_string("executable")?;

        // In check mode, we don't execute the command
        if context.check_mode {
            return Ok(ModuleOutput::ok(format!(
                "Would execute raw command: {}",
                command
            ))
            .with_data("cmd", serde_json::json!(command)));
        }

        // We need to use an async runtime since Connection::execute is async
        // This is typically handled by the executor, but for the Module trait
        // we need to handle it here

        // Create a runtime to execute the async command
        let rt = tokio::runtime::Handle::try_current().map_err(|_| {
            ModuleError::ExecutionFailed("No async runtime available".to_string())
        })?;

        let connection = context.connection.clone();
        let cmd = command.clone();
        let exec = executable.clone();

        // Block on the async execution
        let result = rt.block_on(async move {
            let connection = connection.ok_or_else(|| {
                ModuleError::ExecutionFailed("No connection available for raw command".to_string())
            })?;

            let actual_command = if let Some(ref exec) = exec {
                format!("{} -c '{}'", exec, cmd.replace('\'', "'\\''"))
            } else {
                cmd.clone()
            };

            connection.execute(&actual_command, None).await.map_err(|e| {
                ModuleError::ExecutionFailed(format!("Raw command execution failed: {}", e))
            })
        })?;

        // Build output
        let mut output = if result.success {
            ModuleOutput::changed("Raw command executed successfully")
        } else {
            ModuleOutput::failed(format!(
                "Raw command failed with exit code {}",
                result.exit_code
            ))
        };

        output.stdout = Some(result.stdout.clone());
        output.stderr = Some(result.stderr.clone());
        output.rc = Some(result.exit_code);

        output = output.with_data(
            "stdout_lines",
            serde_json::json!(result.stdout.lines().collect::<Vec<_>>()),
        );
        output = output.with_data(
            "stderr_lines",
            serde_json::json!(result.stderr.lines().collect::<Vec<_>>()),
        );
        output = output.with_data("cmd", serde_json::json!(command));

        Ok(output)
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let command = self.get_command(params)?;

        // In check mode, just report what would happen
        Ok(ModuleOutput::ok(format!(
            "Would execute raw command: {}",
            command
        ))
        .with_data("cmd", serde_json::json!(command)))
    }

    fn diff(
        &self,
        _params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<Option<super::Diff>> {
        // Raw module doesn't produce meaningful diffs
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::ModuleStatus;
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn test_raw_get_command_from_raw_key() {
        let module = RawModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("raw".to_string(), Value::String("echo hello".to_string()));

        let cmd = module.get_command(&params).unwrap();
        assert_eq!(cmd, "echo hello");
    }

    #[test]
    fn test_raw_get_command_from_free_form() {
        let module = RawModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "free_form".to_string(),
            Value::String("apt-get update".to_string()),
        );

        let cmd = module.get_command(&params).unwrap();
        assert_eq!(cmd, "apt-get update");
    }

    #[test]
    fn test_raw_get_command_from_cmd() {
        let module = RawModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("cmd".to_string(), Value::String("ls -la".to_string()));

        let cmd = module.get_command(&params).unwrap();
        assert_eq!(cmd, "ls -la");
    }

    #[test]
    fn test_raw_missing_command() {
        let module = RawModule;
        let params: ModuleParams = HashMap::new();

        let result = module.get_command(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_raw_check_mode() {
        let module = RawModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("raw".to_string(), Value::String("dangerous_command".to_string()));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(!result.changed);
        assert_eq!(result.status, ModuleStatus::Ok);
        assert!(result.msg.contains("Would execute"));
        assert!(result.msg.contains("dangerous_command"));
    }

    #[test]
    fn test_raw_classification() {
        let module = RawModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_raw_validate_params() {
        let module = RawModule;

        // Valid params
        let mut params: ModuleParams = HashMap::new();
        params.insert("raw".to_string(), Value::String("echo test".to_string()));
        assert!(module.validate_params(&params).is_ok());

        // Invalid params (empty)
        let params: ModuleParams = HashMap::new();
        assert!(module.validate_params(&params).is_err());
    }
}
