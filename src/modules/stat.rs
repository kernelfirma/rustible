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
        ModuleClassification::NativeTransport
    }

    fn required_params(&self) -> &[&'static str] {
        &["path"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        _context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let path_str = params.get_string_required("path")?;
        let path = Path::new(&path_str);
        let follow = params.get_bool_or("follow", true);
        let get_checksum = params.get_bool_or("checksum", false);
        let checksum_algorithm = params
            .get_string("checksum_algorithm")?
            .unwrap_or_else(|| "sha1".to_string());

        // Execute locally (remote execution would require connection in ModuleContext)
        self.execute_local(path, follow, get_checksum, &checksum_algorithm)
    }
}

impl StatModule {
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
