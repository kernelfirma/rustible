//! Script module - Transfer and execute a local script on the remote host
//!
//! This module copies a script from the control machine to the target host,
//! makes it executable, runs it, and then removes it. This is useful when
//! you have a complex script that needs to run on remote hosts.
//!
//! # Example
//!
//! ```yaml
//! - name: Run a script with arguments
//!   script: /some/local/script.sh arg1 arg2
//!
//! - name: Run a Python script
//!   script: /path/to/script.py
//!   args:
//!     executable: /usr/bin/python3
//!
//! - name: Run script only if file doesn't exist
//!   script: /path/to/script.sh
//!   args:
//!     creates: /path/to/marker_file
//!
//! - name: Run script only if file exists (will be removed)
//!   script: /path/to/script.sh
//!   args:
//!     removes: /path/to/file_to_check
//! ```
//!
//! # Parameters
//!
//! - `script` / `free_form` / `src` - Path to the local script (required)
//! - `executable` - Override the interpreter for script execution
//! - `creates` - Path on remote; if exists, skip execution
//! - `removes` - Path on remote; if missing, skip execution
//! - `chdir` - Change to this directory before executing
//! - `decrypt` - Decrypt ansible-vault encrypted script (default: true)

use super::{
    get_remote_tmp, validate_command_args, validate_path_param, Module, ModuleClassification,
    ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParallelizationHint,
    ParamExt,
};
use crate::utils::shell_escape;
use std::path::PathBuf;

/// Module for transferring and executing scripts on remote hosts
pub struct ScriptModule;

impl ScriptModule {
    /// Get the script path from parameters
    fn get_script_path(&self, params: &ModuleParams) -> ModuleResult<String> {
        // Try various parameter names
        if let Some(path) = params.get_string("script")? {
            return Ok(path);
        }

        if let Some(path) = params.get_string("free_form")? {
            return Ok(path);
        }

        if let Some(path) = params.get_string("src")? {
            return Ok(path);
        }

        if let Some(path) = params.get_string("_raw_params")? {
            return Ok(path);
        }

        Err(ModuleError::MissingParameter(
            "script path is required. Specify the local script path.".to_string(),
        ))
    }

    /// Parse script path and arguments from free-form input
    /// e.g., "/path/to/script.sh arg1 arg2" -> ("/path/to/script.sh", vec!["arg1", "arg2"])
    fn parse_script_and_args(&self, input: &str) -> (String, Vec<String>) {
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.is_empty() {
            return (String::new(), Vec::new());
        }

        let script_path = parts[0].to_string();
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

        (script_path, args)
    }

    /// Check if creates/removes conditions allow execution
    fn should_execute(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<bool> {
        let connection = match context.connection.as_ref() {
            Some(c) => c,
            None => return Ok(true), // No connection, assume we should execute
        };

        // Check 'creates' condition - skip if path exists
        if let Some(creates_path) = params.get_string("creates")? {
            validate_path_param(&creates_path, "creates")?;

            let rt = tokio::runtime::Handle::try_current().map_err(|_| {
                ModuleError::ExecutionFailed("No async runtime available".to_string())
            })?;

            let conn = connection.clone();
            let path = PathBuf::from(&creates_path);
            let exists = std::thread::scope(|s| {
                s.spawn(|| rt.block_on(async move { conn.path_exists(&path).await }))
                    .join()
                    .unwrap()
            });

            if exists.unwrap_or(false) {
                return Ok(false);
            }
        }

        // Check 'removes' condition - skip if path doesn't exist
        if let Some(removes_path) = params.get_string("removes")? {
            validate_path_param(&removes_path, "removes")?;

            let rt = tokio::runtime::Handle::try_current().map_err(|_| {
                ModuleError::ExecutionFailed("No async runtime available".to_string())
            })?;

            let conn = connection.clone();
            let path = PathBuf::from(&removes_path);
            let exists = std::thread::scope(|s| {
                s.spawn(|| rt.block_on(async move { conn.path_exists(&path).await }))
                    .join()
                    .unwrap()
            });

            if !exists.unwrap_or(true) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Generate a unique temporary path on the remote system
    fn generate_temp_path(&self, context: &ModuleContext) -> String {
        let uuid = uuid::Uuid::new_v4();
        let remote_tmp = get_remote_tmp(context);
        format!("{}/.ansible_script_{}.tmp", remote_tmp, uuid.simple())
    }
}

impl Module for ScriptModule {
    fn name(&self) -> &'static str {
        "script"
    }

    fn description(&self) -> &'static str {
        "Transfer and execute a local script on remote hosts"
    }

    fn classification(&self) -> ModuleClassification {
        // NativeTransport because we upload then execute
        ModuleClassification::NativeTransport
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Scripts can generally run in parallel across hosts
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate we have a script path
        let script_input = self.get_script_path(params)?;
        let (script_path, _) = self.parse_script_and_args(&script_input);

        if script_path.is_empty() {
            return Err(ModuleError::MissingParameter("script path".to_string()));
        }

        // Validate executable if present
        if let Some(executable) = params.get_string("executable")? {
            validate_command_args(&executable)?;
        }

        // Validate creates/removes paths if present
        if let Some(creates) = params.get_string("creates")? {
            validate_path_param(&creates, "creates")?;
        }

        if let Some(removes) = params.get_string("removes")? {
            validate_path_param(&removes, "removes")?;
        }

        // Validate executable parameter for security
        if let Some(executable) = params.get_string("executable")? {
            validate_command_args(&executable)?;
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Get the script path and arguments
        let script_input = self.get_script_path(params)?;
        let (script_path, args) = self.parse_script_and_args(&script_input);

        // Get additional parameters
        let executable = params.get_string("executable")?;
        let chdir = params.get_string("chdir")?;

        // In check mode, report what would happen
        if context.check_mode {
            return Ok(ModuleOutput::ok(format!(
                "Would execute script: {} {}",
                script_path,
                args.join(" ")
            ))
            .with_data("script", serde_json::json!(script_path))
            .with_data("args", serde_json::json!(args)));
        }

        // Check creates/removes conditions
        if !self.should_execute(params, context)? {
            return Ok(
                ModuleOutput::skipped("Skipped due to creates/removes condition")
                    .with_data("script", serde_json::json!(script_path)),
            );
        }

        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed("No connection available for script execution".to_string())
        })?;

        // Read the local script
        let local_script = std::fs::read(&script_path).map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to read local script '{}': {}",
                script_path, e
            ))
        })?;

        // Generate remote temporary path
        let remote_path = self.generate_temp_path(context);
        let remote_path_buf = PathBuf::from(&remote_path);

        // Use async runtime for connection operations
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No async runtime available".to_string()))?;

        // Upload the script
        let conn = connection.clone();
        let script_bytes = local_script.clone();
        let path = remote_path_buf.clone();
        std::thread::scope(|s| {
            s.spawn(|| {
                rt.block_on(async move { conn.upload_content(&script_bytes, &path, None).await })
            })
            .join()
            .unwrap()
        })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to upload script: {}", e)))?;

        // Make the script executable
        let conn = connection.clone();
        // Use shell_escape for safety, although remote_path is a UUID
        let chmod_cmd = format!("chmod +x {}", shell_escape(&remote_path));
        std::thread::scope(|s| {
            s.spawn(|| rt.block_on(async move { conn.execute(&chmod_cmd, None).await }))
                .join()
                .unwrap()
        })
        .map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to make script executable: {}", e))
        })?;

        // Build the execution command
        // We use shell_escape for chdir and args to prevent command injection
        let exec_cmd = if let Some(ref exec) = executable {
            // Parse executable string into parts and escape each part to prevent injection
            // This handles cases where executable contains arguments (e.g. "python3 -u")
            // while preventing shell injection attacks.
            let parts = shell_words::split(exec).map_err(|e| {
                ModuleError::InvalidParameter(format!("Invalid executable string: {}", e))
            })?;
            let safe_exec = parts
                .iter()
                .map(|p| shell_escape(p))
                .collect::<Vec<_>>()
                .join(" ");

            if let Some(ref dir) = chdir {
                format!(
                    "cd {} && {} {} {}",
                    shell_escape(dir),
                    safe_exec,
                    shell_escape(&remote_path),
                    args.iter()
                        .map(|a| shell_escape(a))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            } else {
                format!(
                    "{} {} {}",
                    safe_exec,
                    shell_escape(&remote_path),
                    args.iter()
                        .map(|a| shell_escape(a))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
        } else if let Some(ref dir) = chdir {
            format!(
                "cd {} && {} {}",
                shell_escape(dir),
                shell_escape(&remote_path),
                args.iter()
                    .map(|a| shell_escape(a))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        } else {
            format!(
                "{} {}",
                shell_escape(&remote_path),
                args.iter()
                    .map(|a| shell_escape(a))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        };

        // Execute the script
        let conn = connection.clone();
        let result = std::thread::scope(|s| {
            s.spawn(|| rt.block_on(async move { conn.execute(&exec_cmd, None).await }))
                .join()
                .unwrap()
        })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Script execution failed: {}", e)))?;

        // Clean up the temporary script
        let conn = connection.clone();
        // Use shell_escape for safety
        let rm_cmd = format!("rm -f {}", shell_escape(&remote_path));
        let _ = std::thread::scope(|s| {
            s.spawn(|| rt.block_on(async move { conn.execute(&rm_cmd, None).await }))
                .join()
                .unwrap()
        });

        // Build output
        let mut output = if result.success {
            ModuleOutput::changed("Script executed successfully")
        } else {
            ModuleOutput::failed(format!("Script failed with exit code {}", result.exit_code))
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
        output = output.with_data("script", serde_json::json!(script_path));
        output = output.with_data("args", serde_json::json!(args));

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn test_script_get_path_from_script_key() {
        let module = ScriptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "script".to_string(),
            Value::String("/path/to/script.sh".to_string()),
        );

        let path = module.get_script_path(&params).unwrap();
        assert_eq!(path, "/path/to/script.sh");
    }

    #[test]
    fn test_script_get_path_from_free_form() {
        let module = ScriptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "free_form".to_string(),
            Value::String("/path/to/script.sh arg1 arg2".to_string()),
        );

        let path = module.get_script_path(&params).unwrap();
        assert_eq!(path, "/path/to/script.sh arg1 arg2");
    }

    #[test]
    fn test_script_parse_script_and_args() {
        let module = ScriptModule;

        let (path, args) = module.parse_script_and_args("/path/to/script.sh");
        assert_eq!(path, "/path/to/script.sh");
        assert!(args.is_empty());

        let (path, args) = module.parse_script_and_args("/path/to/script.sh arg1 arg2");
        assert_eq!(path, "/path/to/script.sh");
        assert_eq!(args, vec!["arg1", "arg2"]);

        let (path, args) = module.parse_script_and_args("script.py --flag value");
        assert_eq!(path, "script.py");
        assert_eq!(args, vec!["--flag", "value"]);
    }

    #[test]
    fn test_script_missing_path() {
        let module = ScriptModule;
        let params: ModuleParams = HashMap::new();

        let result = module.get_script_path(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_script_classification() {
        let module = ScriptModule;
        assert_eq!(
            module.classification(),
            ModuleClassification::NativeTransport
        );
    }

    #[test]
    fn test_script_generate_temp_path() {
        use crate::modules::{ModuleContext, ModuleContextBuilder};
        let module = ScriptModule;
        let context = ModuleContext::default();

        let path1 = module.generate_temp_path(&context);
        let path2 = module.generate_temp_path(&context);

        // Should start with expected prefix (default /tmp)
        assert!(path1.starts_with("/tmp/.ansible_script_"));
        assert!(path1.ends_with(".tmp"));

        // Should be unique
        assert_ne!(path1, path2);

        // Test with custom remote_tmp
        let context_custom = ModuleContextBuilder::new()
            .var("ansible_remote_tmp", serde_json::json!("/var/tmp"))
            .build()
            .unwrap();
        let path3 = module.generate_temp_path(&context_custom);
        assert!(path3.starts_with("/var/tmp/.ansible_script_"));
    }

    #[test]
    fn test_script_validate_params_valid() {
        let module = ScriptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "script".to_string(),
            Value::String("/path/to/script.sh".to_string()),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_script_validate_params_with_creates() {
        let module = ScriptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "script".to_string(),
            Value::String("/path/to/script.sh".to_string()),
        );
        params.insert(
            "creates".to_string(),
            Value::String("/path/to/marker".to_string()),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_script_validate_params_invalid_creates() {
        let module = ScriptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "script".to_string(),
            Value::String("/path/to/script.sh".to_string()),
        );
        params.insert(
            "creates".to_string(),
            Value::String("/path/with\0null".to_string()),
        );

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_script_validate_params_invalid_executable() {
        let module = ScriptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "script".to_string(),
            Value::String("/path/to/script.sh".to_string()),
        );
        params.insert(
            "executable".to_string(),
            Value::String("/bin/bash; rm -rf /".to_string()),
        );

        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("potentially dangerous pattern"));
    }

    #[test]
    fn test_cmd_construction_logic() {
        // This test simulates the command construction logic in execute()
        // since we cannot easily call execute() without a full context.
        use crate::utils::shell_escape;

        let remote_path = "/tmp/.ansible_script_123.tmp";
        let args = vec!["arg1", "arg with space"];
        let chdir = Some("/tmp/dir with space");
        let executable = Some("/bin/bash");

        // Logic from execute()
        let exec_cmd = if let Some(ref exec) = executable {
            let parts = shell_words::split(exec).unwrap();
            let safe_exec = parts
                .iter()
                .map(|p| shell_escape(p))
                .collect::<Vec<_>>()
                .join(" ");

            if let Some(ref dir) = chdir {
                format!(
                    "cd {} && {} {} {}",
                    shell_escape(dir),
                    safe_exec,
                    shell_escape(remote_path),
                    args.iter()
                        .map(|a| shell_escape(a))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            } else {
                // Not reached in this test
                String::new()
            }
        } else {
            // Not reached in this test
            String::new()
        };

        // Expected output:
        // cd '/tmp/dir with space' && /bin/bash '/tmp/.ansible_script_123.tmp' arg1 'arg with space'
        // Note: shell_escape returns quoted string if spaces are present.

        assert_eq!(
            exec_cmd,
            "cd '/tmp/dir with space' && /bin/bash /tmp/.ansible_script_123.tmp arg1 'arg with space'"
        );
    }

    #[test]
    fn test_script_executable_validation() {
        let module = ScriptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "script".to_string(),
            Value::String("/path/to/script.sh".to_string()),
        );
        // Dangerous executable that should be blocked
        params.insert(
            "executable".to_string(),
            Value::String("bash; rm -rf /".to_string()),
        );

        // Should now fail with InvalidParameter
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_script_executable_safe() {
        let module = ScriptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "script".to_string(),
            Value::String("/path/to/script.sh".to_string()),
        );
        params.insert(
            "executable".to_string(),
            Value::String("/usr/bin/python3".to_string()),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_cmd_construction_injection() {
        use crate::utils::shell_escape;

        let remote_path = "/tmp/.ansible_script_123.tmp";
        let args = vec!["arg1"];
        let chdir: Option<&str> = None;
        let executable = Some("perl -e 'print \"pwned\"' #"); // This passes validation!

        // Logic from execute() that we want to test/fix
        let exec_cmd = if let Some(ref exec) = executable {
            // New secure logic
            let parts = shell_words::split(exec).unwrap();
            let safe_exec = parts
                .iter()
                .map(|p| shell_escape(p))
                .collect::<Vec<_>>()
                .join(" ");

             format!(
                "{} {} {}",
                safe_exec,
                shell_escape(&remote_path),
                args.iter()
                    .map(|a| shell_escape(a))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        } else {
             String::new()
        };

        // Injection should be neutralized (comment stripped, parts escaped)
        // "perl" -> perl
        // "-e" -> -e
        // "'print \"pwned\"'" -> 'print "pwned"'
        // "#" -> (comment stripped)
        assert_eq!(
            exec_cmd,
            "perl -e 'print \"pwned\"' /tmp/.ansible_script_123.tmp arg1"
        );
    }
}
