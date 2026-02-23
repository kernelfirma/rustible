//! Stat module - Retrieve file information
//!
//! This module retrieves file or directory information from a target system.
//! It returns detailed file statistics including size, permissions, ownership,
//! timestamps, and file type information.
//!
//! The returned information can be registered and used in subsequent tasks for
//! conditional execution or decision-making.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::ExecuteOptions;
use crate::utils::shell_escape;
use serde_json::json;
use std::path::Path;

/// Module for retrieving file statistics
pub struct StatModule;

impl Module for StatModule {
    fn name(&self) -> &'static str {
        "stat"
    }

    fn description(&self) -> &'static str {
        "Retrieve file or directory information"
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
        let path_str = params.get_string_required("path")?;
        let follow = params.get_bool_or("follow", true);
        let get_checksum = params.get_bool_or("checksum", false);
        let checksum_algorithm = params
            .get_string("checksum_algorithm")?
            .unwrap_or_else(|| "sha1".to_string());

        if let Some(ref conn) = context.connection {
            // Remote execution via connection
            let exec_options = Self::build_exec_options(context);
            let conn = conn.clone();
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    Self::execute_remote(
                        conn.as_ref(),
                        &path_str,
                        follow,
                        get_checksum,
                        &checksum_algorithm,
                        Some(exec_options),
                    )
                    .await
                })
            })
        } else {
            // Local execution fallback
            let path = Path::new(&path_str);
            self.execute_local(path, follow, get_checksum, &checksum_algorithm)
        }
    }
}

impl StatModule {
    /// Build execution options with become/sudo if needed
    fn build_exec_options(context: &ModuleContext) -> ExecuteOptions {
        let mut options = ExecuteOptions::new();

        if context.r#become {
            options.escalate = true;
            options.escalate_user = context
                .become_user
                .clone()
                .or_else(|| Some("root".to_string()));
            options.escalate_method = context.become_method.clone();
            options.escalate_password = context.become_password.clone();
        }

        options
    }

    /// Execute stat on a remote system via connection
    async fn execute_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        path_str: &str,
        follow: bool,
        get_checksum: bool,
        checksum_algorithm: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<ModuleOutput> {
        // Build the stat command
        // %F=file type, %s=size, %a=octal mode, %u=uid, %g=gid, %X=atime, %Y=mtime, %h=hard links, %i=inode
        let stat_cmd = if follow {
            format!(
                "stat -L -c '%F|%s|%a|%u|%g|%X|%Y|%h|%i' {} 2>/dev/null || echo 'NOTFOUND'",
                shell_escape(path_str)
            )
        } else {
            format!(
                "stat -c '%F|%s|%a|%u|%g|%X|%Y|%h|%i' {} 2>/dev/null || echo 'NOTFOUND'",
                shell_escape(path_str)
            )
        };

        let result = conn
            .execute(&stat_cmd, options.clone())
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to execute stat command: {}", e))
            })?;

        let output = result.stdout.trim().to_string();

        // Check if path does not exist
        if output == "NOTFOUND" || output.is_empty() {
            return Ok(
                ModuleOutput::ok(format!("Path '{}' does not exist", path_str)).with_data(
                    "stat",
                    json!({
                        "exists": false,
                        "path": path_str,
                    }),
                ),
            );
        }

        // Parse the stat output: "file_type|size|mode|uid|gid|atime|mtime|nlink|inode"
        let parts: Vec<&str> = output.split('|').collect();
        if parts.len() < 9 {
            return Err(ModuleError::ExecutionFailed(format!(
                "Unexpected stat output format: {}",
                output
            )));
        }

        let file_type = parts[0];
        let size: u64 = parts[1].parse().unwrap_or(0);
        let mode_str = parts[2];
        let mode: u32 = u32::from_str_radix(mode_str, 8).unwrap_or(0);
        let uid: u32 = parts[3].parse().unwrap_or(0);
        let gid: u32 = parts[4].parse().unwrap_or(0);
        let atime: i64 = parts[5].parse().unwrap_or(0);
        let mtime: i64 = parts[6].parse().unwrap_or(0);

        // Determine file type flags from %F output
        let is_file = file_type == "regular file" || file_type == "regular empty file";
        let is_dir = file_type == "directory";
        let is_symlink = file_type == "symbolic link";

        // Build stat data
        let mut stat_data = json!({
            "exists": true,
            "path": path_str,
            "mode": format!("{:04o}", mode),
            "isdir": is_dir,
            "isreg": is_file,
            "islnk": is_symlink,
            "size": size,
            "uid": uid,
            "gid": gid,
            "atime": atime,
            "mtime": mtime,
            "readable": true,
            "writeable": (mode & 0o200) != 0,
            "executable": (mode & 0o111) != 0,
        });

        // Get checksum if requested and it's a regular file
        if get_checksum && is_file {
            let algorithm = match checksum_algorithm.to_lowercase().as_str() {
                "md5" => "md5",
                "sha256" => "sha256",
                "sha512" => "sha512",
                _ => "sha1",
            };
            let checksum_cmd = format!(
                "{}sum {} | cut -d' ' -f1",
                algorithm,
                shell_escape(path_str)
            );

            let cksum_result = conn
                .execute(&checksum_cmd, options.clone())
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!(
                        "Failed to execute checksum command: {}",
                        e
                    ))
                })?;

            let checksum = cksum_result.stdout.trim().to_string();
            if !checksum.is_empty() && cksum_result.success {
                if let Some(stat_obj) = stat_data.as_object_mut() {
                    stat_obj.insert("checksum".to_string(), json!(checksum));
                }
            }
        }

        // Get symlink target if it's a symlink
        if is_symlink {
            let link_cmd = format!("readlink {}", shell_escape(path_str));
            let link_result = conn
                .execute(&link_cmd, options.clone())
                .await
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to read symlink: {}", e))
                })?;

            let target = link_result.stdout.trim().to_string();
            if !target.is_empty() && link_result.success {
                if let Some(stat_obj) = stat_data.as_object_mut() {
                    stat_obj.insert("lnk_source".to_string(), json!(target));
                }
            }
        }

        Ok(
            ModuleOutput::ok(format!("Retrieved stats for '{}'", path_str))
                .with_data("stat", stat_data),
        )
    }

    /// Execute stat on local system
    fn execute_local(
        &self,
        path: &Path,
        follow: bool,
        get_checksum: bool,
        checksum_algorithm: &str,
    ) -> ModuleResult<ModuleOutput> {
        // Check if path exists
        let exists = if follow {
            path.exists()
        } else {
            path.symlink_metadata().is_ok()
        };

        if !exists {
            return Ok(
                ModuleOutput::ok(format!("Path '{}' does not exist", path.display())).with_data(
                    "stat",
                    json!({
                        "exists": false,
                        "path": path.display().to_string(),
                    }),
                ),
            );
        }

        // Get metadata
        let metadata = if follow {
            std::fs::metadata(path)
        } else {
            std::fs::symlink_metadata(path)
        }
        .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to get metadata: {}", e)))?;

        // Extract file stats
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        #[cfg(unix)]
        let (mode, uid, gid, atime, mtime) = {
            (
                metadata.mode(),
                metadata.uid(),
                metadata.gid(),
                metadata.atime(),
                metadata.mtime(),
            )
        };

        #[cfg(not(unix))]
        let (mode, uid, gid, atime, mtime) = { (0o644u32, 0u32, 0u32, 0i64, 0i64) };

        let is_dir = metadata.is_dir();
        let is_file = metadata.is_file();
        let is_symlink = metadata.file_type().is_symlink();

        // Build stat data
        let mut stat_data = json!({
            "exists": true,
            "path": path.display().to_string(),
            "mode": format!("{:04o}", mode),
            "isdir": is_dir,
            "isreg": is_file,
            "islnk": is_symlink,
            "size": metadata.len(),
            "uid": uid,
            "gid": gid,
            "atime": atime,
            "mtime": mtime,
            "readable": true,
            "writeable": (mode & 0o200) != 0,
            "executable": (mode & 0o111) != 0,
        });

        // Get checksum if requested and it's a regular file
        if get_checksum && is_file {
            let checksum = self.calculate_checksum_local(path, checksum_algorithm)?;
            if let Some(stat_obj) = stat_data.as_object_mut() {
                stat_obj.insert("checksum".to_string(), json!(checksum));
            }
        }

        // Get symlink target if it's a symlink and follow is true
        if is_symlink && follow {
            if let Ok(target) = std::fs::read_link(path) {
                if let Some(stat_obj) = stat_data.as_object_mut() {
                    stat_obj.insert(
                        "lnk_source".to_string(),
                        json!(target.display().to_string()),
                    );
                }
            }
        }

        Ok(
            ModuleOutput::ok(format!("Retrieved stats for '{}'", path.display()))
                .with_data("stat", stat_data),
        )
    }

    /// Calculate checksum for a local file
    fn calculate_checksum_local(&self, path: &Path, algorithm: &str) -> ModuleResult<String> {
        use std::io::Read;

        let mut file = std::fs::File::open(path)
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to open file: {}", e)))?;

        // Use 64KB buffer for streaming to prevent memory exhaustion
        const BUFFER_SIZE: usize = 64 * 1024;
        let mut buffer = [0u8; BUFFER_SIZE];

        match algorithm.to_lowercase().as_str() {
            "md5" => {
                let mut context = md5::Context::new();
                loop {
                    let bytes_read = file.read(&mut buffer).map_err(|e| {
                        ModuleError::ExecutionFailed(format!("Failed to read file: {}", e))
                    })?;
                    if bytes_read == 0 {
                        break;
                    }
                    context.consume(&buffer[..bytes_read]);
                }
                let digest = context.compute();
                Ok(format!("{:x}", digest))
            }
            "sha1" => {
                use sha1::Digest;
                let mut hasher = sha1::Sha1::new();
                loop {
                    let bytes_read = file.read(&mut buffer).map_err(|e| {
                        ModuleError::ExecutionFailed(format!("Failed to read file: {}", e))
                    })?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                let result = hasher.finalize();
                Ok(format!("{:x}", result))
            }
            "sha256" => {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                loop {
                    let bytes_read = file.read(&mut buffer).map_err(|e| {
                        ModuleError::ExecutionFailed(format!("Failed to read file: {}", e))
                    })?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                let result = hasher.finalize();
                Ok(format!("{:x}", result))
            }
            _ => Err(ModuleError::InvalidParameter(format!(
                "Unsupported checksum algorithm: {}",
                algorithm
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_stat_existing_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");
        std::fs::write(&path, "test content").unwrap();

        let module = StatModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.status, super::super::ModuleStatus::Ok);
        assert!(result.data.contains_key("stat"));

        let stat = &result.data["stat"];
        assert_eq!(stat["exists"], true);
        assert_eq!(stat["isreg"], true);
        assert_eq!(stat["isdir"], false);
        assert_eq!(stat["size"], 12);
    }

    #[test]
    fn test_stat_nonexistent_file() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("nonexistent");

        let module = StatModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.status, super::super::ModuleStatus::Ok);
        assert!(result.data.contains_key("stat"));

        let stat = &result.data["stat"];
        assert_eq!(stat["exists"], false);
    }

    #[test]
    fn test_stat_directory() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testdir");
        std::fs::create_dir(&path).unwrap();

        let module = StatModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert_eq!(result.status, super::super::ModuleStatus::Ok);

        let stat = &result.data["stat"];
        assert_eq!(stat["exists"], true);
        assert_eq!(stat["isdir"], true);
        assert_eq!(stat["isreg"], false);
    }

    #[test]
    fn test_stat_symlink() {
        let temp = TempDir::new().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");

        std::fs::write(&target, "content").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&target, &link).unwrap();
        }

        #[cfg(not(unix))]
        {
            // Skip test on non-Unix systems
            return;
        }

        let module = StatModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(link.to_str().unwrap()),
        );
        params.insert("follow".to_string(), serde_json::json!(false));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        let stat = &result.data["stat"];
        assert_eq!(stat["exists"], true);
        assert_eq!(stat["islnk"], true);
    }

    #[test]
    fn test_stat_with_checksum() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");
        std::fs::write(&path, "test content").unwrap();

        let module = StatModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );
        params.insert("checksum".to_string(), serde_json::json!(true));
        params.insert("checksum_algorithm".to_string(), serde_json::json!("sha1"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        let stat = &result.data["stat"];
        assert!(stat.get("checksum").is_some());
    }

    #[test]
    fn test_stat_check_mode() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("testfile");
        std::fs::write(&path, "test content").unwrap();

        let module = StatModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(path.to_str().unwrap()),
        );

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        // Stat is read-only, so it should work the same in check mode
        assert_eq!(result.status, super::super::ModuleStatus::Ok);
        let stat = &result.data["stat"];
        assert_eq!(stat["exists"], true);
    }
}
