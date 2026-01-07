//! Synchronize module - Rsync wrapper for file/directory synchronization
//!
//! This module provides a wrapper around rsync for efficient file and directory
//! synchronization between the control machine and remote hosts. It supports
//! various rsync options for flexible synchronization scenarios.
//!
//! # Example
//!
//! ```yaml
//! - name: Synchronize directory to remote
//!   synchronize:
//!     src: /local/path/
//!     dest: /remote/path/
//!
//! - name: Pull files from remote
//!   synchronize:
//!     src: /remote/path/
//!     dest: /local/path/
//!     mode: pull
//!
//! - name: Sync with delete (mirror)
//!   synchronize:
//!     src: /local/path/
//!     dest: /remote/path/
//!     delete: true
//!
//! - name: Sync with exclusions
//!   synchronize:
//!     src: /local/path/
//!     dest: /remote/path/
//!     rsync_opts:
//!       - "--exclude=*.log"
//!       - "--exclude=.git"
//! ```
//!
//! # Parameters
//!
//! - `src` - Source path (required)
//! - `dest` - Destination path (required)
//! - `mode` - 'push' (default) or 'pull'
//! - `delete` - Delete files in dest that don't exist in src (default: false)
//! - `recursive` - Recurse into directories (default: true)
//! - `archive` - Archive mode (preserves permissions, times, etc.) (default: true)
//! - `compress` - Compress during transfer (default: true)
//! - `checksum` - Use checksum for comparison instead of time/size (default: false)
//! - `links` - Copy symlinks as symlinks (default: true)
//! - `perms` - Preserve permissions (default: true)
//! - `times` - Preserve modification times (default: true)
//! - `owner` - Preserve owner (requires sudo) (default: false)
//! - `group` - Preserve group (requires sudo) (default: false)
//! - `rsync_path` - Path to rsync on remote (default: rsync)
//! - `rsync_opts` - Additional rsync options as list
//! - `partial` - Keep partially transferred files (default: false)
//! - `verify_host` - Verify SSH host key (default: true)
//! - `ssh_args` - Additional SSH arguments
//! - `set_remote_user` - Set remote user for rsync (default: true)

use super::{
    validate_path_param, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput,
    ModuleParams, ModuleResult, ParallelizationHint, ParamExt,
};
use std::process::Command;

/// Synchronization mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Push from control to remote (default)
    Push,
    /// Pull from remote to control
    Pull,
}

impl SyncMode {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "push" => Some(SyncMode::Push),
            "pull" => Some(SyncMode::Pull),
            _ => None,
        }
    }
}

/// Module for rsync-based file synchronization
pub struct SynchronizeModule;

impl SynchronizeModule {
    /// Build the rsync command from parameters
    fn build_rsync_command(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        src: &str,
        dest: &str,
        mode: SyncMode,
    ) -> ModuleResult<Vec<String>> {
        let mut args = Vec::new();

        // Archive mode (preserves most attributes)
        if params.get_bool_or("archive", true) {
            args.push("-a".to_string());
        } else {
            // If not archive, check individual options
            if params.get_bool_or("recursive", true) {
                args.push("-r".to_string());
            }
            if params.get_bool_or("links", true) {
                args.push("-l".to_string());
            }
            if params.get_bool_or("perms", true) {
                args.push("-p".to_string());
            }
            if params.get_bool_or("times", true) {
                args.push("-t".to_string());
            }
        }

        // Owner and group (require sudo typically)
        if params.get_bool_or("owner", false) {
            args.push("-o".to_string());
        }
        if params.get_bool_or("group", false) {
            args.push("-g".to_string());
        }

        // Compression
        if params.get_bool_or("compress", true) {
            args.push("-z".to_string());
        }

        // Checksum mode
        if params.get_bool_or("checksum", false) {
            args.push("-c".to_string());
        }

        // Delete mode
        if params.get_bool_or("delete", false) {
            args.push("--delete".to_string());
        }

        // Partial transfer
        if params.get_bool_or("partial", false) {
            args.push("--partial".to_string());
        }

        // Verbose output (based on verbosity level)
        if context.verbosity > 0 {
            args.push("-v".to_string());
        }
        if context.verbosity > 1 {
            args.push("--progress".to_string());
        }

        // Dry run in check mode
        if context.check_mode {
            args.push("-n".to_string());
        }

        // Itemize changes for diff
        if context.diff_mode {
            args.push("-i".to_string());
        }

        // Custom rsync path on remote
        if let Some(rsync_path) = params.get_string("rsync_path")? {
            args.push(format!("--rsync-path={}", rsync_path));
        }

        // SSH options
        let mut ssh_opts = Vec::new();

        // Host key verification
        if !params.get_bool_or("verify_host", true) {
            ssh_opts.push("-o StrictHostKeyChecking=no".to_string());
            ssh_opts.push("-o UserKnownHostsFile=/dev/null".to_string());
        }

        // Additional SSH arguments
        if let Some(extra_ssh) = params.get_string("ssh_args")? {
            ssh_opts.push(extra_ssh);
        }

        if !ssh_opts.is_empty() {
            args.push(format!("-e \"ssh {}\"", ssh_opts.join(" ")));
        } else {
            args.push("-e ssh".to_string());
        }

        // Additional rsync options
        if let Some(extra_opts) = params.get_vec_string("rsync_opts")? {
            for opt in extra_opts {
                args.push(opt);
            }
        }

        // Get remote host info from context or params
        let remote_host = context
            .connection
            .as_ref()
            .map(|c| c.identifier().to_string())
            .unwrap_or_else(|| "localhost".to_string());

        // Determine remote user
        let remote_user = if params.get_bool_or("set_remote_user", true) {
            context
                .become_user
                .clone()
                .or_else(|| {
                    context
                        .vars
                        .get("ansible_user")
                        .and_then(|v| v.as_str().map(String::from))
                })
                .unwrap_or_else(|| "".to_string())
        } else {
            String::new()
        };

        let remote_prefix = if remote_user.is_empty() {
            format!("{}:", remote_host)
        } else {
            format!("{}@{}:", remote_user, remote_host)
        };

        // Build source and destination based on mode
        match mode {
            SyncMode::Push => {
                args.push(src.to_string());
                args.push(format!("{}{}", remote_prefix, dest));
            }
            SyncMode::Pull => {
                args.push(format!("{}{}", remote_prefix, src));
                args.push(dest.to_string());
            }
        }

        Ok(args)
    }

    /// Execute rsync command
    fn run_rsync(&self, args: &[String]) -> ModuleResult<(String, String, i32)> {
        let output = Command::new("rsync").args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ModuleError::ExecutionFailed(
                    "rsync not found. Please install rsync on the control machine.".to_string(),
                )
            } else {
                ModuleError::ExecutionFailed(format!("Failed to execute rsync: {}", e))
            }
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        Ok((stdout, stderr, exit_code))
    }

    /// Parse rsync output to determine if changes were made
    fn parse_changes(&self, stdout: &str, itemize: bool) -> bool {
        if itemize {
            // In itemize mode, lines starting with something other than '.' indicate changes
            stdout.lines().any(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty() && !trimmed.starts_with('.')
            })
        } else {
            // Without itemize, any non-empty output typically means changes
            !stdout.trim().is_empty()
        }
    }
}

impl Module for SynchronizeModule {
    fn name(&self) -> &'static str {
        "synchronize"
    }

    fn description(&self) -> &'static str {
        "Rsync wrapper for file/directory synchronization"
    }

    fn classification(&self) -> ModuleClassification {
        // NativeTransport as it uses rsync locally
        ModuleClassification::NativeTransport
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Synchronize can be network-intensive, but can run in parallel
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        &["src", "dest"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate required parameters
        let src = params.get_string_required("src")?;
        let dest = params.get_string_required("dest")?;

        validate_path_param(&src, "src")?;
        validate_path_param(&dest, "dest")?;

        // Validate mode if specified
        if let Some(mode_str) = params.get_string("mode")? {
            if SyncMode::from_str(&mode_str).is_none() {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid mode '{}'. Use 'push' or 'pull'.",
                    mode_str
                )));
            }
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let src = params.get_string_required("src")?;
        let dest = params.get_string_required("dest")?;

        let mode = params
            .get_string("mode")?
            .and_then(|s| SyncMode::from_str(&s))
            .unwrap_or(SyncMode::Push);

        // Build rsync command
        let args = self.build_rsync_command(params, context, &src, &dest, mode)?;

        // In check mode, add dry-run flag (already handled in build_rsync_command)
        let (stdout, stderr, exit_code) = self.run_rsync(&args)?;

        // Build output
        let changed = if context.check_mode {
            // In check mode, parse output to see if changes would be made
            self.parse_changes(&stdout, context.diff_mode)
        } else {
            // In normal mode, non-zero content usually means changes
            exit_code == 0 && self.parse_changes(&stdout, context.diff_mode)
        };

        let mut output = if exit_code == 0 {
            if changed {
                ModuleOutput::changed("Synchronization complete")
            } else {
                ModuleOutput::ok("Already synchronized")
            }
        } else {
            ModuleOutput::failed(format!("rsync failed with exit code {}", exit_code))
        };

        output.stdout = Some(stdout.clone());
        output.stderr = Some(stderr.clone());
        output.rc = Some(exit_code);

        output = output.with_data("src", serde_json::json!(src));
        output = output.with_data("dest", serde_json::json!(dest));
        output = output.with_data(
            "mode",
            serde_json::json!(if mode == SyncMode::Push {
                "push"
            } else {
                "pull"
            }),
        );
        output = output.with_data("rsync_args", serde_json::json!(args.join(" ")));

        // If diff mode, include itemized changes
        if context.diff_mode && !stdout.is_empty() {
            output = output.with_diff(
                super::Diff::new("Current state", "Synchronized state")
                    .with_details(stdout.clone()),
            );
        }

        Ok(output)
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        // check mode is handled by execute with context.check_mode = true
        // which adds -n flag to rsync
        self.execute(params, context)
    }

    fn diff(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<Option<super::Diff>> {
        // Run with itemize to get detailed changes
        let diff_context = ModuleContext {
            check_mode: true,
            diff_mode: true,
            ..context.clone()
        };

        let output = self.execute(params, &diff_context)?;

        if let Some(ref stdout) = output.stdout {
            if !stdout.trim().is_empty() {
                return Ok(Some(
                    super::Diff::new("Current state", "Synchronized state")
                        .with_details(stdout.clone()),
                ));
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashMap;

    #[test]
    fn test_sync_mode_from_str() {
        assert_eq!(SyncMode::from_str("push"), Some(SyncMode::Push));
        assert_eq!(SyncMode::from_str("PUSH"), Some(SyncMode::Push));
        assert_eq!(SyncMode::from_str("pull"), Some(SyncMode::Pull));
        assert_eq!(SyncMode::from_str("PULL"), Some(SyncMode::Pull));
        assert_eq!(SyncMode::from_str("invalid"), None);
    }

    #[test]
    fn test_synchronize_validate_params_valid() {
        let module = SynchronizeModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), Value::String("/local/path/".to_string()));
        params.insert(
            "dest".to_string(),
            Value::String("/remote/path/".to_string()),
        );

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_synchronize_validate_params_missing_src() {
        let module = SynchronizeModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "dest".to_string(),
            Value::String("/remote/path/".to_string()),
        );

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_synchronize_validate_params_missing_dest() {
        let module = SynchronizeModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), Value::String("/local/path/".to_string()));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_synchronize_validate_params_invalid_mode() {
        let module = SynchronizeModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), Value::String("/local/path/".to_string()));
        params.insert(
            "dest".to_string(),
            Value::String("/remote/path/".to_string()),
        );
        params.insert("mode".to_string(), Value::String("invalid".to_string()));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_synchronize_classification() {
        let module = SynchronizeModule;
        assert_eq!(
            module.classification(),
            ModuleClassification::NativeTransport
        );
    }

    #[test]
    fn test_synchronize_parse_changes() {
        let module = SynchronizeModule;

        // No changes (all dots or empty)
        assert!(!module.parse_changes("", false));
        assert!(!module.parse_changes("", true));

        // Changes without itemize
        assert!(module.parse_changes("file.txt\n", false));

        // Changes with itemize
        assert!(module.parse_changes(">f+++++++++ file.txt\n", true));
        assert!(!module.parse_changes(".f....... file.txt\n", true));
    }

    #[test]
    fn test_synchronize_required_params() {
        let module = SynchronizeModule;
        let required = module.required_params();
        assert!(required.contains(&"src"));
        assert!(required.contains(&"dest"));
    }

    #[test]
    fn test_synchronize_validate_path_with_null() {
        let module = SynchronizeModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "src".to_string(),
            Value::String("/path/with\0null".to_string()),
        );
        params.insert(
            "dest".to_string(),
            Value::String("/remote/path/".to_string()),
        );

        assert!(module.validate_params(&params).is_err());
    }
}
