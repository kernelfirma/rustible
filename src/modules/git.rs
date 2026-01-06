//! Git module - Git repository management
//!
//! This module manages git repositories including cloning, updating,
//! and checking out specific versions or branches.

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::utils::shell_escape;
use std::path::Path;
use std::process::Command;

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
        let mut parts = vec!["ssh".to_string()];

        if let Some(key) = &self.key_file {
            parts.push(format!("-i {}", shell_escape(key)));
            // Disable other key sources when using specific key
            parts.push("-o IdentitiesOnly=yes".to_string());
        }

        if self.accept_hostkey {
            parts.push("-o StrictHostKeyChecking=no".to_string());
            parts.push("-o UserKnownHostsFile=/dev/null".to_string());
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

    /// Apply SSH configuration to a Command
    fn apply_to_command(&self, command: &mut Command) {
        if let Some(ssh_cmd) = self.build_ssh_command() {
            command.env("GIT_SSH_COMMAND", ssh_cmd);
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
    /// Check if git is installed
    fn check_git_installed() -> ModuleResult<bool> {
        let output = Command::new("git")
            .arg("--version")
            .output()
            .map_err(|_| ModuleError::ExecutionFailed("git is not installed".to_string()))?;
        Ok(output.status.success())
    }

    /// Check if a directory is a git repository
    fn is_git_repo(dest: &str) -> bool {
        Path::new(&format!("{}/.git", dest)).exists() || Self::is_bare_repo(dest)
    }

    /// Check if a directory is a bare git repository
    fn is_bare_repo(dest: &str) -> bool {
        // A bare repo has HEAD file directly in the directory
        Path::new(&format!("{}/HEAD", dest)).exists()
            && Path::new(&format!("{}/objects", dest)).exists()
    }

    /// Get the current HEAD commit hash
    fn get_current_version(dest: &str) -> ModuleResult<Option<String>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to get current version: {}", e))
            })?;

        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else {
            Ok(None)
        }
    }

    /// Get the current branch name
    fn get_current_branch(dest: &str) -> ModuleResult<Option<String>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("rev-parse")
            .arg("--abbrev-ref")
            .arg("HEAD")
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to get current branch: {}", e))
            })?;

        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
    fn get_remote_url(dest: &str, remote: &str) -> ModuleResult<Option<String>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("config")
            .arg("--get")
            .arg(format!("remote.{}.url", remote))
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to get remote URL: {}", e))
            })?;

        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else {
            Ok(None)
        }
    }

    /// Get list of local changes (for diff output)
    fn get_local_changes(dest: &str) -> ModuleResult<Vec<String>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("status")
            .arg("--porcelain")
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to get local changes: {}", e))
            })?;

        if output.status.success() {
            let changes: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect();
            Ok(changes)
        } else {
            Ok(vec![])
        }
    }

    /// Get commit log between two versions
    fn get_commit_log(dest: &str, from: &str, to: &str) -> ModuleResult<Vec<String>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("log")
            .arg("--oneline")
            .arg(format!("{}..{}", from, to))
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to get commit log: {}", e))
            })?;

        if output.status.success() {
            let commits: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.to_string())
                .collect();
            Ok(commits)
        } else {
            Ok(vec![])
        }
    }

    /// Verify GPG signature of a commit
    fn verify_commit(dest: &str, commit: &str, gpg_whitelist: &[String]) -> ModuleResult<bool> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("verify-commit")
            .arg("--raw")
            .arg(commit)
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to verify commit signature: {}", e))
            })?;

        if !output.status.success() {
            return Ok(false);
        }

        // If whitelist is empty, any valid signature is accepted
        if gpg_whitelist.is_empty() {
            return Ok(true);
        }

        // Check if the signature key is in the whitelist
        let stderr = String::from_utf8_lossy(&output.stderr);
        for fingerprint in gpg_whitelist {
            if stderr.contains(fingerprint) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Discard local modifications
    fn reset_hard(dest: &str) -> ModuleResult<()> {
        let output = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("reset")
            .arg("--hard")
            .arg("HEAD")
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to reset repository: {}", e))
            })?;

        if !output.status.success() {
            return Err(ModuleError::CommandFailed {
                code: output.status.code().unwrap_or(-1),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        // Also clean untracked files
        let _ = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("clean")
            .arg("-fd")
            .output();

        Ok(())
    }

    /// Set file permissions (umask) for a command
    fn apply_umask(command: &mut Command, umask: Option<&str>) {
        if let Some(mask) = umask {
            // On Unix, set umask via environment
            #[cfg(unix)]
            {
                // We can't directly set umask for a subprocess, but we can use
                // a wrapper script approach or document the limitation
                command.env("GIT_UMASK", mask);
            }
            #[cfg(not(unix))]
            {
                let _ = mask; // Silence unused variable warning
            }
        }
    }

    /// Clone a git repository
    fn clone_repo(
        repo: &str,
        dest: &str,
        version: Option<&str>,
        clone_config: &CloneConfig,
        ssh_config: &SshConfig,
        remote: &str,
        umask: Option<&str>,
        track_submodules: bool,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let mut command = Command::new("git");
        command.arg("clone");

        // Apply SSH configuration
        ssh_config.apply_to_command(&mut command);

        // Apply umask
        Self::apply_umask(&mut command, umask);

        // Bare repository
        if clone_config.bare {
            command.arg("--bare");
        }

        // Shallow clone
        if let Some(d) = clone_config.depth {
            command.arg("--depth").arg(d.to_string());
        }

        // Single branch (implied with depth, but can be explicit)
        if clone_config.single_branch {
            command.arg("--single-branch");
        }

        // Separate git directory
        if let Some(ref git_dir) = clone_config.separate_git_dir {
            command.arg("--separate-git-dir").arg(git_dir);
        }

        // Branch/tag to clone
        if let Some(v) = version {
            command.arg("--branch").arg(v);
        }

        // Custom remote name
        if remote != "origin" {
            command.arg("--origin").arg(remote);
        }

        // Recursive submodules
        if clone_config.recursive {
            command.arg("--recurse-submodules");
            if track_submodules {
                command.arg("--remote-submodules");
            }
        }

        command.arg(repo).arg(dest);

        let output = command.output().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to clone repository: {}", e))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ModuleError::CommandFailed {
                code: output.status.code().unwrap_or(-1),
                message: stderr.to_string(),
            });
        }

        // If refspec is specified, fetch it separately
        if let Some(ref refspec) = clone_config.refspec {
            let mut fetch_cmd = Command::new("git");
            fetch_cmd
                .arg("-C")
                .arg(dest)
                .arg("fetch")
                .arg(remote)
                .arg(refspec);
            ssh_config.apply_to_command(&mut fetch_cmd);

            let fetch_output = fetch_cmd.output().map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to fetch refspec: {}", e))
            })?;

            if !fetch_output.status.success() {
                let stderr = String::from_utf8_lossy(&fetch_output.stderr);
                return Err(ModuleError::CommandFailed {
                    code: fetch_output.status.code().unwrap_or(-1),
                    message: format!("Failed to fetch refspec '{}': {}", refspec, stderr),
                });
            }
        }

        let new_version = Self::get_current_version(dest)?.unwrap_or_else(|| "unknown".to_string());

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
    fn update_repo(
        dest: &str,
        version: Option<&str>,
        remote: &str,
        ssh_config: &SshConfig,
        force: bool,
        track_submodules: bool,
        refspec: Option<&str>,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String, String, Vec<String>)> {
        // Get current version before update
        let before_version =
            Self::get_current_version(dest)?.unwrap_or_else(|| "unknown".to_string());

        if context.check_mode {
            return Ok((false, before_version.clone(), before_version, vec![]));
        }

        // If force, reset local changes first
        if force {
            Self::reset_hard(dest)?;
        }

        // Fetch updates
        let mut fetch_cmd = Command::new("git");
        fetch_cmd.arg("-C").arg(dest).arg("fetch").arg(remote);

        if let Some(rs) = refspec {
            fetch_cmd.arg(rs);
        }

        ssh_config.apply_to_command(&mut fetch_cmd);

        let fetch_output = fetch_cmd
            .output()
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to fetch updates: {}", e)))?;

        if !fetch_output.status.success() {
            return Err(ModuleError::CommandFailed {
                code: fetch_output.status.code().unwrap_or(-1),
                message: String::from_utf8_lossy(&fetch_output.stderr).to_string(),
            });
        }

        // Checkout the specified version or default branch
        let checkout_target = if let Some(v) = version {
            v.to_string()
        } else {
            format!("{}/HEAD", remote)
        };

        let checkout_output = Command::new("git")
            .arg("-C")
            .arg(dest)
            .arg("checkout")
            .arg(&checkout_target)
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to checkout version: {}", e))
            })?;

        if !checkout_output.status.success() {
            return Err(ModuleError::CommandFailed {
                code: checkout_output.status.code().unwrap_or(-1),
                message: String::from_utf8_lossy(&checkout_output.stderr).to_string(),
            });
        }

        // If on a branch, pull the latest changes
        let current_branch = Self::get_current_branch(dest)?;
        if current_branch.is_some() {
            let mut pull_cmd = Command::new("git");
            pull_cmd.arg("-C").arg(dest).arg("pull");

            if force {
                pull_cmd.arg("--force");
            } else {
                pull_cmd.arg("--ff-only");
            }

            ssh_config.apply_to_command(&mut pull_cmd);

            let _ = pull_cmd.output();
        }

        // Update submodules if needed
        if track_submodules {
            let mut submodule_cmd = Command::new("git");
            submodule_cmd
                .arg("-C")
                .arg(dest)
                .arg("submodule")
                .arg("update")
                .arg("--init")
                .arg("--recursive")
                .arg("--remote");

            ssh_config.apply_to_command(&mut submodule_cmd);
            let _ = submodule_cmd.output();
        }

        // Get version after update
        let after_version =
            Self::get_current_version(dest)?.unwrap_or_else(|| "unknown".to_string());

        // Get commit log if versions differ
        let commits = if before_version != after_version {
            Self::get_commit_log(dest, &before_version, &after_version).unwrap_or_default()
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

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
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
        if !Self::check_git_installed()? {
            return Err(ModuleError::ExecutionFailed(
                "git is not installed on the system".to_string(),
            ));
        }

        // Check if destination exists and is a git repo
        let is_repo = Self::is_git_repo(&dest);

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
        let current_remote = Self::get_remote_url(&dest, &remote)?;
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
            if verify_commit && changed {
                if !Self::verify_commit(&dest, &new_version, &gpg_whitelist)? {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "GPG signature verification failed for commit {}",
                        new_version
                    )));
                }
            }

            if changed {
                // Build detailed diff output
                let diff_before = format!(
                    "commit: {}\nbranch: {}",
                    &old_version[..8.min(old_version.len())],
                    Self::get_current_branch(&dest)?.unwrap_or_else(|| "detached".to_string())
                );

                let mut diff_after = format!(
                    "commit: {}\nbranch: {}",
                    &new_version[..8.min(new_version.len())],
                    Self::get_current_branch(&dest)?.unwrap_or_else(|| "detached".to_string())
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
                Self::get_current_version(&dest)?.unwrap_or_else(|| "unknown".to_string());

            Ok(ModuleOutput::ok(format!(
                "Repository exists at version '{}'",
                &current_version[..8.min(current_version.len())]
            ))
            .with_data("before", serde_json::json!(current_version.clone()))
            .with_data("after", serde_json::json!(current_version)))
        }
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        self.execute(params, &check_context)
    }

    fn diff(&self, params: &ModuleParams, _context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let dest = params.get_string_required("dest")?;
        let repo = params.get_string_required("repo")?;
        let version = params.get_string("version")?;
        let remote = params
            .get_string("remote")?
            .unwrap_or_else(|| "origin".to_string());

        let is_repo = Self::is_git_repo(&dest);

        if !is_repo {
            Ok(Some(Diff::new(
                "repository: absent",
                format!(
                    "repository: {} @ {}",
                    repo,
                    version.as_deref().unwrap_or("HEAD")
                ),
            )))
        } else {
            let current_version =
                Self::get_current_version(&dest)?.unwrap_or_else(|| "unknown".to_string());
            let current_branch = Self::get_current_branch(&dest)?;
            let local_changes = Self::get_local_changes(&dest)?;
            let target_version = version.unwrap_or_else(|| "HEAD".to_string());

            // Build detailed before state
            let mut before = format!(
                "commit: {}\nbranch: {}",
                &current_version[..8.min(current_version.len())],
                current_branch.as_deref().unwrap_or("detached")
            );

            if !local_changes.is_empty() {
                before.push_str(&format!("\nlocal changes: {} files", local_changes.len()));
                for change in local_changes.iter().take(5) {
                    before.push_str(&format!("\n  {}", change));
                }
                if local_changes.len() > 5 {
                    before.push_str(&format!("\n  ... and {} more", local_changes.len() - 5));
                }
            }

            // Build target state
            let after = format!("commit: {}\nremote: {}", target_version, remote);

            Ok(Some(Diff::new(before, after)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

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

        let context = ModuleContext::default().with_check_mode(true);
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

        let context = ModuleContext::default();
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
        assert!(cmd.contains("-i /path/to/key"));
        assert!(cmd.contains("-o IdentitiesOnly=yes"));

        // With accept_hostkey
        let config = SshConfig {
            key_file: None,
            ssh_opts: None,
            accept_hostkey: true,
        };
        let cmd = config.build_ssh_command().unwrap();
        assert!(cmd.contains("-o StrictHostKeyChecking=no"));
        assert!(cmd.contains("-o UserKnownHostsFile=/dev/null"));

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
        assert!(cmd.contains("-o StrictHostKeyChecking=no"));
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

        // Not a bare repo
        assert!(!GitModule::is_bare_repo(dest));

        // Create fake bare repo structure
        std::fs::write(temp.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::create_dir(temp.path().join("objects")).unwrap();

        assert!(GitModule::is_bare_repo(dest));
    }

    #[test]
    fn test_diff_output_missing_repo() {
        let module = GitModule;
        let temp = TempDir::new().unwrap();
        let dest_path = temp.path().join("nonexistent");

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "repo".to_string(),
            serde_json::json!("https://github.com/test/repo"),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest_path.to_str().unwrap()),
        );
        params.insert("version".to_string(), serde_json::json!("v1.0.0"));

        let context = ModuleContext::default();
        let diff = module.diff(&params, &context).unwrap();

        assert!(diff.is_some());
        let diff = diff.unwrap();
        assert!(diff.before.contains("absent"));
        assert!(diff.after.contains("v1.0.0"));
    }
}
