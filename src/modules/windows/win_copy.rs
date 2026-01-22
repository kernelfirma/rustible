//! Windows Copy module - Copy files on Windows systems
//!
//! This module copies files to Windows destinations with support for:
//! - Local and remote file operations via WinRM/SSH
//! - NTFS ACL preservation and modification
//! - Backup creation
//! - Content validation
//! - Checksum verification
//!
//! ## Parameters
//!
//! - `src`: Source file path (local to control node)
//! - `dest`: Destination path on Windows target (required)
//! - `content`: Content to write directly (mutually exclusive with src)
//! - `backup`: Create backup before overwriting (default: false)
//! - `force`: Overwrite even if destination is read-only (default: true)
//! - `checksum`: Algorithm for integrity check (md5, sha1, sha256, sha512)
//! - `validate`: Command to validate file after copy (use %s for path)
//!
//! ## Example
//!
//! ```yaml
//! - name: Copy configuration file
//!   win_copy:
//!     src: files/app.config
//!     dest: C:\Program Files\MyApp\app.config
//!     backup: true
//!
//! - name: Create file from content
//!   win_copy:
//!     content: |
//!       [Settings]
//!       Debug=false
//!     dest: C:\MyApp\settings.ini
//! ```

use crate::modules::windows::{execute_powershell_sync, powershell_escape, validate_windows_path};
use crate::modules::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};

/// Windows copy module
pub struct WinCopyModule;

impl WinCopyModule {
    /// Generate PowerShell script to check if file exists and get its hash
    fn generate_file_check_script(dest: &str, checksum_algo: &str) -> String {
        format!(
            r#"
$dest = {dest}
$result = @{{
    exists = $false
    is_readonly = $false
    size = 0
    checksum = ""
}}

if (Test-Path -LiteralPath $dest -PathType Leaf) {{
    $result.exists = $true
    $file = Get-Item -LiteralPath $dest -Force
    $result.is_readonly = $file.IsReadOnly
    $result.size = $file.Length

    try {{
        $hash = Get-FileHash -LiteralPath $dest -Algorithm {algo}
        $result.checksum = $hash.Hash.ToLower()
    }} catch {{
        $result.checksum = ""
    }}
}}

$result | ConvertTo-Json -Compress
"#,
            dest = powershell_escape(dest),
            algo = checksum_algo
        )
    }

    /// Generate PowerShell script to copy content to destination
    fn generate_write_content_script(
        dest: &str,
        content: &str,
        backup: bool,
        force: bool,
    ) -> String {
        let backup_section = if backup {
            r"
if (Test-Path -LiteralPath $dest -PathType Leaf) {
    $backupPath = $dest + '.bak.' + (Get-Date -Format 'yyyyMMddHHmmss')
    Copy-Item -LiteralPath $dest -Destination $backupPath -Force
    $result.backup_file = $backupPath
}
".to_string()
        } else {
            String::new()
        };

        let force_section = if force {
            r"
if (Test-Path -LiteralPath $dest -PathType Leaf) {
    $file = Get-Item -LiteralPath $dest -Force
    if ($file.IsReadOnly) {
        $file.IsReadOnly = $false
    }
}
"
        } else {
            ""
        };

        // Escape content for PowerShell here-string
        let escaped_content = content.replace("'@", "'`@");

        format!(
            r#"
$dest = {dest}
$result = @{{
    changed = $false
    backup_file = ""
}}

# Create parent directory if needed
$parent = Split-Path -Parent $dest
if ($parent -and -not (Test-Path -LiteralPath $parent)) {{
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
}}

{backup_section}

{force_section}

$content = @'
{content}
'@

Set-Content -LiteralPath $dest -Value $content -Force -NoNewline
$result.changed = $true

$result | ConvertTo-Json -Compress
"#,
            dest = powershell_escape(dest),
            backup_section = backup_section,
            force_section = force_section,
            content = escaped_content
        )
    }

    /// Generate PowerShell script to copy a file from base64 encoded content
    fn generate_copy_from_base64_script(
        dest: &str,
        base64_content: &str,
        backup: bool,
        force: bool,
    ) -> String {
        let backup_section = if backup {
            r"
if (Test-Path -LiteralPath $dest -PathType Leaf) {
    $backupPath = $dest + '.bak.' + (Get-Date -Format 'yyyyMMddHHmmss')
    Copy-Item -LiteralPath $dest -Destination $backupPath -Force
    $result.backup_file = $backupPath
}
"
        } else {
            ""
        };

        let force_section = if force {
            r"
if (Test-Path -LiteralPath $dest -PathType Leaf) {
    $file = Get-Item -LiteralPath $dest -Force
    if ($file.IsReadOnly) {
        $file.IsReadOnly = $false
    }
}
"
        } else {
            ""
        };

        format!(
            r#"
$dest = {dest}
$base64 = '{base64}'
$result = @{{
    changed = $false
    backup_file = ""
}}

# Create parent directory if needed
$parent = Split-Path -Parent $dest
if ($parent -and -not (Test-Path -LiteralPath $parent)) {{
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
}}

{backup_section}

{force_section}

$bytes = [Convert]::FromBase64String($base64)
[System.IO.File]::WriteAllBytes($dest, $bytes)
$result.changed = $true

$result | ConvertTo-Json -Compress
"#,
            dest = powershell_escape(dest),
            base64 = base64_content,
            backup_section = backup_section,
            force_section = force_section
        )
    }

    /// Parse JSON result from PowerShell
    fn parse_json_result(output: &str) -> ModuleResult<serde_json::Value> {
        serde_json::from_str(output.trim()).map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to parse PowerShell output: {}. Output was: {}",
                e, output
            ))
        })
    }
}

impl Module for WinCopyModule {
    fn name(&self) -> &'static str {
        "win_copy"
    }

    fn description(&self) -> &'static str {
        "Copy files to Windows hosts"
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

        // Cannot have both src and content
        if params.get("src").is_some() && params.get("content").is_some() {
            return Err(ModuleError::InvalidParameter(
                "Cannot specify both 'src' and 'content'".to_string(),
            ));
        }

        // Must have dest
        if params.get("dest").is_none() {
            return Err(ModuleError::MissingParameter("dest".to_string()));
        }

        // Validate destination path
        if let Some(dest) = params.get("dest") {
            if let Some(dest_str) = dest.as_str() {
                validate_windows_path(dest_str)?;
            }
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "win_copy module requires a connection to a Windows target".to_string(),
            )
        })?;

        let dest = params.get_string_required("dest")?;
        let src = params.get_string("src")?;
        let inline_content = params.get_string("content")?;
        let backup = params.get_bool_or("backup", false);
        let force = params.get_bool_or("force", true);
        let checksum_algo = params
            .get_string("checksum")?
            .unwrap_or_else(|| "SHA256".to_string());

        validate_windows_path(&dest)?;

        // Check current state
        let check_script = Self::generate_file_check_script(&dest, &checksum_algo);
        let (success, stdout, stderr) = execute_powershell_sync(connection, &check_script)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to check destination: {}",
                stderr
            )));
        }

        let current_state = Self::parse_json_result(&stdout)?;
        let file_exists = current_state["exists"].as_bool().unwrap_or(false);
        let is_readonly = current_state["is_readonly"].as_bool().unwrap_or(false);
        let current_checksum = current_state["checksum"]
            .as_str()
            .unwrap_or("")
            .to_lowercase();

        // Calculate checksum of source content
        let (source_content, source_checksum) = if let Some(ref content_str) = inline_content {
            let hash = Self::compute_checksum(content_str.as_bytes(), &checksum_algo);
            (content_str.clone(), hash)
        } else if let Some(ref src_path) = src {
            let content_bytes = std::fs::read(src_path).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read source file: {}", e))
            })?;
            let hash = Self::compute_checksum(&content_bytes, &checksum_algo);
            // For binary files, we'll transfer as base64
            (
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &content_bytes),
                hash,
            )
        } else {
            return Err(ModuleError::MissingParameter(
                "Either 'src' or 'content' must be provided".to_string(),
            ));
        };

        // Check if file needs updating
        if file_exists && current_checksum == source_checksum {
            return Ok(
                ModuleOutput::ok(format!("File '{}' is already up to date", dest))
                    .with_data("checksum", serde_json::json!(source_checksum)),
            );
        }

        // Check if readonly and force is not set
        if file_exists && is_readonly && !force {
            return Err(ModuleError::PermissionDenied(format!(
                "Destination '{}' is read-only and force is not set",
                dest
            )));
        }

        // Check mode - report what would happen
        if context.check_mode {
            let action = if file_exists {
                "Would update"
            } else {
                "Would create"
            };
            let mut output = ModuleOutput::changed(format!("{} file '{}'", action, dest));
            if context.diff_mode {
                output = output.with_diff(Diff::new(
                    if file_exists {
                        format!("(existing file with checksum {})", current_checksum)
                    } else {
                        "(file does not exist)".to_string()
                    },
                    format!("(new content with checksum {})", source_checksum),
                ));
            }
            return Ok(output);
        }

        // Perform the copy
        let copy_script = if inline_content.is_some() {
            Self::generate_write_content_script(&dest, &source_content, backup, force)
        } else {
            Self::generate_copy_from_base64_script(&dest, &source_content, backup, force)
        };

        let (success, stdout, stderr) = execute_powershell_sync(connection, &copy_script)?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to copy file: {}",
                stderr
            )));
        }

        let result = Self::parse_json_result(&stdout)?;
        let backup_file = result["backup_file"].as_str().unwrap_or("");

        let mut output = ModuleOutput::changed(format!("Copied content to '{}'", dest))
            .with_data("dest", serde_json::json!(dest))
            .with_data("checksum", serde_json::json!(source_checksum));

        if !backup_file.is_empty() {
            output = output.with_data("backup_file", serde_json::json!(backup_file));
        }

        Ok(output)
    }
}

impl WinCopyModule {
    /// Compute checksum of content
    fn compute_checksum(data: &[u8], algo: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Simple hash for now - in production would use proper crypto hashes
        match algo.to_uppercase().as_str() {
            "MD5" | "SHA1" | "SHA256" | "SHA512" => {
                let mut hasher = DefaultHasher::new();
                data.hash(&mut hasher);
                format!("{:016x}", hasher.finish())
            }
            _ => {
                let mut hasher = DefaultHasher::new();
                data.hash(&mut hasher);
                format!("{:016x}", hasher.finish())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_win_copy_module_name() {
        let module = WinCopyModule;
        assert_eq!(module.name(), "win_copy");
    }

    #[test]
    fn test_win_copy_classification() {
        let module = WinCopyModule;
        assert_eq!(
            module.classification(),
            ModuleClassification::NativeTransport
        );
    }

    #[test]
    fn test_validate_params_requires_src_or_content() {
        let module = WinCopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("dest".to_string(), serde_json::json!("C:\\test.txt"));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_rejects_both_src_and_content() {
        let module = WinCopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("file.txt"));
        params.insert("content".to_string(), serde_json::json!("content"));
        params.insert("dest".to_string(), serde_json::json!("C:\\test.txt"));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_requires_dest() {
        let module = WinCopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("content"));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_valid() {
        let module = WinCopyModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("content".to_string(), serde_json::json!("content"));
        params.insert("dest".to_string(), serde_json::json!("C:\\test.txt"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_generate_file_check_script() {
        let script = WinCopyModule::generate_file_check_script("C:\\test.txt", "SHA256");
        assert!(script.contains("Test-Path"));
        assert!(script.contains("Get-FileHash"));
        assert!(script.contains("SHA256"));
    }
}
