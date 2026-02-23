//! Git module - Git repository management
//!
//! This module manages git repositories including cloning, updating,
//! and checking out specific versions or branches.

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Configuration for SSH key handling
#[derive(Debug, Clone, Default)]
struct SshConfig {
    key_file: Option<String>,
    ssh_opts: Option<String>,
    accept_hostkey: bool,
}

impl SshConfig {
    /// Build the GIT_SSH_COMMAND environment variable
    fn build_ssh_command(&self) -> Option<String> {
        let mut parts: Vec<std::borrow::Cow<'_, str>> = vec![std::borrow::Cow::Borrowed("ssh")];

        if let Some(key) = &self.key_file {
            parts.push(std::borrow::Cow::Borrowed("-i"));
            parts.push(shell_escape(key));
            // Disable other key sources when using specific key
            parts.push(std::borrow::Cow::Borrowed("-o"));
            parts.push(shell_escape("IdentitiesOnly=yes"));
        }

        if self.accept_hostkey {
            parts.push(std::borrow::Cow::Borrowed("-o"));
            parts.push(shell_escape("StrictHostKeyChecking=no"));
            parts.push(std::borrow::Cow::Borrowed("-o"));
            parts.push(shell_escape("UserKnownHostsFile=/dev/null"));
        }

        if let Some(opts) = &self.ssh_opts {
            // Options might contain spaces, so we should escape them to be safe
            // when git passes them to the shell
            parts.push(shell_escape(opts));
        }

        if parts.len() > 1 {
            Some(parts.join(" "))
        } else {
            None
        }
    }

    /// Apply SSH configuration to ExecuteOptions
    fn apply_to_options(&self, options: &mut ExecuteOptions) {
        if let Some(ssh_cmd) = self.build_ssh_command() {
            options.env.insert("GIT_SSH_COMMAND".to_string(), ssh_cmd);
        }
    }
}

/// Configuration for clone operations
#[derive(Debug, Clone, Default)]
struct CloneConfig {
    bare: bool,
    depth: Option<u32>,
    single_branch: bool,
    recursive: bool,
    separate_git_dir: Option<String>,
    refspec: Option<String>,
}

/// Module for git repository management
pub struct GitModule;

impl GitModule {
    /// Get execution options with become support if needed
    fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
        let mut options = ExecuteOptions::new();
        if context.r#become {
            options = options.with_escalation(context.become_user.clone());
            if let Some(ref method) = context.become_method {
                options.escalate_method = Some(method.clone());
            }
            if let Some(ref password) = context.become_password {
                options.escalate_password = Some(password.clone());
            }
        }
        options
    }

    /// Build execution options with SSH configuration applied
    fn build_git_exec_options(
        context: &ModuleContext,
        ssh_config: &SshConfig,
    ) -> ExecuteOptions {
        let mut options = Self::get_exec_options(context);
        ssh_config.apply_to_options(&mut options);
        options
    }

    /// Execute a command via connection
    fn run_command(
        connection: &Arc<dyn Connection + Send + Sync>,
        command: &str,
        options: ExecuteOptions,
    ) -> ModuleResult<(bool, String, String)> {
        let connection = connection.clone();
        let command = command.to_string();
        let fut = async move { connection.execute(&command, Some(options)).await };

        let result = if let Ok(handle) = Handle::try_current() {
            std::thread::scope(|s| s.spawn(move || handle.block_on(fut)).join()).map_err(|_| {
                ModuleError::ExecutionFailed("Tokio runtime thread panicked".to_string())
            })?
        } else {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to create tokio runtime: {}", e))
                })?;
            rt.block_on(fut)
        }
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Execute a git command via connection, requiring success
    fn run_git_command(
        connection: &Arc<dyn Connection + Send + Sync>,
        command: &str,
        options: ExecuteOptions,
        error_msg: &str,
    ) -> ModuleResult<String> {
        let (success, stdout, stderr) = Self::run_command(connection, command, options)?;
        if !success {
            return Err(ModuleError::CommandFailed {
                code: -1,
                message: format!("{}: {}", error_msg, stderr),
            });
        }
        Ok(stdout)
    }

    /// Check if git is installed on the remote host
    fn check_git_installed(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let options = Self::get_exec_options(context);
        let (success, _, _) = Self::run_command(connection, "git --version", options)?;
        Ok(success)
    }

    /// Check if a directory is a git repository via connection
    fn is_git_repo(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let escaped = shell_escape(dest);
        // Check for .git directory (normal repo) or bare repo (HEAD + objects)
        let cmd = format!(
            "test -d {}/.git || ( test -f {}/HEAD && test -d {}/objects )",
            escaped, escaped, escaped
        );
        let options = Self::get_exec_options(context);
        let (success, _, _) = Self::run_command(connection, &cmd, options)?;
        Ok(success)
    }

    /// Check if a directory is a bare git repository via connection
    fn is_bare_repo(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let escaped = shell_escape(dest);
        let cmd = format!(
            "test -f {}/HEAD && test -d {}/objects",
            escaped, escaped
        );
        let options = Self::get_exec_options(context);
        let (success, _, _) = Self::run_command(connection, &cmd, options)?;
        Ok(success)
    }

    /// Get the current HEAD commit hash
    fn get_current_version(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        let cmd = format!("git -C {} rev-parse HEAD", shell_escape(dest));
        let options = Self::get_exec_options(context);
        let (success, stdout, _) = Self::run_command(connection, &cmd, options)?;

        if success {
            Ok(Some(stdout.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    /// Get the current branch name
    fn get_current_branch(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        let cmd = format!(
            "git -C {} rev-parse --abbrev-ref HEAD",
            shell_escape(dest)
        );
        let options = Self::get_exec_options(context);
        let (success, stdout, _) = Self::run_command(connection, &cmd, options)?;

        if success {
            let branch = stdout.trim().to_string();
            if branch == "HEAD" {
                // Detached HEAD state
                Ok(None)
            } else {
                Ok(Some(branch))
            }
        } else {
            Ok(None)
        }
    }

    /// Get the remote URL of the repository
    fn get_remote_url(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        remote: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<String>> {
        let cmd = format!(
            "git -C {} config --get remote.{}.url",
            shell_escape(dest),
            shell_escape(remote)
        );
        let options = Self::get_exec_options(context);
        let (success, stdout, _) = Self::run_command(connection, &cmd, options)?;

        if success {
            Ok(Some(stdout.trim().to_string()))
        } else {
            Ok(None)
        }
    }

    /// Get list of local changes (for diff output)
    #[allow(dead_code)]
    fn get_local_changes(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Vec<String>> {
        let cmd = format!("git -C {} status --porcelain", shell_escape(dest));
        let options = Self::get_exec_options(context);
        let (success, stdout, _) = Self::run_command(connection, &cmd, options)?;

        if success {
            let changes: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();
            Ok(changes)
        } else {
            Ok(vec![])
        }
    }

    /// Get commit log between two versions
    fn get_commit_log(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        from: &str,
        to: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Vec<String>> {
        let cmd = format!(
            "git -C {} log --oneline {}..{}",
            shell_escape(dest),
            shell_escape(from),
            shell_escape(to)
        );
        let options = Self::get_exec_options(context);
        let (success, stdout, _) = Self::run_command(connection, &cmd, options)?;

        if success {
            let commits: Vec<String> = stdout.lines().map(|s| s.to_string()).collect();
            Ok(commits)
        } else {
            Ok(vec![])
        }
    }

    /// Verify GPG signature of a commit
    fn verify_commit(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        commit: &str,
        gpg_whitelist: &[String],
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = format!(
            "git -C {} verify-commit --raw {}",
            shell_escape(dest),
            shell_escape(commit)
        );
        let options = Self::get_exec_options(context);
        let (success, _, stderr) = Self::run_command(connection, &cmd, options)?;

        if !success {
            return Ok(false);
        }

        // If whitelist is empty, any valid signature is accepted
        if gpg_whitelist.is_empty() {
            return Ok(true);
        }

        // Check if the signature key is in the whitelist
        for fingerprint in gpg_whitelist {
            if stderr.contains(fingerprint) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Discard local modifications
    fn reset_hard(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let escaped = shell_escape(dest);
        let cmd = format!("git -C {} reset --hard HEAD", escaped);
        let options = Self::get_exec_options(context);
        let (success, _, stderr) = Self::run_command(connection, &cmd, options)?;

        if !success {
            return Err(ModuleError::CommandFailed {
                code: -1,
                message: stderr,
            });
        }

        // Also clean untracked files
        let clean_cmd = format!("git -C {} clean -fd", escaped);
        let options = Self::get_exec_options(context);
        let _ = Self::run_command(connection, &clean_cmd, options);

        Ok(())
    }

    /// Build umask prefix for a command
    fn umask_prefix(umask: Option<&str>) -> String {
        if let Some(mask) = umask {
            format!("umask {} && ", shell_escape(mask))
        } else {
            String::new()
        }
    }

    /// Clone a git repository
    #[allow(clippy::too_many_arguments)]
    fn clone_repo(
        connection: &Arc<dyn Connection + Send + Sync>,
        repo: &str,
        dest: &str,
        version: Option<&str>,
        clone_config: &CloneConfig,
        ssh_config: &SshConfig,
        remote: &str,
        umask: Option<&str>,
        track_submodules: bool,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut parts = vec!["git".to_string(), "clone".to_string()];

        // Bare repository
        if clone_config.bare {
            parts.push("--bare".to_string());
        }

        // Shallow clone
        if let Some(d) = clone_config.depth {
            parts.push("--depth".to_string());
            parts.push(d.to_string());
        }

        // Single branch (implied with depth, but can be explicit)
        if clone_config.single_branch {
            parts.push("--single-branch".to_string());
        }

        // Separate git directory
        if let Some(ref git_dir) = clone_config.separate_git_dir {
            parts.push("--separate-git-dir".to_string());
            parts.push(shell_escape(git_dir).into_owned());
        }

        // Branch/tag to clone
        if let Some(v) = version {
            parts.push("--branch".to_string());
            parts.push(shell_escape(v).into_owned());
        }

        // Custom remote name
        if remote != "origin" {
            parts.push("--origin".to_string());
            parts.push(shell_escape(remote).into_owned());
        }

        // Recursive submodules
        if clone_config.recursive {
            parts.push("--recurse-submodules".to_string());
            if track_submodules {
                parts.push("--remote-submodules".to_string());
            }
        }

        parts.push(shell_escape(repo).into_owned());
        parts.push(shell_escape(dest).into_owned());

        let prefix = Self::umask_prefix(umask);
        let cmd = format!("{}{}", prefix, parts.join(" "));
        let options = Self::build_git_exec_options(context, ssh_config);

        Self::run_git_command(connection, &cmd, options, "Failed to clone repository")?;

        // If refspec is specified, fetch it separately
        if let Some(ref refspec) = clone_config.refspec {
            let fetch_cmd = format!(
                "git -C {} fetch {} {}",
                shell_escape(dest),
                shell_escape(remote),
                shell_escape(refspec)
            );
            let options = Self::build_git_exec_options(context, ssh_config);
            Self::run_git_command(
                connection,
                &fetch_cmd,
                options,
                &format!("Failed to fetch refspec '{}'", refspec),
            )?;
        }

        let new_version =
            Self::get_current_version(connection, dest, context)?.unwrap_or_else(|| "unknown".to_string());

        Ok(
            ModuleOutput::changed(format!("Cloned repository '{}' to '{}'", repo, dest))
                .with_data("after", serde_json::json!(new_version))
                .with_data("before", serde_json::json!(null))
                .with_diff(Diff::new(
                    "repository: absent",
                    format!(
                        "repository: {} @ {}",
                        repo,
                        &new_version[..8.min(new_version.len())]
                    ),
                )),
        )
    }

    /// Update (pull) a git repository
    #[allow(clippy::too_many_arguments)]
    fn update_repo(
        connection: &Arc<dyn Connection + Send + Sync>,
        dest: &str,
        version: Option<&str>,
        remote: &str,
        ssh_config: &SshConfig,
        force: bool,
        track_submodules: bool,
        refspec: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String, String, Vec<String>)> {
        let escaped_dest = shell_escape(dest);

        // Get current version before update
        let before_version =
            Self::get_current_version(connection, dest, context)?.unwrap_or_else(|| "unknown".to_string());

        if context.check_mode {
            return Ok((false, before_version.clone(), before_version, vec![]));
        }

        // If force, reset local changes first
        if force {
            Self::reset_hard(connection, dest, context)?;
        }

        // Fetch updates
        let fetch_cmd = if let Some(rs) = refspec {
            format!(
                "git -C {} fetch {} {}",
                escaped_dest,
                shell_escape(remote),
                shell_escape(rs)
            )
        } else {
            format!(
                "git -C {} fetch {}",
                escaped_dest,
                shell_escape(remote)
            )
        };
        let options = Self::build_git_exec_options(context, ssh_config);
        let (success, _, stderr) = Self::run_command(connection, &fetch_cmd, options)?;
        if !success {
            return Err(ModuleError::CommandFailed {
                code: -1,
                message: stderr,
            });
        }

        // Checkout the specified version or default branch
        let checkout_target = if let Some(v) = version {
            shell_escape(v).into_owned()
        } else {
            format!("{}/HEAD", shell_escape(remote))
        };

        let checkout_cmd = format!(
            "git -C {} checkout {}",
            escaped_dest, checkout_target
        );
        let options = Self::get_exec_options(context);
        let (success, _, stderr) = Self::run_command(connection, &checkout_cmd, options)?;
        if !success {
            return Err(ModuleError::CommandFailed {
                code: -1,
                message: stderr,
            });
        }

        // If on a branch, pull the latest changes
        let current_branch = Self::get_current_branch(connection, dest, context)?;
        if current_branch.is_some() {
            let pull_flag = if force { "--force" } else { "--ff-only" };
            let pull_cmd = format!(
                "git -C {} pull {}",
                escaped_dest, pull_flag
            );
            let options = Self::build_git_exec_options(context, ssh_config);
            let _ = Self::run_command(connection, &pull_cmd, options);
        }

        // Update submodules if needed
        if track_submodules {
            let submodule_cmd = format!(
                "git -C {} submodule update --init --recursive --remote",
                escaped_dest
            );
            let options = Self::build_git_exec_options(context, ssh_config);
            let _ = Self::run_command(connection, &submodule_cmd, options);
        }

        // Get version after update
        let after_version =
            Self::get_current_version(connection, dest, context)?.unwrap_or_else(|| "unknown".to_string());

        // Get commit log if versions differ
        let commits = if before_version != after_version {
            Self::get_commit_log(connection, dest, &before_version, &after_version, context)
                .unwrap_or_default()
        } else {
            vec![]
        };

        let changed = before_version != after_version;
        Ok((changed, after_version, before_version, commits))
    }
}

impl Module for GitModule {
    fn name(&self) -> &'static str {
        "git"
    }

    fn description(&self) -> &'static str {
        "Manage git repositories - clone, update, and checkout versions"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn required_params(&self) -> &[&'static str] {
        &["repo", "dest"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate required parameters
        if params.get("repo").is_none() {
            return Err(ModuleError::MissingParameter("repo".to_string()));
        }
        if params.get("dest").is_none() {
            return Err(ModuleError::MissingParameter("dest".to_string()));
        }

        // Security: Prevent argument injection via repo
        if let Some(repo) = params.get_string("repo")? {
            if repo.trim().starts_with('-') {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid repo: '{}'. Repo URL cannot start with '-' to prevent argument injection.",
                    repo
                )));
            }
        }

        // Security: Prevent argument injection via remote
        if let Some(remote) = params.get_string("remote")? {
            if remote.trim().starts_with('-') {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid remote: '{}'. Remote name cannot start with '-' to prevent argument injection.",
                    remote
                )));
            }
        }

        // Validate depth if provided
        if let Some(depth) = params.get_u32("depth")? {
            if depth == 0 {
                return Err(ModuleError::InvalidParameter(
                    "depth must be greater than 0".to_string(),
                ));
            }
        }

        // Validate key_file path if provided
        if let Some(key_file) = params.get_string("key_file")? {
            if !Path::new(&key_file).exists() {
                return Err(ModuleError::InvalidParameter(format!(
                    "SSH key file does not exist: {}",
                    key_file
                )));
            }
        }

        // Validate umask format if provided
        if let Some(umask) = params.get_string("umask")? {
            // Umask should be an octal number like "0022" or "022"
            if !umask.chars().all(|c| c.is_ascii_digit() && c < '8') {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid umask format: {}. Expected octal number like '0022'",
                    umask
                )));
            }
        }

        // Security: Prevent argument injection via refspec
        if let Some(refspec) = params.get_string("refspec")? {
            if refspec.trim().starts_with('-') {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid refspec: '{}'. Refspecs cannot start with '-' to prevent argument injection.",
                    refspec
                )));
            }
        }

        // Security: Prevent argument injection via version (branch/tag)
        if let Some(version) = params.get_string("version")? {
            if version.trim().starts_with('-') {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid version: '{}'. Version/branch names cannot start with '-' to prevent argument injection.",
                    version
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
        // Get connection from context
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "Git module requires a connection for remote execution".to_string(),
            )
        })?;

        // Extract required parameters
        let repo = params.get_string_required("repo")?;
        let dest = params.get_string_required("dest")?;

        // Extract optional parameters
        let version = params.get_string("version")?;
        let depth = params.get_u32("depth")?;
        let update = params.get_bool_or("update", true);
        let clone = params.get_bool_or("clone", true);
        let force = params.get_bool_or("force", false);
        let bare = params.get_bool_or("bare", false);
        let recursive = params.get_bool_or("recursive", true);
        let single_branch = params.get_bool_or("single_branch", false);
        let track_submodules = params.get_bool_or("track_submodules", false);
        let verify_commit = params.get_bool_or("verify_commit", false);

        // SSH configuration
        let key_file = params.get_string("key_file")?;
        let ssh_opts = params.get_string("ssh_opts")?;
        let accept_hostkey = params.get_bool_or("accept_hostkey", false);

        // Other options
        let remote = params
            .get_string("remote")?
            .unwrap_or_else(|| "origin".to_string());
        let refspec = params.get_string("refspec")?;
        let separate_git_dir = params.get_string("separate_git_dir")?;
        let umask = params.get_string("umask")?;

        // GPG verification whitelist
        let gpg_whitelist: Vec<String> = params
            .get("gpg_whitelist")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Build SSH configuration
        let ssh_config = SshConfig {
            key_file,
            ssh_opts,
            accept_hostkey,
        };

        // Build clone configuration
        let clone_config = CloneConfig {
            bare,
            depth,
            single_branch,
            recursive,
            separate_git_dir,
            refspec: refspec.clone(),
        };

        // Check if git is installed
        if !Self::check_git_installed(connection, context)? {
            return Err(ModuleError::ExecutionFailed(
                "git is not installed on the system".to_string(),
            ));
        }

        // Check if destination exists and is a git repo
        let is_repo = Self::is_git_repo(connection, &dest, context)?;

        if !is_repo {
            // Repository doesn't exist
            if !clone {
                return Ok(ModuleOutput::ok(format!(
                    "Repository not present at '{}' and clone=false",
                    dest
                )));
            }

            // Clone it
            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would clone repository '{}' to '{}'",
                    repo, dest
                ))
                .with_diff(Diff::new(
                    "repository: absent",
                    format!("repository: {}", repo),
                )));
            }

            return Self::clone_repo(
                connection,
                &repo,
                &dest,
                version.as_deref(),
                &clone_config,
                &ssh_config,
                &remote,
                umask.as_deref(),
                track_submodules,
                context,
            );
        }

        // Repository exists - check if it's the same repo
        let current_remote = Self::get_remote_url(connection, &dest, &remote, context)?;
        if let Some(ref current) = current_remote {
            if current != &repo {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Destination '{}' is a git repository for '{}', not '{}'",
                    dest, current, repo
                )));
            }
        } else {
            return Err(ModuleError::ExecutionFailed(format!(
                "Destination '{}' exists but is not a valid git repository",
                dest
            )));
        }

        // Repository exists and is correct, update if requested
        if update {
            let (changed, new_version, old_version, commits) = Self::update_repo(
                connection,
                &dest,
                version.as_deref(),
                &remote,
                &ssh_config,
                force,
                track_submodules,
                refspec.as_deref(),
                context,
            )?;

            // Verify GPG signature if requested
            if verify_commit
                && changed
                && !Self::verify_commit(connection, &dest, &new_version, &gpg_whitelist, context)?
            {
                return Err(ModuleError::ExecutionFailed(format!(
                    "GPG signature verification failed for commit {}",
                    new_version
                )));
            }

            if changed {
                // Build detailed diff output
                let diff_before = format!(
                    "commit: {}\nbranch: {}",
                    &old_version[..8.min(old_version.len())],
                    Self::get_current_branch(connection, &dest, context)?
                        .unwrap_or_else(|| "detached".to_string())
                );

                let mut diff_after = format!(
                    "commit: {}\nbranch: {}",
                    &new_version[..8.min(new_version.len())],
                    Self::get_current_branch(connection, &dest, context)?
                        .unwrap_or_else(|| "detached".to_string())
                );

                if !commits.is_empty() {
                    diff_after.push_str("\n\nNew commits:");
                    for commit in commits.iter().take(10) {
                        diff_after.push_str(&format!("\n  {}", commit));
                    }
                    if commits.len() > 10 {
                        diff_after.push_str(&format!("\n  ... and {} more", commits.len() - 10));
                    }
                }

                Ok(ModuleOutput::changed(format!(
                    "Updated repository from '{}' to '{}'",
                    &old_version[..8.min(old_version.len())],
                    &new_version[..8.min(new_version.len())]
                ))
                .with_data("before", serde_json::json!(old_version))
                .with_data("after", serde_json::json!(new_version))
                .with_data("commits", serde_json::json!(commits))
                .with_data("remote_url_changed", serde_json::json!(false))
                .with_diff(Diff::new(diff_before, diff_after)))
            } else {
                Ok(ModuleOutput::ok(format!(
                    "Repository already at version '{}'",
                    &new_version[..8.min(new_version.len())]
                ))
                .with_data("before", serde_json::json!(old_version))
                .with_data("after", serde_json::json!(new_version))
                .with_data("remote_url_changed", serde_json::json!(false)))
            }
        } else {
            // Just check current version
            let current_version =
                Self::get_current_version(connection, &dest, context)?
                    .unwrap_or_else(|| "unknown".to_string());

            Ok(ModuleOutput::ok(format!(
                "Repository exists at version '{}'",
                &current_version[..8.min(current_version.len())]
            ))
            .with_data("before", serde_json::json!(current_version.clone()))
            .with_data("after", serde_json::json!(current_version)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::local::LocalConnection;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;

    /// Helper to create a ModuleContext with a LocalConnection
    fn context_with_local_connection() -> ModuleContext {
        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        ModuleContext::default().with_connection(conn)
    }

    #[test]
    fn test_git_module_validate_params() {
        let module = GitModule;

        // Missing repo
        let mut params: ModuleParams = HashMap::new();
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        assert!(module.validate_params(&params).is_err());

        // Missing dest
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        assert!(module.validate_params(&params).is_err());

        // Valid params
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_git_module_validate_depth() {
        let module = GitModule;

        // Invalid depth (0)
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        params.insert("depth".to_string(), serde_json::json!(0));
        assert!(module.validate_params(&params).is_err());

        // Valid depth
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        params.insert("depth".to_string(), serde_json::json!(1));
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_git_module_validate_umask() {
        let module = GitModule;

        // Invalid umask (contains 8)
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        params.insert("umask".to_string(), serde_json::json!("0088"));
        assert!(module.validate_params(&params).is_err());

        // Invalid umask (contains letters)
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        params.insert("umask".to_string(), serde_json::json!("abc"));
        assert!(module.validate_params(&params).is_err());

        // Valid umask
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        params.insert("umask".to_string(), serde_json::json!("0022"));
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_git_module_check_mode() {
        let module = GitModule;
        let temp = TempDir::new().unwrap();
        let dest_path = temp.path().join("test-repo");

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest_path.to_str().unwrap()),
        );

        let context = context_with_local_connection().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would clone"));
        assert!(!dest_path.exists()); // Should not be created in check mode
    }

    #[test]
    fn test_git_module_clone_disabled() {
        let module = GitModule;
        let temp = TempDir::new().unwrap();
        let dest_path = temp.path().join("test-repo");

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest_path.to_str().unwrap()),
        );
        params.insert("clone".to_string(), serde_json::json!(false));

        let context = context_with_local_connection();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("clone=false"));
        assert!(!dest_path.exists());
    }

    #[test]
    fn test_git_module_name_and_description() {
        let module = GitModule;
        assert_eq!(module.name(), "git");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_git_module_required_params() {
        let module = GitModule;
        let required = module.required_params();
        assert_eq!(required.len(), 2);
        assert!(required.contains(&"repo"));
        assert!(required.contains(&"dest"));
    }

    #[test]
    fn test_ssh_config_build_command() {
        // Empty config
        let config = SshConfig::default();
        assert!(config.build_ssh_command().is_none());

        // With key file
        let config = SshConfig {
            key_file: Some("/path/to/key".to_string()),
            ssh_opts: None,
            accept_hostkey: false,
        };
        let cmd = config.build_ssh_command().unwrap();
        // /path/to/key is safe, so it is NOT quoted by shell_escape
        assert!(cmd.contains("-i /path/to/key"));
        // IdentitiesOnly=yes contains =, so it IS quoted
        assert!(cmd.contains("-o 'IdentitiesOnly=yes'"));

        // With key file containing spaces (security check)
        let config = SshConfig {
            key_file: Some("/path/to/my key".to_string()),
            ssh_opts: None,
            accept_hostkey: false,
        };
        let cmd = config.build_ssh_command().unwrap();
        // Contains space, so it IS quoted
        assert!(cmd.contains("-i '/path/to/my key'"));

        // With key file containing injection attempt (security check)
        let config = SshConfig {
            key_file: Some("id_rsa; rm -rf /".to_string()),
            ssh_opts: None,
            accept_hostkey: false,
        };
        let cmd = config.build_ssh_command().unwrap();
        assert!(cmd.contains("-i 'id_rsa; rm -rf /'"));

        // With accept_hostkey
        let config = SshConfig {
            key_file: None,
            ssh_opts: None,
            accept_hostkey: true,
        };
        let cmd = config.build_ssh_command().unwrap();
        assert!(cmd.contains("-o 'StrictHostKeyChecking=no'"));
        assert!(cmd.contains("-o 'UserKnownHostsFile=/dev/null'"));

        // With custom ssh_opts
        let config = SshConfig {
            key_file: None,
            ssh_opts: Some("-o ProxyCommand=ssh -W %h:%p proxy".to_string()),
            accept_hostkey: false,
        };
        let cmd = config.build_ssh_command().unwrap();
        // It should be quoted now because it contains spaces
        assert!(cmd.contains("'-o ProxyCommand=ssh -W %h:%p proxy'"));

        // Combined options
        let config = SshConfig {
            key_file: Some("/path/to/key".to_string()),
            ssh_opts: Some("-v".to_string()),
            accept_hostkey: true,
        };
        let cmd = config.build_ssh_command().unwrap();
        assert!(cmd.contains("-i /path/to/key"));
        assert!(cmd.contains("-o 'StrictHostKeyChecking=no'"));
        assert!(cmd.contains("-v"));
    }

    #[test]
    fn test_ssh_command_injection_mitigation() {
        // Attempt injection via key_file
        let config = SshConfig {
            key_file: Some("id_rsa; touch /tmp/pwned".to_string()),
            ssh_opts: None,
            accept_hostkey: false,
        };
        let cmd = config.build_ssh_command().unwrap();

        // Should be escaped: -i 'id_rsa; touch /tmp/pwned'
        assert!(cmd.contains("-i 'id_rsa; touch /tmp/pwned'"));

        // Attempt injection via ssh_opts
        let config = SshConfig {
            key_file: None,
            ssh_opts: Some("-o ProxyCommand=nc 127.0.0.1 22; echo injection".to_string()),
            accept_hostkey: false,
        };
        let cmd = config.build_ssh_command().unwrap();

        // Should be escaped
        assert!(cmd.contains("'-o ProxyCommand=nc 127.0.0.1 22; echo injection'"));
    }

    #[test]
    fn test_is_bare_repo() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().to_str().unwrap();
        let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
        let context = ModuleContext::default();

        // Not a bare repo
        assert!(!GitModule::is_bare_repo(&conn, dest, &context).unwrap());

        // Create fake bare repo structure
        std::fs::write(temp.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir(temp.path().join("objects")).unwrap();

        assert!(GitModule::is_bare_repo(&conn, dest, &context).unwrap());
    }

    #[test]
    fn test_git_module_argument_injection_protection() {
        let module = GitModule;

        // Test refspec starting with -
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        params.insert(
            "refspec".to_string(),
            serde_json::json!("--upload-pack=malicious"),
        );

        // This should fail validation
        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot start with '-'"));

        // Test version starting with -
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        params.insert("version".to_string(), serde_json::json!("-f"));
        params.insert("update".to_string(), serde_json::json!(true));

        // This should fail validation
        let result = module.validate_params(&params);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot start with '-'"));
    }

    #[test]
    fn test_git_module_vulnerability_check() {
        let module = GitModule;

        // Test repo starting with -
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("--upload-pack=touch /tmp/pwned"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));

        // This should FAIL validation
        let result = module.validate_params(&params);
        assert!(result.is_err(), "Repo starting with - should be rejected");
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot start with '-'"));

        // Test remote starting with -
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert("dest".to_string(), serde_json::json!("/tmp/test"));
        params.insert(
            "remote".to_string(),
            serde_json::json!("--upload-pack=touch /tmp/pwned"),
        );

        // This should FAIL validation
        let result = module.validate_params(&params);
        assert!(result.is_err(), "Remote starting with - should be rejected");
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cannot start with '-'"));
    }
}
