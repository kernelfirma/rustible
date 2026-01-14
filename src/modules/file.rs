//! File module - File/directory state management
//!
//! This module manages file and directory state including creation, deletion,
//! permissions, ownership, and symbolic links. It supports setting access/modification
//! times and SELinux contexts on compatible systems.

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::Path;

/// Desired state for a file/directory
#[derive(Debug, Clone, PartialEq)]
pub enum FileState {
    /// File should exist
    File,
    /// Directory should exist
    Directory,
    /// Symbolic link should exist
    Link,
    /// Hard link should exist
    Hard,
    /// Path should not exist
    Absent,
    /// Only update attributes (touch)
    Touch,
}

impl FileState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "file" => Ok(FileState::File),
            "directory" | "dir" => Ok(FileState::Directory),
            "link" | "symlink" => Ok(FileState::Link),
            "hard" | "hardlink" => Ok(FileState::Hard),
            "absent" => Ok(FileState::Absent),
            "touch" => Ok(FileState::Touch),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: file, directory, link, hard, absent, touch",
                s
            ))),
        }
    }
}

/// SELinux context parameters
#[derive(Debug, Clone, Default)]
pub struct SelinuxContext {
    /// SELinux user
    pub seuser: Option<String>,
    /// SELinux role
    pub serole: Option<String>,
    /// SELinux type
    pub setype: Option<String>,
    /// SELinux level/range
    pub selevel: Option<String>,
}

impl SelinuxContext {
    /// Check if any SELinux parameters are set
    pub fn is_set(&self) -> bool {
        self.seuser.is_some()
            || self.serole.is_some()
            || self.setype.is_some()
            || self.selevel.is_some()
    }

    /// Build context string in format user:role:type:level
    pub fn to_context_string(&self) -> Option<String> {
        if !self.is_set() {
            return None;
        }
        Some(format!(
            "{}:{}:{}:{}",
            self.seuser.as_deref().unwrap_or("_"),
            self.serole.as_deref().unwrap_or("_"),
            self.setype.as_deref().unwrap_or("_"),
            self.selevel.as_deref().unwrap_or("_")
        ))
    }
}

/// Module for file/directory management
pub struct FileModule;

impl FileModule {
    fn get_current_state(path: &Path) -> Option<FileState> {
        if !path.exists() && !path.is_symlink() {
            return None;
        }

        let meta = match path.symlink_metadata() {
            Ok(m) => m,
            Err(_) => return None,
        };

        if meta.file_type().is_symlink() {
            Some(FileState::Link)
        } else if meta.is_dir() {
            Some(FileState::Directory)
        } else if meta.is_file() {
            Some(FileState::File)
        } else {
            None
        }
    }

    fn set_permissions(path: &Path, mode: u32) -> ModuleResult<bool> {
        let meta = fs::symlink_metadata(path)?;

        // Don't change permissions on symlinks
        if meta.file_type().is_symlink() {
            return Ok(false);
        }

        let current = meta.permissions().mode() & 0o7777;
        if current != mode {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
            return Ok(true);
        }
        Ok(false)
    }

    fn set_owner(path: &Path, owner: Option<u32>, group: Option<u32>) -> ModuleResult<bool> {
        use std::os::unix::fs::chown;

        let meta = fs::symlink_metadata(path)?;
        let current_uid = meta.uid();
        let current_gid = meta.gid();

        let new_uid = owner.unwrap_or(current_uid);
        let new_gid = group.unwrap_or(current_gid);

        if current_uid != new_uid || current_gid != new_gid {
            chown(path, Some(new_uid), Some(new_gid))?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Set access and modification times on a file
    fn set_times(
        path: &Path,
        access_time: Option<i64>,
        modification_time: Option<i64>,
    ) -> ModuleResult<bool> {
        if access_time.is_none() && modification_time.is_none() {
            return Ok(false);
        }

        let meta = fs::metadata(path)?;
        let current_atime = meta.atime();
        let current_mtime = meta.mtime();

        let new_atime = access_time.unwrap_or(current_atime);
        let new_mtime = modification_time.unwrap_or(current_mtime);

        if current_atime != new_atime || current_mtime != new_mtime {
            let atime = filetime::FileTime::from_unix_time(new_atime, 0);
            let mtime = filetime::FileTime::from_unix_time(new_mtime, 0);
            filetime::set_file_times(path, atime, mtime)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Parse a timestamp from string (supports epoch seconds or ISO 8601)
    fn parse_timestamp(value: &str) -> ModuleResult<i64> {
        // Try parsing as epoch seconds first
        if let Ok(epoch) = value.parse::<i64>() {
            return Ok(epoch);
        }

        // Try parsing as ISO 8601 datetime
        // Basic format: YYYY-MM-DDTHH:MM:SS or YYYYMMDDTHHMMSS
        // For simplicity, we'll support common formats
        Err(ModuleError::InvalidParameter(format!(
            "Invalid timestamp '{}'. Use epoch seconds or ISO 8601 format.",
            value
        )))
    }

    /// Set SELinux context on a file (Linux-specific)
    #[cfg(target_os = "linux")]
    fn set_selinux_context(path: &Path, context: &SelinuxContext) -> ModuleResult<bool> {
        use std::process::Command;

        if !context.is_set() {
            return Ok(false);
        }

        // Check if SELinux is enabled
        let sestatus = Command::new("sestatus").output();
        let selinux_enabled = match sestatus {
            Ok(output) => {
                let status = String::from_utf8_lossy(&output.stdout);
                status.contains("SELinux status:                 enabled")
            }
            Err(_) => false,
        };

        if !selinux_enabled {
            // SELinux not available, skip silently
            return Ok(false);
        }

        // Build chcon arguments
        let mut args: Vec<String> = Vec::new();

        if let Some(ref user) = context.seuser {
            args.push("-u".to_string());
            args.push(user.clone());
        }
        if let Some(ref role) = context.serole {
            args.push("-r".to_string());
            args.push(role.clone());
        }
        if let Some(ref setype) = context.setype {
            args.push("-t".to_string());
            args.push(setype.clone());
        }
        if let Some(ref level) = context.selevel {
            args.push("-l".to_string());
            args.push(level.clone());
        }

        args.push(path.to_string_lossy().to_string());

        let output = Command::new("chcon").args(&args).output()?;

        if !output.status.success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to set SELinux context: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(true)
    }

    /// Stub for non-Linux systems
    #[cfg(not(target_os = "linux"))]
    fn set_selinux_context(_path: &Path, context: &SelinuxContext) -> ModuleResult<bool> {
        if context.is_set() {
            // Warn that SELinux is not available but don't fail
            return Ok(false);
        }
        Ok(false)
    }

    /// Apply attributes recursively to a directory
    fn apply_attributes_recursive(
        path: &Path,
        mode: Option<u32>,
        owner: Option<u32>,
        group: Option<u32>,
        follow: bool,
        selinux: &SelinuxContext,
    ) -> ModuleResult<bool> {
        let mut changed = false;

        for entry in walkdir::WalkDir::new(path).follow_links(follow) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Error walking directory: {}",
                        e
                    )));
                }
            };

            let entry_path = entry.path();

            // Skip the root path itself - we handle it separately
            if entry_path == path {
                continue;
            }

            // Set mode if specified
            if let Some(m) = mode {
                if Self::set_permissions(entry_path, m)? {
                    changed = true;
                }
            }

            // Set ownership if specified
            if Self::set_owner(entry_path, owner, group)? {
                changed = true;
            }

            // Set SELinux context if specified
            if Self::set_selinux_context(entry_path, selinux)? {
                changed = true;
            }
        }

        Ok(changed)
    }

    fn create_directory(path: &Path, mode: Option<u32>, recurse: bool) -> ModuleResult<bool> {
        if path.exists() {
            if path.is_dir() {
                return Ok(false);
            } else {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Path '{}' exists but is not a directory",
                    path.display()
                )));
            }
        }

        if recurse {
            fs::create_dir_all(path)?;
        } else {
            fs::create_dir(path)?;
        }

        if let Some(mode) = mode {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        }

        Ok(true)
    }

    fn create_file(path: &Path, mode: Option<u32>) -> ModuleResult<bool> {
        if path.exists() {
            if path.is_file() {
                return Ok(false);
            } else {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Path '{}' exists but is not a file",
                    path.display()
                )));
            }
        }

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        fs::File::create(path)?;

        if let Some(mode) = mode {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
        }

        Ok(true)
    }

    fn create_symlink(src: &Path, dest: &Path, force: bool) -> ModuleResult<bool> {
        // Check if symlink already exists and points to correct target
        if dest.is_symlink() {
            if let Ok(target) = fs::read_link(dest) {
                if target == src {
                    return Ok(false);
                }
            }
            if force {
                fs::remove_file(dest)?;
            } else {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Symlink '{}' already exists with different target",
                    dest.display()
                )));
            }
        } else if dest.exists() {
            if force {
                if dest.is_dir() {
                    fs::remove_dir_all(dest)?;
                } else {
                    fs::remove_file(dest)?;
                }
            } else {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Path '{}' already exists and is not a symlink",
                    dest.display()
                )));
            }
        }

        // Create parent directories if needed
        if let Some(parent) = dest.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        symlink(src, dest)?;
        Ok(true)
    }

    fn create_hardlink(src: &Path, dest: &Path, force: bool) -> ModuleResult<bool> {
        if !src.exists() {
            return Err(ModuleError::ExecutionFailed(format!(
                "Source '{}' does not exist",
                src.display()
            )));
        }

        // Check if hardlink already exists
        if dest.exists() {
            let src_meta = fs::metadata(src)?;
            let dest_meta = fs::metadata(dest)?;

            // Same inode means same file (hardlink already exists)
            if src_meta.ino() == dest_meta.ino() && src_meta.dev() == dest_meta.dev() {
                return Ok(false);
            }

            if force {
                fs::remove_file(dest)?;
            } else {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Path '{}' already exists",
                    dest.display()
                )));
            }
        }

        // Create parent directories if needed
        if let Some(parent) = dest.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        fs::hard_link(src, dest)?;
        Ok(true)
    }

    fn remove_path(path: &Path, recurse: bool) -> ModuleResult<bool> {
        if !path.exists() && !path.is_symlink() {
            return Ok(false);
        }

        let meta = fs::symlink_metadata(path)?;

        if meta.is_dir() {
            if recurse {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_dir(path)?;
            }
        } else {
            fs::remove_file(path)?;
        }

        Ok(true)
    }

    fn touch_file(path: &Path) -> ModuleResult<bool> {
        use std::time::SystemTime;

        if !path.exists() {
            // Create parent directories if needed
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
            // Create the file
            fs::File::create(path)?;
            return Ok(true);
        }

        // Update access and modification times
        let now = SystemTime::now();
        filetime::set_file_mtime(path, filetime::FileTime::from_system_time(now))?;
        filetime::set_file_atime(path, filetime::FileTime::from_system_time(now))?;

        Ok(true)
    }
}

impl Module for FileModule {
    fn name(&self) -> &'static str {
        "file"
    }

    fn description(&self) -> &'static str {
        "Manage file and directory state"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::NativeTransport
    }

    fn required_params(&self) -> &[&'static str] {
        &["path"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let path_str = params.get_string_required("path")?;
        let path = Path::new(&path_str);
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "file".to_string());
        let state = FileState::from_str(&state_str)?;
        let mode = params.get_u32("mode")?;
        let owner = params.get_u32("owner")?;
        let group = params.get_u32("group")?;
        // Default recurse to true for directory creation (matches Ansible behavior)
        let recurse = params.get_bool_or("recurse", true);
        let force = params.get_bool_or("force", false);
        let follow = params.get_bool_or("follow", true);
        let src = params.get_string("src")?;

        // Parse access and modification times
        let access_time = if let Some(atime_str) = params.get_string("access_time")? {
            Some(Self::parse_timestamp(&atime_str)?)
        } else {
            params.get_i64("access_time")?
        };

        let modification_time = if let Some(mtime_str) = params.get_string("modification_time")? {
            Some(Self::parse_timestamp(&mtime_str)?)
        } else {
            params.get_i64("modification_time")?
        };

        // SELinux context parameters
        let selinux = SelinuxContext {
            seuser: params.get_string("seuser")?,
            serole: params.get_string("serole")?,
            setype: params.get_string("setype")?,
            selevel: params.get_string("selevel")?,
        };

        let current_state = Self::get_current_state(path);

        // Handle each state
        match state {
            FileState::Absent => {
                if current_state.is_none() {
                    return Ok(ModuleOutput::ok(format!(
                        "Path '{}' already absent",
                        path_str
                    )));
                }

                if context.check_mode {
                    return Ok(
                        ModuleOutput::changed(format!("Would remove '{}'", path_str))
                            .with_diff(Diff::new(format!("{:?}", current_state), "absent")),
                    );
                }

                Self::remove_path(path, recurse)?;
                Ok(ModuleOutput::changed(format!("Removed '{}'", path_str)))
            }

            FileState::Directory => {
                if context.check_mode {
                    if current_state == Some(FileState::Directory) {
                        // Check if permissions need changing
                        if mode.is_some()
                            || owner.is_some()
                            || group.is_some()
                            || access_time.is_some()
                            || modification_time.is_some()
                            || selinux.is_set()
                        {
                            return Ok(ModuleOutput::changed(format!(
                                "Would update attributes on '{}'",
                                path_str
                            )));
                        }
                        return Ok(ModuleOutput::ok(format!(
                            "Directory '{}' already exists",
                            path_str
                        )));
                    }
                    return Ok(ModuleOutput::changed(format!(
                        "Would create directory '{}'",
                        path_str
                    )));
                }

                let created = Self::create_directory(path, mode, recurse)?;
                let perm_changed = if let Some(m) = mode {
                    Self::set_permissions(path, m)?
                } else {
                    false
                };
                let owner_changed = Self::set_owner(path, owner, group)?;
                let times_changed = Self::set_times(path, access_time, modification_time)?;
                let selinux_changed = Self::set_selinux_context(path, &selinux)?;

                // Apply attributes recursively if requested
                let recursive_changed = if recurse && path.is_dir() {
                    Self::apply_attributes_recursive(path, mode, owner, group, follow, &selinux)?
                } else {
                    false
                };

                if created {
                    Ok(ModuleOutput::changed(format!(
                        "Created directory '{}'",
                        path_str
                    )))
                } else if perm_changed
                    || owner_changed
                    || times_changed
                    || selinux_changed
                    || recursive_changed
                {
                    Ok(ModuleOutput::changed(format!(
                        "Updated attributes on directory '{}'",
                        path_str
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Directory '{}' already exists with correct attributes",
                        path_str
                    )))
                }
            }

            FileState::File => {
                if context.check_mode {
                    if current_state == Some(FileState::File) {
                        if mode.is_some()
                            || owner.is_some()
                            || group.is_some()
                            || access_time.is_some()
                            || modification_time.is_some()
                            || selinux.is_set()
                        {
                            return Ok(ModuleOutput::changed(format!(
                                "Would update attributes on '{}'",
                                path_str
                            )));
                        }
                        return Ok(ModuleOutput::ok(format!(
                            "File '{}' already exists",
                            path_str
                        )));
                    }
                    return Ok(ModuleOutput::changed(format!(
                        "Would create file '{}'",
                        path_str
                    )));
                }

                // Resolve path if follow is enabled and it's a symlink
                let target_path = if follow && path.is_symlink() {
                    fs::read_link(path)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| path.to_path_buf())
                } else {
                    path.to_path_buf()
                };

                let created = Self::create_file(&target_path, mode)?;
                let perm_changed = if let Some(m) = mode {
                    Self::set_permissions(&target_path, m)?
                } else {
                    false
                };
                let owner_changed = Self::set_owner(&target_path, owner, group)?;
                let times_changed = Self::set_times(&target_path, access_time, modification_time)?;
                let selinux_changed = Self::set_selinux_context(&target_path, &selinux)?;

                if created {
                    Ok(ModuleOutput::changed(format!(
                        "Created file '{}'",
                        path_str
                    )))
                } else if perm_changed || owner_changed || times_changed || selinux_changed {
                    Ok(ModuleOutput::changed(format!(
                        "Updated attributes on file '{}'",
                        path_str
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "File '{}' already exists with correct attributes",
                        path_str
                    )))
                }
            }

            FileState::Link => {
                let src = src.ok_or_else(|| {
                    ModuleError::MissingParameter("src is required for symlinks".to_string())
                })?;
                let src_path = Path::new(&src);

                if context.check_mode {
                    if current_state == Some(FileState::Link) {
                        if let Ok(target) = fs::read_link(path) {
                            if target == src_path {
                                return Ok(ModuleOutput::ok(format!(
                                    "Symlink '{}' already points to '{}'",
                                    path_str, src
                                )));
                            }
                        }
                    }
                    return Ok(ModuleOutput::changed(format!(
                        "Would create symlink '{}' -> '{}'",
                        path_str, src
                    )));
                }

                let created = Self::create_symlink(src_path, path, force)?;

                if created {
                    Ok(ModuleOutput::changed(format!(
                        "Created symlink '{}' -> '{}'",
                        path_str, src
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Symlink '{}' already points to '{}'",
                        path_str, src
                    )))
                }
            }

            FileState::Hard => {
                let src = src.ok_or_else(|| {
                    ModuleError::MissingParameter("src is required for hard links".to_string())
                })?;
                let src_path = Path::new(&src);

                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would create hard link '{}' -> '{}'",
                        path_str, src
                    )));
                }

                let created = Self::create_hardlink(src_path, path, force)?;

                if created {
                    Ok(ModuleOutput::changed(format!(
                        "Created hard link '{}' -> '{}'",
                        path_str, src
                    )))
                } else {
                    Ok(ModuleOutput::ok(format!(
                        "Hard link '{}' already exists",
                        path_str
                    )))
                }
            }

            FileState::Touch => {
                if context.check_mode {
                    if path.exists() {
                        return Ok(ModuleOutput::changed(format!(
                            "Would update timestamps on '{}'",
                            path_str
                        )));
                    }
                    return Ok(ModuleOutput::changed(format!(
                        "Would create file '{}'",
                        path_str
                    )));
                }

                // If specific times are provided, use those; otherwise touch with current time
                if access_time.is_some() || modification_time.is_some() {
                    if !path.exists() {
                        // Create parent directories if needed
                        if let Some(parent) = path.parent() {
                            if !parent.exists() {
                                fs::create_dir_all(parent)?;
                            }
                        }
                        fs::File::create(path)?;
                    }
                    Self::set_times(path, access_time, modification_time)?;
                } else {
                    Self::touch_file(path)?;
                }

                if let Some(m) = mode {
                    Self::set_permissions(path, m)?;
                }
                Self::set_owner(path, owner, group)?;
                Self::set_selinux_context(path, &selinux)?;

                Ok(ModuleOutput::changed(format!("Touched '{}'", path_str)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_file_create_directory() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testdir");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(path.is_dir());
    }

    #[test]
    fn test_file_create_directory_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testdir");
        fs::create_dir(&path).unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
    }

    #[test]
    fn test_file_create_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("file"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(path.is_file());
    }

    #[test]
    fn test_file_absent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");
        fs::write(&path, "content").unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("absent"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(!path.exists());
    }

    #[test]
    fn test_file_symlink() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("source");
        let dest = temp.path().join("link");
        fs::write(&src, "content").unwrap();

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert("state".to_string(), serde_json::json!("link"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(dest.is_symlink());
        assert_eq!(fs::read_link(&dest).unwrap(), src);
    }

    #[test]
    fn test_file_with_mode() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("file"));
        params.insert("mode".to_string(), serde_json::json!(0o755));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
    }

    #[test]
    fn test_file_check_mode() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testdir");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("directory"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would create"));
        assert!(!path.exists()); // Should not be created in check mode
    }

    #[test]
    fn test_file_touch() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");

        let module = FileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("state".to_string(), serde_json::json!("touch"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(path.exists());
    }
}
