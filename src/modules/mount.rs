//! Mount module - Mount point management
//!
//! This module manages filesystem mount points, including mounting, unmounting,
//! and configuring persistent mounts in /etc/fstab.

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use std::sync::Arc;
use tokio::runtime::Handle;

/// Desired state for a mount point
#[derive(Debug, Clone, PartialEq)]
pub enum MountState {
    /// Mount point should be mounted (and in fstab)
    Mounted,
    /// Mount point should be unmounted (but kept in fstab)
    Unmounted,
    /// Mount point should be in fstab only (not necessarily mounted)
    Present,
    /// Mount point should not exist (removed from fstab and unmounted)
    Absent,
    /// Mount point should be remounted (useful for applying option changes)
    Remounted,
}

impl MountState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "mounted" => Ok(MountState::Mounted),
            "unmounted" => Ok(MountState::Unmounted),
            "present" => Ok(MountState::Present),
            "absent" => Ok(MountState::Absent),
            "remounted" => Ok(MountState::Remounted),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: mounted, unmounted, present, absent, remounted",
                s
            ))),
        }
    }
}

/// Information about a mount entry
#[derive(Debug, Clone)]
pub struct MountEntry {
    pub src: String,
    pub path: String,
    pub fstype: String,
    pub opts: String,
    pub dump: u32,
    pub passno: u32,
}

impl MountEntry {
    /// Generate an fstab line for this entry
    pub fn to_fstab_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            self.src, self.path, self.fstype, self.opts, self.dump, self.passno
        )
    }

    /// Parse an fstab line into a MountEntry
    pub fn from_fstab_line(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return None;
        }

        Some(Self {
            src: parts[0].to_string(),
            path: parts[1].to_string(),
            fstype: parts[2].to_string(),
            opts: parts[3].to_string(),
            dump: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
            passno: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
        })
    }
}

/// Module for mount point management
pub struct MountModule;

impl MountModule {
    /// Get execution options with become support if needed
    fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
        let mut options = ExecuteOptions::new();
        if context.r#become {
            options = options.with_escalation(context.become_user.clone());
            if let Some(ref method) = context.become_method {
                options.escalate_method = Some(method.clone());
            }
        }
        options
    }

    /// Execute a command via connection
    fn execute_command(
        connection: &Arc<dyn Connection + Send + Sync>,
        command: &str,
        context: &ModuleContext,
    ) -> ModuleResult<(bool, String, String)> {
        let options = Self::get_exec_options(context);

        let result = Handle::current()
            .block_on(async { connection.execute(command, Some(options)).await })
            .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Check if a path is currently mounted
    fn is_mounted(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let cmd = format!(
            "findmnt -n {} >/dev/null 2>&1 && echo mounted || echo not_mounted",
            shell_escape(path)
        );
        let (_, stdout, _) = Self::execute_command(connection, &cmd, context)?;
        Ok(stdout.trim() == "mounted")
    }

    /// Get current mount info for a path
    #[allow(dead_code)]
    fn get_mount_info(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        context: &ModuleContext,
    ) -> ModuleResult<Option<MountEntry>> {
        let cmd = format!(
            "findmnt -n -o SOURCE,TARGET,FSTYPE,OPTIONS {} 2>/dev/null",
            shell_escape(path)
        );
        let (success, stdout, _) = Self::execute_command(connection, &cmd, context)?;

        if !success || stdout.trim().is_empty() {
            return Ok(None);
        }

        let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
        if parts.len() >= 4 {
            Ok(Some(MountEntry {
                src: parts[0].to_string(),
                path: parts[1].to_string(),
                fstype: parts[2].to_string(),
                opts: parts[3].to_string(),
                dump: 0,
                passno: 0,
            }))
        } else {
            Ok(None)
        }
    }

    /// Read /etc/fstab content
    fn read_fstab(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let (success, stdout, stderr) =
            Self::execute_command(connection, "cat /etc/fstab", context)?;
        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to read /etc/fstab: {}",
                stderr
            )));
        }
        Ok(stdout)
    }

    /// Write /etc/fstab content
    fn write_fstab(
        connection: &Arc<dyn Connection + Send + Sync>,
        content: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Create a backup first
        let backup_cmd = "cp /etc/fstab /etc/fstab.bak";
        Self::execute_command(connection, backup_cmd, context)?;

        // Write new content
        let cmd = format!(
            "cat << 'RUSTIBLE_EOF' > /etc/fstab\n{}\nRUSTIBLE_EOF",
            content.trim()
        );
        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;

        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to write /etc/fstab: {}",
                stderr
            )))
        }
    }

    /// Find entry in fstab by mount path
    fn find_fstab_entry(fstab: &str, path: &str) -> Option<(usize, MountEntry)> {
        for (i, line) in fstab.lines().enumerate() {
            if let Some(entry) = MountEntry::from_fstab_line(line) {
                if entry.path == path {
                    return Some((i, entry));
                }
            }
        }
        None
    }

    /// Update or add entry in fstab
    fn update_fstab(fstab: &str, entry: &MountEntry) -> (String, bool) {
        let mut lines: Vec<String> = fstab.lines().map(|s| s.to_string()).collect();
        let mut found = false;

        for line in &mut lines {
            if let Some(existing) = MountEntry::from_fstab_line(line) {
                if existing.path == entry.path {
                    *line = entry.to_fstab_line();
                    found = true;
                    break;
                }
            }
        }

        if !found {
            lines.push(entry.to_fstab_line());
        }

        (lines.join("\n"), !found)
    }

    /// Remove entry from fstab by mount path
    fn remove_from_fstab(fstab: &str, path: &str) -> (String, bool) {
        let mut lines = Vec::new();
        let mut removed = false;

        for line in fstab.lines() {
            if let Some(entry) = MountEntry::from_fstab_line(line) {
                if entry.path == path {
                    removed = true;
                    continue;
                }
            }
            lines.push(line);
        }

        (lines.join("\n"), removed)
    }

    /// Mount a filesystem
    fn mount(
        connection: &Arc<dyn Connection + Send + Sync>,
        src: &str,
        path: &str,
        fstype: &str,
        opts: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        // Create mount point if it doesn't exist
        let mkdir_cmd = format!("mkdir -p {}", shell_escape(path));
        Self::execute_command(connection, &mkdir_cmd, context)?;

        let cmd = format!(
            "mount -t {} -o {} {} {}",
            shell_escape(fstype),
            shell_escape(opts),
            shell_escape(src),
            shell_escape(path)
        );

        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to mount: {}",
                stderr
            )))
        }
    }

    /// Unmount a filesystem
    fn unmount(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        force: bool,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let force_flag = if force { "-f" } else { "" };
        let cmd = format!("umount {} {}", force_flag, shell_escape(path));

        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to unmount: {}",
                stderr
            )))
        }
    }

    /// Remount a filesystem with new options
    fn remount(
        connection: &Arc<dyn Connection + Send + Sync>,
        path: &str,
        opts: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let cmd = format!(
            "mount -o remount,{} {}",
            shell_escape(opts),
            shell_escape(path)
        );

        let (success, _, stderr) = Self::execute_command(connection, &cmd, context)?;
        if success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to remount: {}",
                stderr
            )))
        }
    }
}

impl Module for MountModule {
    fn name(&self) -> &'static str {
        "mount"
    }

    fn description(&self) -> &'static str {
        "Manage filesystem mount points"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn required_params(&self) -> &[&'static str] {
        &["path"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "Mount module requires a connection for remote execution".to_string(),
            )
        })?;

        let path = params.get_string_required("path")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "mounted".to_string());
        let state = MountState::from_str(&state_str)?;

        let src = params.get_string("src")?;
        let fstype = params
            .get_string("fstype")?
            .unwrap_or_else(|| "auto".to_string());
        let opts = params
            .get_string("opts")?
            .unwrap_or_else(|| "defaults".to_string());
        let dump = params.get_u32("dump")?.unwrap_or(0);
        let passno = params.get_u32("passno")?.unwrap_or(0);
        let fstab_path = params
            .get_string("fstab")?
            .unwrap_or_else(|| "/etc/fstab".to_string());
        let force = params.get_bool_or("force", false);

        let is_mounted = Self::is_mounted(connection, &path, context)?;
        let fstab_content = Self::read_fstab(connection, context)?;
        let fstab_entry = Self::find_fstab_entry(&fstab_content, &path);

        let mut changed = false;
        let mut messages = Vec::new();

        match state {
            MountState::Absent => {
                // Unmount if mounted
                if is_mounted {
                    if context.check_mode {
                        messages.push(format!("Would unmount '{}'", path));
                        changed = true;
                    } else {
                        Self::unmount(connection, &path, force, context)?;
                        messages.push(format!("Unmounted '{}'", path));
                        changed = true;
                    }
                }

                // Remove from fstab if present
                if fstab_entry.is_some() {
                    if context.check_mode {
                        messages.push(format!("Would remove '{}' from {}", path, fstab_path));
                        changed = true;
                    } else {
                        let (new_fstab, _) = Self::remove_from_fstab(&fstab_content, &path);
                        Self::write_fstab(connection, &new_fstab, context)?;
                        messages.push(format!("Removed '{}' from {}", path, fstab_path));
                        changed = true;
                    }
                }

                if !changed {
                    return Ok(ModuleOutput::ok(format!(
                        "Mount point '{}' already absent",
                        path
                    )));
                }
            }

            MountState::Unmounted => {
                // Unmount if mounted
                if is_mounted {
                    if context.check_mode {
                        messages.push(format!("Would unmount '{}'", path));
                        changed = true;
                    } else {
                        Self::unmount(connection, &path, force, context)?;
                        messages.push(format!("Unmounted '{}'", path));
                        changed = true;
                    }
                } else {
                    messages.push(format!("'{}' is not mounted", path));
                }

                // Ensure fstab entry exists if src is provided
                if let Some(ref src) = src {
                    let entry = MountEntry {
                        src: src.clone(),
                        path: path.clone(),
                        fstype: fstype.clone(),
                        opts: opts.clone(),
                        dump,
                        passno,
                    };

                    let needs_fstab_update = match &fstab_entry {
                        Some((_, existing)) => {
                            existing.src != entry.src
                                || existing.fstype != entry.fstype
                                || existing.opts != entry.opts
                                || existing.dump != entry.dump
                                || existing.passno != entry.passno
                        }
                        None => true,
                    };

                    if needs_fstab_update {
                        if context.check_mode {
                            messages.push(format!("Would update '{}' in {}", path, fstab_path));
                            changed = true;
                        } else {
                            let (new_fstab, _) = Self::update_fstab(&fstab_content, &entry);
                            Self::write_fstab(connection, &new_fstab, context)?;
                            messages.push(format!("Updated '{}' in {}", path, fstab_path));
                            changed = true;
                        }
                    }
                }
            }

            MountState::Present => {
                let src = src.ok_or_else(|| {
                    ModuleError::MissingParameter(
                        "src is required when state is present".to_string(),
                    )
                })?;

                let entry = MountEntry {
                    src: src.clone(),
                    path: path.clone(),
                    fstype: fstype.clone(),
                    opts: opts.clone(),
                    dump,
                    passno,
                };

                let needs_fstab_update = match &fstab_entry {
                    Some((_, existing)) => {
                        existing.src != entry.src
                            || existing.fstype != entry.fstype
                            || existing.opts != entry.opts
                            || existing.dump != entry.dump
                            || existing.passno != entry.passno
                    }
                    None => true,
                };

                if needs_fstab_update {
                    if context.check_mode {
                        messages.push(format!("Would update '{}' in {}", path, fstab_path));
                        changed = true;
                    } else {
                        let (new_fstab, _) = Self::update_fstab(&fstab_content, &entry);
                        Self::write_fstab(connection, &new_fstab, context)?;
                        messages.push(format!("Updated '{}' in {}", path, fstab_path));
                        changed = true;
                    }
                } else {
                    messages.push(format!("'{}' already present in {}", path, fstab_path));
                }
            }

            MountState::Mounted => {
                let src = src.ok_or_else(|| {
                    ModuleError::MissingParameter(
                        "src is required when state is mounted".to_string(),
                    )
                })?;

                let entry = MountEntry {
                    src: src.clone(),
                    path: path.clone(),
                    fstype: fstype.clone(),
                    opts: opts.clone(),
                    dump,
                    passno,
                };

                // Update fstab if needed
                let needs_fstab_update = match &fstab_entry {
                    Some((_, existing)) => {
                        existing.src != entry.src
                            || existing.fstype != entry.fstype
                            || existing.opts != entry.opts
                            || existing.dump != entry.dump
                            || existing.passno != entry.passno
                    }
                    None => true,
                };

                if needs_fstab_update {
                    if context.check_mode {
                        messages.push(format!("Would update '{}' in {}", path, fstab_path));
                        changed = true;
                    } else {
                        let (new_fstab, _) = Self::update_fstab(&fstab_content, &entry);
                        Self::write_fstab(connection, &new_fstab, context)?;
                        messages.push(format!("Updated '{}' in {}", path, fstab_path));
                        changed = true;
                    }
                }

                // Mount if not mounted
                if !is_mounted {
                    if context.check_mode {
                        messages.push(format!("Would mount '{}'", path));
                        changed = true;
                    } else {
                        Self::mount(connection, &src, &path, &fstype, &opts, context)?;
                        messages.push(format!("Mounted '{}'", path));
                        changed = true;
                    }
                } else {
                    messages.push(format!("'{}' is already mounted", path));
                }
            }

            MountState::Remounted => {
                if !is_mounted {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Cannot remount '{}': not mounted",
                        path
                    )));
                }

                if context.check_mode {
                    messages.push(format!("Would remount '{}'", path));
                    changed = true;
                } else {
                    Self::remount(connection, &path, &opts, context)?;
                    messages.push(format!("Remounted '{}'", path));
                    changed = true;
                }
            }
        }

        let msg = if messages.is_empty() {
            format!("Mount point '{}' is in desired state", path)
        } else {
            messages.join(". ")
        };

        if changed {
            Ok(ModuleOutput::changed(msg))
        } else {
            Ok(ModuleOutput::ok(msg))
        }
    }

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        self.execute(params, &check_context)
    }

    fn diff(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let connection = match context.connection.as_ref() {
            Some(c) => c,
            None => return Ok(None),
        };

        let path = params.get_string_required("path")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "mounted".to_string());
        let state = MountState::from_str(&state_str)?;

        let is_mounted = Self::is_mounted(connection, &path, context).unwrap_or(false);
        let fstab_content = Self::read_fstab(connection, context).unwrap_or_default();
        let fstab_entry = Self::find_fstab_entry(&fstab_content, &path);

        let before = format!(
            "path: {}\nmounted: {}\nfstab: {}",
            path,
            if is_mounted { "yes" } else { "no" },
            fstab_entry
                .as_ref()
                .map(|(_, e)| e.to_fstab_line())
                .unwrap_or_else(|| "(not in fstab)".to_string())
        );

        let after = match state {
            MountState::Absent => format!("path: {}\nmounted: no\nfstab: (not in fstab)", path),
            MountState::Unmounted => {
                let src = params.get_string("src")?.unwrap_or_default();
                let fstype = params
                    .get_string("fstype")?
                    .unwrap_or_else(|| "auto".to_string());
                let opts = params
                    .get_string("opts")?
                    .unwrap_or_else(|| "defaults".to_string());
                format!(
                    "path: {}\nmounted: no\nfstab: {} {} {} {}",
                    path, src, path, fstype, opts
                )
            }
            MountState::Present | MountState::Mounted => {
                let src = params.get_string("src")?.unwrap_or_default();
                let fstype = params
                    .get_string("fstype")?
                    .unwrap_or_else(|| "auto".to_string());
                let opts = params
                    .get_string("opts")?
                    .unwrap_or_else(|| "defaults".to_string());
                let mounted = matches!(state, MountState::Mounted);
                format!(
                    "path: {}\nmounted: {}\nfstab: {} {} {} {}",
                    path,
                    if mounted { "yes" } else { "no" },
                    src,
                    path,
                    fstype,
                    opts
                )
            }
            MountState::Remounted => before.clone(),
        };

        if before == after {
            Ok(None)
        } else {
            Ok(Some(Diff::new(before, after)))
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_state_from_str() {
        assert_eq!(
            MountState::from_str("mounted").unwrap(),
            MountState::Mounted
        );
        assert_eq!(
            MountState::from_str("unmounted").unwrap(),
            MountState::Unmounted
        );
        assert_eq!(
            MountState::from_str("present").unwrap(),
            MountState::Present
        );
        assert_eq!(MountState::from_str("absent").unwrap(), MountState::Absent);
        assert_eq!(
            MountState::from_str("remounted").unwrap(),
            MountState::Remounted
        );
        assert!(MountState::from_str("invalid").is_err());
    }

    #[test]
    fn test_mount_entry_to_fstab_line() {
        let entry = MountEntry {
            src: "/dev/sda1".to_string(),
            path: "/mnt/data".to_string(),
            fstype: "ext4".to_string(),
            opts: "defaults,noatime".to_string(),
            dump: 0,
            passno: 2,
        };

        let line = entry.to_fstab_line();
        assert!(line.contains("/dev/sda1"));
        assert!(line.contains("/mnt/data"));
        assert!(line.contains("ext4"));
        assert!(line.contains("defaults,noatime"));
    }

    #[test]
    fn test_mount_entry_from_fstab_line() {
        let line = "/dev/sda1\t/mnt/data\text4\tdefaults\t0\t2";
        let entry = MountEntry::from_fstab_line(line).unwrap();

        assert_eq!(entry.src, "/dev/sda1");
        assert_eq!(entry.path, "/mnt/data");
        assert_eq!(entry.fstype, "ext4");
        assert_eq!(entry.opts, "defaults");
        assert_eq!(entry.dump, 0);
        assert_eq!(entry.passno, 2);
    }

    #[test]
    fn test_mount_entry_from_fstab_line_minimal() {
        let line = "UUID=abc123 /boot ext4 defaults";
        let entry = MountEntry::from_fstab_line(line).unwrap();

        assert_eq!(entry.src, "UUID=abc123");
        assert_eq!(entry.path, "/boot");
        assert_eq!(entry.dump, 0);
        assert_eq!(entry.passno, 0);
    }

    #[test]
    fn test_mount_entry_from_comment() {
        let line = "# This is a comment";
        assert!(MountEntry::from_fstab_line(line).is_none());
    }

    #[test]
    fn test_find_fstab_entry() {
        let fstab = r#"
# /etc/fstab
/dev/sda1    /boot    ext4    defaults    0    2
/dev/sda2    /        ext4    defaults    0    1
/dev/sdb1    /mnt/data    xfs    defaults,noatime    0    0
"#;

        let result = MountModule::find_fstab_entry(fstab, "/mnt/data");
        assert!(result.is_some());
        let (_, entry) = result.unwrap();
        assert_eq!(entry.src, "/dev/sdb1");
        assert_eq!(entry.fstype, "xfs");

        let result = MountModule::find_fstab_entry(fstab, "/nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_update_fstab() {
        let fstab = "/dev/sda1\t/boot\text4\tdefaults\t0\t2\n";
        let entry = MountEntry {
            src: "/dev/sdb1".to_string(),
            path: "/mnt/data".to_string(),
            fstype: "xfs".to_string(),
            opts: "defaults".to_string(),
            dump: 0,
            passno: 0,
        };

        let (result, is_new) = MountModule::update_fstab(fstab, &entry);
        assert!(is_new);
        assert!(result.contains("/mnt/data"));
        assert!(result.contains("/boot"));
    }

    #[test]
    fn test_remove_from_fstab() {
        let fstab = r#"/dev/sda1    /boot    ext4    defaults    0    2
/dev/sdb1    /mnt/data    xfs    defaults    0    0"#;

        let (result, removed) = MountModule::remove_from_fstab(fstab, "/mnt/data");
        assert!(removed);
        assert!(!result.contains("/mnt/data"));
        assert!(result.contains("/boot"));
    }

    #[test]
    fn test_mount_module_metadata() {
        let module = MountModule;
        assert_eq!(module.name(), "mount");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["path"]);
    }
}
