//! Line-in-file module - Manage lines in text files
//!
//! This module ensures a particular line is in a file, or replaces an existing
//! line using a back-referenced regular expression.
//!
//! Supports both local and remote execution:
//! - Local: Uses native Rust std::fs operations
//! - Remote: Downloads file via connection, edits in memory, uploads back

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::TransferOptions;
use crate::utils::{get_regex, secure_write_file};
use once_cell::sync::Lazy;
use regex::Regex;
use std::borrow::Cow;
use std::fs;
use std::path::Path;

/// Desired state for a line
#[derive(Debug, Clone, PartialEq)]
pub enum LineState {
    Present,
    Absent,
}

impl LineState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(LineState::Present),
            "absent" => Ok(LineState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

impl std::str::FromStr for LineState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LineState::from_str(s)
    }
}

/// Where to insert a new line
#[derive(Debug, Clone, PartialEq)]
pub enum InsertPosition {
    /// After the line matching the regex
    AfterMatch,
    /// Before the line matching the regex
    BeforeMatch,
    /// At the beginning of file
    BeginningOfFile,
    /// At the end of file (default)
    EndOfFile,
}

/// Module for line-in-file operations
pub struct LineinfileModule;

impl LineinfileModule {
    fn read_file_content(path: &Path) -> ModuleResult<Option<String>> {
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(path)?;
        Ok(Some(content))
    }

    fn write_file(
        path: &Path,
        lines: &[Cow<str>],
        create: bool,
        mode: Option<u32>,
    ) -> ModuleResult<()> {
        if !path.exists() && !create {
            return Err(ModuleError::ExecutionFailed(format!(
                "File '{}' does not exist and create=false",
                path.display()
            )));
        }

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let content = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };

        secure_write_file(path, &content, create, mode).map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to write file '{}': {}",
                path.display(),
                e
            ))
        })?;

        Ok(())
    }

    fn create_backup(path: &Path, suffix: &str) -> ModuleResult<Option<String>> {
        if path.exists() {
            let backup_path = format!("{}{}", path.display(), suffix);
            fs::copy(path, &backup_path)?;
            Ok(Some(backup_path))
        } else {
            Ok(None)
        }
    }

    fn find_insert_position(
        lines: &[Cow<str>],
        insertafter: Option<&str>,
        insertbefore: Option<&str>,
    ) -> ModuleResult<(InsertPosition, Option<usize>)> {
        if let Some(pattern) = insertafter {
            match pattern.to_uppercase().as_str() {
                "EOF" => Ok((InsertPosition::EndOfFile, None)),
                "BOF" => Ok((InsertPosition::BeginningOfFile, None)),
                _ => {
                    let re = get_regex(pattern).map_err(|e| {
                        ModuleError::InvalidParameter(format!("Invalid insertafter regex: {}", e))
                    })?;
                    let idx = lines.iter().rposition(|l| re.is_match(l));
                    Ok((InsertPosition::AfterMatch, idx))
                }
            }
        } else if let Some(pattern) = insertbefore {
            match pattern.to_uppercase().as_str() {
                "EOF" => Ok((InsertPosition::EndOfFile, None)),
                "BOF" => Ok((InsertPosition::BeginningOfFile, None)),
                _ => {
                    let re = get_regex(pattern).map_err(|e| {
                        ModuleError::InvalidParameter(format!("Invalid insertbefore regex: {}", e))
                    })?;
                    let idx = lines.iter().position(|l| re.is_match(l));
                    Ok((InsertPosition::BeforeMatch, idx))
                }
            }
        } else {
            Ok((InsertPosition::EndOfFile, None))
        }
    }

    fn ensure_line_present(
        lines: &mut Vec<Cow<'_, str>>,
        line: &str,
        regexp: Option<&Regex>,
        insertafter: Option<&str>,
        insertbefore: Option<&str>,
        firstmatch: bool,
    ) -> ModuleResult<bool> {
        // Check if the exact line already exists
        if lines.iter().any(|l| l == line) {
            // If using regexp, check if the line matches the regexp
            if let Some(re) = regexp {
                if re.is_match(line) {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }

        // If we have a regexp, find and replace matching lines
        if let Some(re) = regexp {
            if firstmatch {
                // Optimized: find first match only and replace
                if let Some(idx) = lines.iter().position(|l| re.is_match(l)) {
                    lines[idx] = Cow::Owned(line.to_string());
                    return Ok(true);
                }
            } else {
                // Optimized: replace in-place without allocating vector of indices
                let mut changed = false;
                for l in lines.iter_mut() {
                    if re.is_match(l) {
                        *l = Cow::Owned(line.to_string());
                        changed = true;
                    }
                }
                if changed {
                    return Ok(true);
                }
            }
        }

        // No match found or no regexp - insert the line
        let (position, match_idx) = Self::find_insert_position(lines, insertafter, insertbefore)?;

        match position {
            InsertPosition::BeginningOfFile => {
                lines.insert(0, Cow::Owned(line.to_string()));
            }
            InsertPosition::EndOfFile => {
                lines.push(Cow::Owned(line.to_string()));
            }
            InsertPosition::AfterMatch => {
                if let Some(idx) = match_idx {
                    lines.insert(idx + 1, Cow::Owned(line.to_string()));
                } else {
                    // Pattern not found - append to end
                    lines.push(Cow::Owned(line.to_string()));
                }
            }
            InsertPosition::BeforeMatch => {
                if let Some(idx) = match_idx {
                    lines.insert(idx, Cow::Owned(line.to_string()));
                } else {
                    // Pattern not found - append to end
                    lines.push(Cow::Owned(line.to_string()));
                }
            }
        }

        Ok(true)
    }

    fn ensure_line_absent(
        lines: &mut Vec<Cow<'_, str>>,
        line: Option<&str>,
        regexp: Option<&Regex>,
    ) -> ModuleResult<bool> {
        let original_len = lines.len();

        lines.retain(|l| {
            if let Some(re) = regexp {
                !re.is_match(l)
            } else if let Some(line_str) = line {
                l != line_str
            } else {
                true
            }
        });

        Ok(lines.len() != original_len)
    }

    fn apply_backrefs<'a>(line: &'a str, regexp: &Regex, original: &'a str) -> Cow<'a, str> {
        static BACKREF_RE: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\\(\d+)").expect("Invalid regex"));

        if let Some(captures) = regexp.captures(original) {
            BACKREF_RE.replace_all(line, |caps: &regex::Captures| {
                let n = caps[1].parse::<usize>().unwrap_or(0);
                if let Some(m) = captures.get(n) {
                    Cow::Borrowed(m.as_str())
                } else {
                    // Fallback: allocate String to avoid lifetime issues with caps reference
                    Cow::Owned(caps.get(0).map_or("", |m| m.as_str()).to_string())
                }
            })
        } else {
            Cow::Borrowed(line)
        }
    }

    /// Execute lineinfile on a remote host via connection
    ///
    /// This method downloads the file content from the remote host, performs line edits
    /// in memory, and uploads the modified content back. This pattern avoids remote
    /// command execution and works across all connection types (SSH, Docker, etc.).
    #[allow(clippy::too_many_arguments)]
    fn execute_remote(
        context: &ModuleContext,
        path: &str,
        state: LineState,
        line: Option<String>,
        regexp: Option<Regex>,
        insertafter: Option<String>,
        insertbefore: Option<String>,
        create: bool,
        backup: bool,
        backup_suffix: String,
        firstmatch: bool,
        backrefs: bool,
        mode: Option<u32>,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed("No connection available for remote execution".to_string())
        })?;

        let conn = connection.clone();
        let check_mode = context.check_mode;
        let diff_mode = context.diff_mode;

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

                    rt.block_on(async move {
                        let remote_path = Path::new(path);

                        // Check if file exists on remote
                        let file_exists = conn.path_exists(remote_path).await.unwrap_or(false);

                        if !file_exists && !create {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "File '{}' does not exist and create=false",
                                path
                            )));
                        }

                        // Download file content (empty if doesn't exist)
                        let file_bytes = if file_exists {
                            conn.download_content(remote_path).await.map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to download file: {}",
                                    e
                                ))
                            })?
                        } else {
                            Vec::new()
                        };

                        // Parse lines from content
                        let content_str = String::from_utf8_lossy(&file_bytes);
                        let mut lines: Vec<Cow<str>> =
                            content_str.lines().map(Cow::Borrowed).collect();
                        let original_lines = if diff_mode { Some(lines.clone()) } else { None };

                        // Apply changes based on state
                        let changed = match state {
                            LineState::Present => {
                                let line_str = line.as_ref().ok_or_else(|| {
                                    ModuleError::MissingParameter(
                                        "line is required for state=present".to_string(),
                                    )
                                })?;

                                // Handle backrefs
                                let final_line = if backrefs {
                                    if let Some(ref re) = regexp {
                                        // Find the matching line and apply backrefs
                                        let matching_line = lines.iter().find(|l| re.is_match(l));
                                        if let Some(orig) = matching_line {
                                            match orig {
                                                Cow::Borrowed(s) => {
                                                    Self::apply_backrefs(line_str, re, s)
                                                }
                                                Cow::Owned(s) => Cow::Owned(
                                                    Self::apply_backrefs(line_str, re, s)
                                                        .into_owned(),
                                                ),
                                            }
                                        } else {
                                            // No match - line won't be added when using backrefs
                                            return Ok(ModuleOutput::ok(format!(
                                                "No match for regexp in '{}'",
                                                path
                                            )));
                                        }
                                    } else {
                                        Cow::Borrowed(line_str.as_str())
                                    }
                                } else {
                                    Cow::Borrowed(line_str.as_str())
                                };

                                Self::ensure_line_present(
                                    &mut lines,
                                    &final_line,
                                    regexp.as_ref(),
                                    insertafter.as_deref(),
                                    insertbefore.as_deref(),
                                    firstmatch,
                                )?
                            }
                            LineState::Absent => Self::ensure_line_absent(
                                &mut lines,
                                line.as_deref(),
                                regexp.as_ref(),
                            )?,
                        };

                        if !changed {
                            return Ok(ModuleOutput::ok(format!(
                                "File '{}' already has desired content",
                                path
                            )));
                        }

                        // In check mode, don't actually write
                        if check_mode {
                            let diff = if diff_mode {
                                Some(Diff::new(
                                    original_lines.as_ref().unwrap().join("\n"),
                                    lines.join("\n"),
                                ))
                            } else {
                                None
                            };

                            let mut output =
                                ModuleOutput::changed(format!("Would modify '{}'", path));

                            if let Some(d) = diff {
                                output = output.with_diff(d);
                            }

                            return Ok(output);
                        }

                        // Create backup if requested
                        if backup && file_exists {
                            let backup_path_str = format!("{}{}", path, backup_suffix);
                            let backup_dest = Path::new(&backup_path_str);

                            conn.upload_content(&file_bytes, backup_dest, None)
                                .await
                                .map_err(|e| {
                                    ModuleError::ExecutionFailed(format!(
                                        "Failed to create backup: {}",
                                        e
                                    ))
                                })?;
                        }

                        // Prepare new content
                        let new_content = if lines.is_empty() {
                            String::new()
                        } else {
                            format!("{}\n", lines.join("\n"))
                        };

                        // Build transfer options
                        let mut transfer_opts = TransferOptions::new();
                        if let Some(m) = mode {
                            transfer_opts = transfer_opts.with_mode(m);
                        }
                        transfer_opts = transfer_opts.with_create_dirs();

                        // Upload modified content
                        conn.upload_content(
                            new_content.as_bytes(),
                            remote_path,
                            Some(transfer_opts),
                        )
                        .await
                        .map_err(|e| {
                            ModuleError::ExecutionFailed(format!("Failed to upload file: {}", e))
                        })?;

                        let mut output = ModuleOutput::changed(format!("Modified '{}'", path));

                        if diff_mode {
                            output = output.with_diff(Diff::new(
                                original_lines.as_ref().unwrap().join("\n"),
                                lines.join("\n"),
                            ));
                        }

                        if backup && file_exists {
                            output = output.with_data(
                                "backup_file",
                                serde_json::json!(format!("{}{}", path, backup_suffix)),
                            );
                        }

                        Ok(output)
                    })
                })
                .join()
                .map_err(|_| ModuleError::ExecutionFailed("Thread panicked".to_string()))?
        })
    }

    /// Execute lineinfile locally using filesystem operations
    #[allow(clippy::too_many_arguments)]
    fn execute_local(
        context: &ModuleContext,
        path_str: &str,
        state: LineState,
        line: Option<String>,
        regexp: Option<Regex>,
        insertafter: Option<String>,
        insertbefore: Option<String>,
        create: bool,
        backup: bool,
        backup_suffix: String,
        firstmatch: bool,
        backrefs: bool,
        mode: Option<u32>,
    ) -> ModuleResult<ModuleOutput> {
        let path = Path::new(path_str);

        // Check if file exists
        if !path.exists() && !create {
            return Err(ModuleError::ExecutionFailed(format!(
                "File '{}' does not exist and create=false",
                path_str
            )));
        }

        // Read current content
        let content_opt = Self::read_file_content(path)?;
        let mut lines: Vec<Cow<str>> = match &content_opt {
            Some(c) => c.lines().map(Cow::Borrowed).collect(),
            None => Vec::new(),
        };

        let original_lines = if context.diff_mode {
            Some(lines.clone())
        } else {
            None
        };

        // Apply changes based on state
        let changed = match state {
            LineState::Present => {
                let line_str = line.as_ref().ok_or_else(|| {
                    ModuleError::MissingParameter("line is required for state=present".to_string())
                })?;

                // Handle backrefs
                let final_line = if backrefs {
                    if let Some(ref re) = regexp {
                        // Find the matching line and apply backrefs
                        let matching_line = lines.iter().find(|l| re.is_match(l));
                        if let Some(orig) = matching_line {
                            match orig {
                                Cow::Borrowed(s) => Self::apply_backrefs(line_str, re, s),
                                Cow::Owned(s) => {
                                    Cow::Owned(Self::apply_backrefs(line_str, re, s).into_owned())
                                }
                            }
                        } else {
                            // No match - line won't be added when using backrefs
                            return Ok(ModuleOutput::ok(format!(
                                "No match for regexp in '{}'",
                                path_str
                            )));
                        }
                    } else {
                        Cow::Borrowed(line_str.as_str())
                    }
                } else {
                    Cow::Borrowed(line_str.as_str())
                };

                Self::ensure_line_present(
                    &mut lines,
                    &final_line,
                    regexp.as_ref(),
                    insertafter.as_deref(),
                    insertbefore.as_deref(),
                    firstmatch,
                )?
            }
            LineState::Absent => {
                Self::ensure_line_absent(&mut lines, line.as_deref(), regexp.as_ref())?
            }
        };

        if !changed {
            return Ok(ModuleOutput::ok(format!(
                "File '{}' already has desired content",
                path_str
            )));
        }

        // In check mode, don't actually write
        if context.check_mode {
            let diff = if context.diff_mode {
                Some(Diff::new(
                    original_lines.as_ref().unwrap().join("\n"),
                    lines.join("\n"),
                ))
            } else {
                None
            };

            let mut output = ModuleOutput::changed(format!("Would modify '{}'", path_str));

            if let Some(d) = diff {
                output = output.with_diff(d);
            }

            return Ok(output);
        }

        // Create backup if requested
        let backup_file = if backup {
            Self::create_backup(path, &backup_suffix)?
        } else {
            None
        };

        // Write the file
        Self::write_file(path, &lines, create, mode)?;

        let mut output = ModuleOutput::changed(format!("Modified '{}'", path_str));

        if let Some(backup_path) = backup_file {
            output = output.with_data("backup_file", serde_json::json!(backup_path));
        }

        if context.diff_mode {
            output = output.with_diff(Diff::new(
                original_lines.as_ref().unwrap().join("\n"),
                lines.join("\n"),
            ));
        }

        Ok(output)
    }
}

impl Module for LineinfileModule {
    fn name(&self) -> &'static str {
        "lineinfile"
    }

    fn description(&self) -> &'static str {
        "Ensure a particular line is in a file"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::NativeTransport
    }

    fn required_params(&self) -> &[&'static str] {
        &["path"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let state = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());

        if state == "present" {
            if params.get("line").is_none() && params.get("regexp").is_none() {
                return Err(ModuleError::MissingParameter(
                    "Either 'line' or 'regexp' is required for state=present".to_string(),
                ));
            }
        } else if state == "absent"
            && params.get("line").is_none()
            && params.get("regexp").is_none()
        {
            return Err(ModuleError::MissingParameter(
                "Either 'line' or 'regexp' is required for state=absent".to_string(),
            ));
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let path_str = params.get_string_required("path")?;
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = LineState::from_str(&state_str)?;
        let line = params.get_string("line")?;
        let regexp_str = params.get_string("regexp")?;
        let insertafter = params.get_string("insertafter")?;
        let insertbefore = params.get_string("insertbefore")?;
        let create = params.get_bool_or("create", false);
        let backup = params.get_bool_or("backup", false);
        let backup_suffix = params
            .get_string("backup_suffix")?
            .unwrap_or_else(|| "~".to_string());
        let firstmatch = params.get_bool_or("firstmatch", false);
        let backrefs = params.get_bool_or("backrefs", false);
        let mode = params.get_u32("mode")?;

        // Compile regexp if provided
        let regexp = if let Some(ref re_str) = regexp_str {
            Some(
                get_regex(re_str)
                    .map_err(|e| ModuleError::InvalidParameter(format!("Invalid regexp: {}", e)))?,
            )
        } else {
            None
        };

        // Route to remote or local execution based on connection availability
        if context.connection.is_some() {
            // Remote execution via connection
            Self::execute_remote(
                context,
                &path_str,
                state,
                line,
                regexp,
                insertafter,
                insertbefore,
                create,
                backup,
                backup_suffix,
                firstmatch,
                backrefs,
                mode,
            )
        } else {
            // Local execution using filesystem operations
            Self::execute_local(
                context,
                &path_str,
                state,
                line,
                regexp,
                insertafter,
                insertbefore,
                create,
                backup,
                backup_suffix,
                firstmatch,
                backrefs,
                mode,
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
    fn test_lineinfile_add_line() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "line1\nline2\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("line3"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("line3"));
    }

    #[test]
    fn test_lineinfile_idempotent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("line2"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
    }

    #[test]
    fn test_lineinfile_regexp_replace() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "key=old_value\nother=stuff\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("regexp".to_string(), serde_json::json!("^key="));
        params.insert("line".to_string(), serde_json::json!("key=new_value"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("key=new_value"));
        assert!(!content.contains("key=old_value"));
    }

    #[test]
    fn test_lineinfile_absent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("line2"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("line2"));
    }

    #[test]
    fn test_lineinfile_insertafter() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("new_line"));
        params.insert("insertafter".to_string(), serde_json::json!("^line1"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let lines: Vec<_> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(lines[1], "new_line");
    }

    #[test]
    fn test_lineinfile_insertbefore() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "line1\nline2\nline3\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("new_line"));
        params.insert("insertbefore".to_string(), serde_json::json!("^line3"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let lines: Vec<_> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(lines[2], "new_line");
    }

    #[test]
    fn test_lineinfile_create() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("new_file.txt");

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("new_line"));
        params.insert("create".to_string(), serde_json::json!(true));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("new_line"));
    }

    #[test]
    fn test_lineinfile_check_mode() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "line1\nline2\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("line".to_string(), serde_json::json!("line3"));

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would modify"));

        // File should not be modified
        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("line3"));
    }

    #[test]
    fn test_lineinfile_regexp_absent() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "# comment1\nkey=value\n# comment2\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("regexp".to_string(), serde_json::json!("^#"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("#"));
        assert!(content.contains("key=value"));
    }

    #[test]
    fn test_apply_backrefs_optimized() {
        let re = Regex::new(r"^key=(\w+)_(\w+)").unwrap();
        let original = "key=value_one";
        let line = r"new_key=\1:\2";

        let result = LineinfileModule::apply_backrefs(line, &re, original);
        assert_eq!(result, "new_key=value:one");
    }

    #[test]
    fn test_apply_backrefs_many_groups() {
        // Test with > 9 groups to verify \10 is handled correctly
        let re = Regex::new(r"(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)").unwrap();
        let original = "abcdefghij";
        let line = r"\10";

        let result = LineinfileModule::apply_backrefs(line, &re, original);
        // Correct behavior should be 'j' (group 10), not 'a0' (group 1 + '0')
        assert_eq!(result, "j");
    }

    #[test]
    fn test_lineinfile_firstmatch() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "match1\nmatch2\n").unwrap();

        let module = LineinfileModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("regexp".to_string(), serde_json::json!("^match"));
        params.insert("line".to_string(), serde_json::json!("replaced"));
        params.insert("firstmatch".to_string(), serde_json::json!(true));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        let content = fs::read_to_string(&path).unwrap();
        // Should only replace first match
        assert!(content.contains("replaced"));
        assert!(!content.contains("match1"));
        assert!(content.contains("match2"));
    }
}
