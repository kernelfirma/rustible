//! Copy module - Copy files to destination
//!
//! This module copies files from a source to a destination, with support for
//! permissions, ownership, backup creation, and validation. It supports both local
//! operations and remote file transfers over SSH connections.
//!
//! Features:
//! - Local and remote file copying via SSH/SFTP
//! - Content validation before finalizing copy
//! - Automatic backup creation
//! - Directory mode control for created parent directories
//! - Symlink following on source files

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, TransferOptions};
use crate::utils::{compute_checksum, get_file_checksum, shell_escape};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use uuid::Uuid;

/// Module for copying files
pub struct CopyModule;

impl CopyModule {
    fn create_backup(dest: &Path, backup_suffix: &str) -> ModuleResult<Option<String>> {
        if dest.exists() {
            let backup_path = format!("{}{}", dest.display(), backup_suffix);
            fs::copy(dest, &backup_path)?;
            Ok(Some(backup_path))
        } else {
            Ok(None)
        }
    }

    fn set_permissions(path: &Path, mode: Option<u32>) -> ModuleResult<bool> {
        if let Some(mode) = mode {
            let current = fs::metadata(path)?.permissions().mode() & 0o7777;
            if current != mode {
                fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn files_differ(src: &Path, dest: &Path) -> std::io::Result<bool> {
        if !dest.exists() {
            return Ok(true);
        }

        let src_meta = fs::metadata(src)?;
        let dest_meta = fs::metadata(dest)?;

        // Quick check: different sizes means different content
        if src_meta.len() != dest_meta.len() {
            return Ok(true);
        }

        // Compare content directly using buffered readers to avoid
        // reading entire files into memory and computing checksums.
        // This fails fast on first difference.
        let mut f1 = std::io::BufReader::new(fs::File::open(src)?);
        let mut f2 = std::io::BufReader::new(fs::File::open(dest)?);

        let mut buf1 = [0; 8192];
        let mut buf2 = [0; 8192];

        loop {
            let n1 = f1.read(&mut buf1)?;
            if n1 == 0 {
                // EOF reached on src. Since sizes are equal, dest should also be at EOF.
                return Ok(false);
            }

            // Read corresponding chunk from dest
            let mut n2_total = 0;
            while n2_total < n1 {
                let n2 = f2.read(&mut buf2[n2_total..n1])?;
                if n2 == 0 {
                    // Unexpected EOF on dest (sizes were same, but file changed?)
                    return Ok(true);
                }
                n2_total += n2;
            }

            if buf1[..n1] != buf2[..n1] {
                return Ok(true);
            }
        }
    }

    fn copy_content(content: &str, dest: &Path, mode: Option<u32>) -> ModuleResult<()> {
        // Use secure_write_file for atomic creation and permission setting
        crate::utils::secure_write_file(dest, content, true, mode).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to write file '{}': {}", dest.display(), e))
        })?;
        Ok(())
    }

    fn copy_file(src: &Path, dest: &Path, force: bool, mode: Option<u32>) -> ModuleResult<()> {
        if dest.exists() {
            if !force {
                let dest_meta = fs::metadata(dest)?;
                if dest_meta.permissions().readonly() {
                    return Err(ModuleError::PermissionDenied(format!(
                        "Destination '{}' is read-only and force is not set",
                        dest.display()
                    )));
                }
            }
        }

        // Open source first so destination is never truncated on source-side errors.
        let mut src_file = fs::File::open(src)?;

        // Open destination file with specific options for security
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);

        #[cfg(unix)]
        if let Some(m) = mode {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(m);
        }

        let mut dest_file = options.open(dest)?;

        std::io::copy(&mut src_file, &mut dest_file)?;

        // Ensure permissions are set correctly
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let target_mode = if let Some(m) = mode {
                m
            } else {
                // If mode is not specified, preserve source mode
                let src_meta = src_file.metadata()?;
                src_meta.permissions().mode() & 0o7777
            };

            let metadata = dest_file.metadata()?;
            if (metadata.permissions().mode() & 0o7777) != target_mode {
                let mut perms = metadata.permissions();
                perms.set_mode(target_mode);
                dest_file.set_permissions(perms)?;
            }
        }

        Ok(())
    }

    /// Validate file content using a validation command
    /// The command should use %s as a placeholder for the file path
    fn validate_file(path: &Path, validate_cmd: &str) -> ModuleResult<()> {
        // Replace %s with the actual file path
        // Use shell_escape to prevent command injection
        let cmd = validate_cmd.replace("%s", &shell_escape(&path.to_string_lossy()));

        // Execute via shell to handle complex commands
        let output = Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .output()
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to run validation command: {}", e))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ModuleError::ValidationFailed(format!(
                "Validation command failed: {}",
                stderr.trim()
            )));
        }

        Ok(())
    }

    /// Create parent directories with specified mode
    fn create_parent_dirs(path: &Path, directory_mode: Option<u32>) -> ModuleResult<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;

                // Apply directory mode if specified
                if let Some(mode) = directory_mode {
                    // Walk up and apply mode to each created directory
                    let mut current = parent.to_path_buf();
                    while !current.as_os_str().is_empty() {
                        if current.exists() {
                            fs::set_permissions(&current, fs::Permissions::from_mode(mode))?;
                        }
                        if !current.pop() {
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Resolve source path, following symlinks if local_follow is true
    fn resolve_source(src: &Path, local_follow: bool) -> ModuleResult<std::path::PathBuf> {
        if local_follow && src.is_symlink() {
            // Follow the symlink to get the real path
            fs::canonicalize(src).map_err(|e| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to follow symlink '{}': {}",
                    src.display(),
                    e
                ))
            })
        } else {
            Ok(src.to_path_buf())
        }
    }

    /// Async implementation for remote copy using connection
    #[allow(clippy::too_many_arguments)]
    async fn execute_remote_async(
        connection: Arc<dyn Connection + Send + Sync>,
        dest: &str,
        src: Option<&str>,
        content: Option<&str>,
        mode: Option<u32>,
        owner: Option<&str>,
        group: Option<&str>,
        backup: bool,
        backup_suffix: &str,
        check_mode: bool,
        diff_mode: bool,
    ) -> ModuleResult<ModuleOutput> {
        let dest_path = Path::new(dest);

        // Determine the final destination path (handle directory destinations)
        let final_dest = if connection.is_directory(dest_path).await.unwrap_or(false) {
            if let Some(src_str) = src {
                let src_path = Path::new(src_str);
                dest_path.join(src_path.file_name().ok_or_else(|| {
                    ModuleError::InvalidParameter(
                        "Cannot determine filename from source".to_string(),
                    )
                })?)
            } else {
                return Err(ModuleError::InvalidParameter(
                    "Cannot copy content to a directory without specifying filename".to_string(),
                ));
            }
        } else {
            dest_path.to_path_buf()
        };

        // Check if file already exists and get checksum
        let (needs_copy, current_checksum) =
            if connection.path_exists(&final_dest).await.unwrap_or(false) {
                // Download current content to check if it differs
                if let Some(content_str) = content {
                    // Compare content
                    match connection.download_content(&final_dest).await {
                        Ok(existing) => {
                            let existing_str = String::from_utf8_lossy(&existing);
                            (
                                existing_str.as_ref() != content_str,
                                Some(compute_checksum(&existing)),
                            )
                        }
                        Err(_) => (true, None),
                    }
                } else if let Some(src_str) = src {
                    // Compare file checksums
                    let src_path = Path::new(src_str);
                    if !src_path.exists() {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Source file '{}' does not exist",
                            src_str
                        )));
                    }

                    // Use streaming checksum for source file to avoid loading into memory
                    let src_checksum = get_file_checksum(src_path).map_err(ModuleError::Io)?;

                    match connection.download_content(&final_dest).await {
                        Ok(existing) => {
                            let dest_checksum = compute_checksum(&existing);
                            (src_checksum != dest_checksum, Some(dest_checksum))
                        }
                        Err(_) => (true, None),
                    }
                } else {
                    (false, None)
                }
            } else {
                (true, None)
            };

        // Check if only permissions need updating
        if !needs_copy {
            let perm_changed = if let Some(m) = mode {
                match connection.stat(&final_dest).await {
                    Ok(stat) => (stat.mode & 0o7777) != m,
                    Err(_) => false,
                }
            } else {
                false
            };

            if perm_changed {
                if check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would change permissions on '{}'",
                        final_dest.display()
                    )));
                }

                // Update permissions via chmod command
                // Use shell_escape to prevent command injection via malicious file paths
                let path_str = final_dest.to_string_lossy();
                let escaped_path = shell_escape(&path_str);
                let chmod_cmd = format!("chmod {:o} {}", mode.unwrap(), escaped_path);
                connection.execute(&chmod_cmd, None).await.map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to set permissions: {}", e))
                })?;

                return Ok(ModuleOutput::changed(format!(
                    "Changed permissions on '{}'",
                    final_dest.display()
                )));
            }

            return Ok(ModuleOutput::ok(format!(
                "File '{}' is already up to date",
                final_dest.display()
            )));
        }

        // In check mode, report what would happen
        if check_mode {
            let src_display = if content.is_some() {
                "(content)"
            } else {
                src.unwrap_or_default()
            };

            let diff = if diff_mode {
                if let Some(content_str) = content {
                    let before = if let Some(cksum) = current_checksum {
                        format!("(existing file with checksum {})", cksum)
                    } else {
                        String::new()
                    };
                    Some(Diff::new(before, content_str))
                } else {
                    Some(Diff::new(
                        format!("(current state of {})", final_dest.display()),
                        format!("(contents of {})", src_display),
                    ))
                }
            } else {
                None
            };

            let mut output = ModuleOutput::changed(format!(
                "Would copy {} to '{}'",
                src_display,
                final_dest.display()
            ));

            if let Some(d) = diff {
                output = output.with_diff(d);
            }

            return Ok(output);
        }

        // Create backup if requested
        if backup && connection.path_exists(&final_dest).await.unwrap_or(false) {
            let backup_path = format!("{}{}", final_dest.display(), backup_suffix);
            let backup_dest = Path::new(&backup_path);

            // Download and re-upload as backup
            match connection.download_content(&final_dest).await {
                Ok(backup_content) => {
                    connection
                        .upload_content(&backup_content, backup_dest, None)
                        .await
                        .map_err(|e| {
                            ModuleError::ExecutionFailed(format!("Failed to create backup: {}", e))
                        })?;
                }
                Err(e) => {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Failed to read file for backup: {}",
                        e
                    )));
                }
            }
        }

        // Build transfer options
        let mut transfer_opts = TransferOptions::new();
        if let Some(m) = mode {
            transfer_opts = transfer_opts.with_mode(m);
        }
        if let Some(o) = owner {
            transfer_opts = transfer_opts.with_owner(o);
        }
        if let Some(g) = group {
            transfer_opts = transfer_opts.with_group(g);
        }
        transfer_opts = transfer_opts.with_create_dirs();

        // Perform the copy
        let src_display = if let Some(content_str) = content {
            // Upload content directly
            connection
                .upload_content(content_str.as_bytes(), &final_dest, Some(transfer_opts))
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to upload content: {}", e))
                })?;
            "(content)".to_string()
        } else if let Some(src_str) = src {
            // Upload file
            let src_path = Path::new(src_str);
            connection
                .upload(src_path, &final_dest, Some(transfer_opts))
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to upload file: {}", e))
                })?;
            src_str.to_string()
        } else {
            return Err(ModuleError::MissingParameter(
                "Either 'src' or 'content' must be provided".to_string(),
            ));
        };

        // Get file info from remote
        let mut output = ModuleOutput::changed(format!(
            "Copied {} to '{}'",
            src_display,
            final_dest.display()
        ));

        // Add file metadata if available
        if let Ok(stat) = connection.stat(&final_dest).await {
            output = output
                .with_data("dest", serde_json::json!(final_dest.to_string_lossy()))
                .with_data("size", serde_json::json!(stat.size))
                .with_data(
                    "mode",
                    serde_json::json!(format!("{:o}", stat.mode & 0o7777)),
                )
                .with_data("uid", serde_json::json!(stat.uid))
                .with_data("gid", serde_json::json!(stat.gid));
        }

        Ok(output)
    }

    /// Execute remote copy using the connection from context
    /// Uses a dedicated runtime to avoid blocking single-threaded executors
    #[allow(clippy::too_many_arguments)]
    fn execute_remote(
        connection: Arc<dyn Connection + Send + Sync>,
        dest: &str,
        src: Option<&str>,
        content: Option<&str>,
        mode: Option<u32>,
        owner: Option<&str>,
        group: Option<&str>,
        backup: bool,
        backup_suffix: &str,
        check_mode: bool,
        diff_mode: bool,
    ) -> ModuleResult<ModuleOutput> {
        // Use a dedicated runtime on a separate thread to avoid blocking
        // single-threaded executors (e.g., tokio::test current_thread flavor).
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| {
                            ModuleError::ExecutionFailed(format!("Failed to create runtime: {}", e))
                        })?;

                    rt.block_on(Self::execute_remote_async(
                        connection,
                        dest,
                        src,
                        content,
                        mode,
                        owner,
                        group,
                        backup,
                        backup_suffix,
                        check_mode,
                        diff_mode,
                    ))
                })
                .join()
                .map_err(|_| ModuleError::ExecutionFailed("Thread panicked".to_string()))?
        })
    }

    /// Execute local copy (when connection is None or local)
    #[allow(clippy::too_many_arguments)]
    fn execute_local(
        dest: &str,
        src: Option<&str>,
        content: Option<&str>,
        mode: Option<u32>,
        directory_mode: Option<u32>,
        force: bool,
        backup: bool,
        backup_suffix: &str,
        validate: Option<&str>,
        local_follow: bool,
        check_mode: bool,
        diff_mode: bool,
    ) -> ModuleResult<ModuleOutput> {
        let dest_path = Path::new(dest);

        // Determine if we're copying from src or content
        let (source_content, src_display, resolved_src) = if let Some(content_str) = content {
            (Some(content_str.to_string()), "(content)".to_string(), None)
        } else if let Some(src_str) = src {
            let src_path = Path::new(src_str);
            if !src_path.exists() {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Source file '{}' does not exist",
                    src_str
                )));
            }
            // Resolve symlinks if local_follow is true
            let resolved = Self::resolve_source(src_path, local_follow)?;
            (None, src_str.to_string(), Some(resolved))
        } else {
            return Err(ModuleError::MissingParameter(
                "Either 'src' or 'content' must be provided".to_string(),
            ));
        };

        // Check if dest is a directory
        let final_dest = if dest_path.is_dir() {
            if let Some(src_str) = src {
                let src_path = Path::new(src_str);
                dest_path.join(src_path.file_name().ok_or_else(|| {
                    ModuleError::InvalidParameter(
                        "Cannot determine filename from source".to_string(),
                    )
                })?)
            } else {
                return Err(ModuleError::InvalidParameter(
                    "Cannot copy content to a directory without specifying filename".to_string(),
                ));
            }
        } else {
            dest_path.to_path_buf()
        };

        // Check if copy is needed - use resolved source if available
        let needs_copy = if let Some(ref resolved) = resolved_src {
            Self::files_differ(resolved, &final_dest)?
        } else {
            // For content, always check
            if final_dest.exists() {
                let mut existing = String::new();
                fs::File::open(&final_dest)?.read_to_string(&mut existing)?;
                // Use as_deref() for safe access - source_content is guaranteed to be Some here
                // because we're in the else branch of resolved_src check
                existing != source_content.as_deref().unwrap_or("")
            } else {
                true
            }
        };

        if !needs_copy {
            // Check if only permissions need updating
            let perm_changed = if let Some(m) = mode {
                if final_dest.exists() {
                    let current = fs::metadata(&final_dest)?.permissions().mode() & 0o7777;
                    current != m
                } else {
                    false
                }
            } else {
                false
            };

            if perm_changed {
                if check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would change permissions on '{}'",
                        final_dest.display()
                    )));
                }
                Self::set_permissions(&final_dest, mode)?;
                return Ok(ModuleOutput::changed(format!(
                    "Changed permissions on '{}'",
                    final_dest.display()
                )));
            }

            return Ok(ModuleOutput::ok(format!(
                "File '{}' is already up to date",
                final_dest.display()
            )));
        }

        // In check mode, return what would happen
        if check_mode {
            let diff = if diff_mode {
                if let Some(ref content_str) = source_content {
                    let before = if final_dest.exists() {
                        fs::read_to_string(&final_dest).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    Some(Diff::new(before, content_str.clone()))
                } else {
                    Some(Diff::new(
                        format!("(current state of {})", final_dest.display()),
                        format!("(contents of {})", src_display),
                    ))
                }
            } else {
                None
            };

            let mut output = ModuleOutput::changed(format!(
                "Would copy {} to '{}'",
                src_display,
                final_dest.display()
            ));

            if let Some(d) = diff {
                output = output.with_diff(d);
            }

            return Ok(output);
        }

        // Create backup if requested
        let backup_file = if backup {
            Self::create_backup(&final_dest, backup_suffix)?
        } else {
            None
        };

        // Create parent directories with specified mode if needed
        Self::create_parent_dirs(&final_dest, directory_mode)?;

        // For validation, we'll copy to a temp file first, validate, then move
        let use_validation = validate.is_some();
        let temp_dest = if use_validation {
            // Use cryptographically secure random UUID for temporary filename to prevent race conditions
            // and predictable filename attacks.
            let temp_name = format!("{}.rustible.tmp.{}", final_dest.display(), Uuid::new_v4());
            std::path::PathBuf::from(temp_name)
        } else {
            final_dest.clone()
        };

        // Perform the copy to temp or final destination
        if let Some(ref content_str) = source_content {
            Self::copy_content(content_str, &temp_dest, mode)?;
        } else if let Some(ref resolved) = resolved_src {
            Self::copy_file(resolved, &temp_dest, force, mode)?;
        }

        // Permissions are handled inside copy_content/copy_file via secure methods,
        // but we still need to report changes. Since we're writing new content,
        // we can assume we've set them correctly.
        // Self::set_permissions(&temp_dest, mode)?; // Redundant now

        // Run validation if specified
        if let Some(validate_cmd) = validate {
            match Self::validate_file(&temp_dest, validate_cmd) {
                Ok(()) => {
                    // Validation passed, move temp to final destination
                    if use_validation {
                        fs::rename(&temp_dest, &final_dest)?;
                    }
                }
                Err(e) => {
                    // Validation failed, clean up temp file
                    if use_validation {
                        let _ = fs::remove_file(&temp_dest);
                    }
                    return Err(e);
                }
            }
        }

        let mut output = ModuleOutput::changed(format!(
            "Copied {} to '{}'",
            src_display,
            final_dest.display()
        ));

        if let Some(backup_path) = backup_file {
            output = output.with_data("backup_file", serde_json::json!(backup_path));
        }

        // Add file info to output
        let meta = fs::metadata(&final_dest)?;
        output = output
            .with_data("dest", serde_json::json!(final_dest.to_string_lossy()))
            .with_data("size", serde_json::json!(meta.len()))
            .with_data(
                "mode",
                serde_json::json!(format!("{:o}", meta.permissions().mode() & 0o7777)),
            )
            .with_data("uid", serde_json::json!(meta.uid()))
            .with_data("gid", serde_json::json!(meta.gid()));

        if validate.is_some() {
            output = output.with_data("validated", serde_json::json!(true));
        }

        Ok(output)
    }
}

impl Module for CopyModule {
    fn name(&self) -> &'static str {
        "copy"
    }

    fn description(&self) -> &'static str {
        "Copy files to a destination"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::NativeTransport
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Must have either src or content
        if params.get("src").is_none() && params.get("content").is_none() {
            return Err(ModuleError::MissingParameter(
                "Either 'src' or 'content' must be provided".to_string(),
            ));
        }

        // Must have dest
        if params.get("dest").is_none() {
            return Err(ModuleError::MissingParameter("dest".to_string()));
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let dest = params.get_string_required("dest")?;
        let src = params.get_string("src")?;
        let inline_content = params.get_string("content")?;
        let force = params.get_bool_or("force", true);
        let backup = params.get_bool_or("backup", false);
        let backup_suffix = params
            .get_string("backup_suffix")?
            .unwrap_or_else(|| "~".to_string());
        let mode = params.get_u32("mode")?;
        let directory_mode = params.get_u32("directory_mode")?;
        let owner = params.get_string("owner")?;
        let group = params.get_string("group")?;
        let validate = params.get_string("validate")?;
        let local_follow = params.get_bool_or("local_follow", true);

        // Check if we have a remote connection
        if let Some(ref connection) = context.connection {
            // Remote execution via async connection
            Self::execute_remote(
                connection.clone(),
                &dest,
                src.as_deref(),
                inline_content.as_deref(),
                mode,
                owner.as_deref(),
                group.as_deref(),
                backup,
                &backup_suffix,
                context.check_mode,
                context.diff_mode,
            )
        } else {
            // Local execution (connection is None)
            Self::execute_local(
                &dest,
                src.as_deref(),
                inline_content.as_deref(),
                mode,
                directory_mode,
                force,
                backup,
                &backup_suffix,
                validate.as_deref(),
                local_follow,
                context.check_mode,
                context.diff_mode,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_validate_command_injection() {
        let temp = TempDir::new().unwrap();
        // Construct a filename that attempts to break out of the shell command
        // validation command: ls %s
        // filename: foo; touch pwned
        // result: ls foo; touch pwned

        // We need a directory for this to work cleanly as a destination
        let dest_dir = temp.path().join("foo; touch pwned");

        // However, we are copying TO a file.
        // Let's use a destination path that contains the injection payload.
        // The module will try to write to {dest}.rustible.tmp.{uuid}
        // Then run validation on it.

        // Payload: "safe; echo pwned > pwned_file #"
        // This is tricky because of the suffix .rustible.tmp...
        // But if we can just execute `touch pwned`, that's enough.

        let pwned_file = temp.path().join("pwned");
        let payload = format!("safe; touch '{}' #", pwned_file.display());
        let dest = temp.path().join(&payload);

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("content"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("validate".to_string(), serde_json::json!("ls %s"));

        let context = ModuleContext::default();
        // This might fail due to "safe;..." not being a valid filename if we were actually creating it
        // BUT CopyModule creates the temp file first.
        // File::create("safe; touch '...' #.rustible.tmp.123") works on Linux.

        // We expect execute to fail if the path is invalid, OR succeed if valid.
        // But the side effect (touch pwned) is what we care about.

        let _ = module.execute(&params, &context);

        // If injection worked, pwned_file should exist
        assert!(
            !pwned_file.exists(),
            "Command injection successful! pwned file created."
        );
    }

    #[test]
    fn test_copy_content() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("test.txt");

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("Hello, World!"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_copy_file() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("source.txt");
        let dest = temp.path().join("dest.txt");

        fs::write(&src, "Source content").unwrap();

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "Source content");
    }

    #[test]
    fn test_copy_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("source.txt");
        let dest = temp.path().join("dest.txt");

        fs::write(&src, "Same content").unwrap();
        fs::write(&dest, "Same content").unwrap();

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
    }

    #[test]
    fn test_copy_with_mode() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("test.txt");

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("Hello"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("mode".to_string(), serde_json::json!(0o755));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let meta = fs::metadata(&dest).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
    }

    #[test]
    fn test_copy_check_mode() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("test.txt");

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("Hello"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would copy"));
        assert!(!dest.exists()); // File should not be created in check mode
    }

    #[test]
    fn test_copy_with_backup() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("test.txt");

        // Create existing file
        fs::write(&dest, "Old content").unwrap();

        let module = CopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("New content"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("backup".to_string(), serde_json::json!(true));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.data.contains_key("backup_file"));

        let backup_path = temp.path().join("test.txt~");
        assert!(backup_path.exists());
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), "Old content");
    }

    #[test]
    fn test_files_differ_impl() {
        let temp = TempDir::new().unwrap();
        let file1 = temp.path().join("file1");
        let file2 = temp.path().join("file2");

        // Case 1: Identical files
        fs::write(&file1, "content").unwrap();
        fs::write(&file2, "content").unwrap();
        assert!(!CopyModule::files_differ(&file1, &file2).unwrap());

        // Case 2: Different sizes
        fs::write(&file2, "content_longer").unwrap();
        assert!(CopyModule::files_differ(&file1, &file2).unwrap());

        // Case 3: Same size, different content
        fs::write(&file1, "aaaa").unwrap();
        fs::write(&file2, "bbbb").unwrap();
        assert!(CopyModule::files_differ(&file1, &file2).unwrap());

        // Case 4: Large identical files (larger than buffer)
        let large_content = vec![b'x'; 20000];
        fs::write(&file1, &large_content).unwrap();
        fs::write(&file2, &large_content).unwrap();
        assert!(!CopyModule::files_differ(&file1, &file2).unwrap());

        // Case 5: Large different files (diff at end)
        let mut large_content2 = large_content.clone();
        large_content2[19999] = b'y';
        fs::write(&file2, &large_content2).unwrap();
        assert!(CopyModule::files_differ(&file1, &file2).unwrap());
    }
}
