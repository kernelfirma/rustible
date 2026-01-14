//! Archive module - Create compressed archives
//!
//! This module creates compressed archives from files and directories.
//! Supports tar, tar.gz, tar.bz2, and zip formats with configurable compression
//! levels and file exclusion patterns.
//!
//! # Supported Formats
//!
//! - **tar**: Plain tar archive (no compression)
//! - **tar.gz / tgz**: Gzip-compressed tar archive
//! - **zip**: Zip archive with deflate compression
//!
//! # Examples
//!
//! ```yaml
//! # Create a tar.gz archive
//! - archive:
//!     path: /path/to/files
//!     dest: /tmp/backup.tar.gz
//!     format: gz
//!
//! # Create a zip archive with exclusions
//! - archive:
//!     path: /path/to/project
//!     dest: /tmp/project.zip
//!     format: zip
//!     exclude_path:
//!       - node_modules
//!       - .git
//! ```

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

/// Supported archive formats
#[derive(Debug, Clone, PartialEq)]
pub enum ArchiveFormat {
    /// Plain tar archive
    Tar,
    /// Gzip-compressed tar archive
    TarGz,
    /// Zip archive
    Zip,
}

impl ArchiveFormat {
    /// Parse format from string
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "tar" => Ok(ArchiveFormat::Tar),
            "gz" | "tar.gz" | "tgz" | "gzip" => Ok(ArchiveFormat::TarGz),
            "zip" => Ok(ArchiveFormat::Zip),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Unsupported archive format '{}'. Valid formats: tar, gz, zip",
                s
            ))),
        }
    }

    /// Infer format from file extension
    fn from_path(path: &Path) -> Option<Self> {
        let filename = path.file_name()?.to_str()?;
        let lower = filename.to_lowercase();

        if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            Some(ArchiveFormat::TarGz)
        } else if lower.ends_with(".tar") {
            Some(ArchiveFormat::Tar)
        } else if lower.ends_with(".zip") {
            Some(ArchiveFormat::Zip)
        } else if lower.ends_with(".gz") {
            Some(ArchiveFormat::TarGz)
        } else {
            None
        }
    }

    /// Get default file extension for this format
    fn extension(&self) -> &'static str {
        match self {
            ArchiveFormat::Tar => ".tar",
            ArchiveFormat::TarGz => ".tar.gz",
            ArchiveFormat::Zip => ".zip",
        }
    }
}

/// Module for creating archives
pub struct ArchiveModule;

impl ArchiveModule {
    /// Check if a path matches any exclusion pattern
    fn is_excluded(path: &Path, base: &Path, exclude_patterns: &[String]) -> bool {
        let relative = match path.strip_prefix(base) {
            Ok(p) => p,
            Err(_) => return false,
        };

        let relative_str = relative.to_string_lossy();

        for pattern in exclude_patterns {
            // Simple matching: check if any component matches the pattern
            if relative_str.contains(pattern) {
                return true;
            }

            // Also check the filename directly
            if let Some(filename) = path.file_name() {
                if filename.to_string_lossy() == *pattern {
                    return true;
                }
            }
        }

        false
    }

    /// Collect all files to archive, respecting exclusions
    fn collect_files(
        source: &Path,
        exclude_patterns: &[String],
    ) -> ModuleResult<Vec<std::path::PathBuf>> {
        let mut files = Vec::new();

        if source.is_file() {
            files.push(source.to_path_buf());
            return Ok(files);
        }

        let base = source;
        for entry in walkdir::WalkDir::new(source)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            if Self::is_excluded(path, base, exclude_patterns) {
                continue;
            }

            files.push(path.to_path_buf());
        }

        Ok(files)
    }

    /// Create a tar archive (optionally gzip-compressed)
    fn create_tar_archive(
        source: &Path,
        dest: &Path,
        compress: bool,
        compression_level: u32,
        exclude_patterns: &[String],
        remove_source: bool,
    ) -> ModuleResult<ArchiveStats> {
        let files = Self::collect_files(source, exclude_patterns)?;
        let file_count = files.len();
        let mut total_size: u64 = 0;

        // Create parent directories
        if let Some(parent) = dest.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Create the archive
        let dest_file = File::create(dest)?;

        if compress {
            let level = match compression_level {
                0 => Compression::none(),
                1..=3 => Compression::fast(),
                4..=6 => Compression::default(),
                _ => Compression::best(),
            };
            let encoder = GzEncoder::new(dest_file, level);
            let mut builder = tar::Builder::new(encoder);

            for file_path in &files {
                if file_path.is_file() {
                    let meta = fs::metadata(file_path)?;
                    total_size += meta.len();

                    let relative_path = file_path.strip_prefix(source).unwrap_or(file_path);
                    builder.append_path_with_name(file_path, relative_path)?;
                } else if file_path.is_dir() && file_path != source {
                    let relative_path = file_path.strip_prefix(source).unwrap_or(file_path);
                    builder.append_dir(relative_path, file_path)?;
                }
            }

            builder.finish()?;
        } else {
            let mut builder = tar::Builder::new(dest_file);

            for file_path in &files {
                if file_path.is_file() {
                    let meta = fs::metadata(file_path)?;
                    total_size += meta.len();

                    let relative_path = file_path.strip_prefix(source).unwrap_or(file_path);
                    builder.append_path_with_name(file_path, relative_path)?;
                } else if file_path.is_dir() && file_path != source {
                    let relative_path = file_path.strip_prefix(source).unwrap_or(file_path);
                    builder.append_dir(relative_path, file_path)?;
                }
            }

            builder.finish()?;
        }

        // Remove source if requested
        if remove_source {
            if source.is_dir() {
                fs::remove_dir_all(source)?;
            } else {
                fs::remove_file(source)?;
            }
        }

        let archive_size = fs::metadata(dest)?.len();

        Ok(ArchiveStats {
            file_count,
            original_size: total_size,
            archive_size,
        })
    }

    /// Create a zip archive
    fn create_zip_archive(
        source: &Path,
        dest: &Path,
        compression_level: u32,
        exclude_patterns: &[String],
        remove_source: bool,
    ) -> ModuleResult<ArchiveStats> {
        use zip::write::SimpleFileOptions;
        use zip::CompressionMethod;

        let files = Self::collect_files(source, exclude_patterns)?;
        let file_count = files.len();
        let mut total_size: u64 = 0;

        // Create parent directories
        if let Some(parent) = dest.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let dest_file = File::create(dest)?;
        let mut zip = zip::ZipWriter::new(dest_file);

        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .compression_level(Some(compression_level as i64));

        for file_path in &files {
            let relative_path = file_path.strip_prefix(source).unwrap_or(file_path);
            let relative_str = relative_path.to_string_lossy();

            if file_path.is_file() {
                let meta = fs::metadata(file_path)?;
                total_size += meta.len();

                zip.start_file(relative_str.as_ref(), options)
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!("Failed to add file: {}", e))
                    })?;

                let mut file = File::open(file_path)?;
                std::io::copy(&mut file, &mut zip)?;
            } else if file_path.is_dir() && file_path != source {
                // Add directory entries with trailing slash
                let dir_name = format!("{}/", relative_str);
                zip.add_directory(&dir_name, options).map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to add directory: {}", e))
                })?;
            }
        }

        zip.finish()
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to finish zip: {}", e)))?;

        // Remove source if requested
        if remove_source {
            if source.is_dir() {
                fs::remove_dir_all(source)?;
            } else {
                fs::remove_file(source)?;
            }
        }

        let archive_size = fs::metadata(dest)?.len();

        Ok(ArchiveStats {
            file_count,
            original_size: total_size,
            archive_size,
        })
    }

    /// Compute checksum of a file
    fn compute_checksum(path: &Path, algorithm: &str) -> ModuleResult<String> {
        use sha2::{Digest, Sha256};

        let mut file = File::open(path)?;
        // Use 64KB buffer for streaming to prevent memory exhaustion
        const BUFFER_SIZE: usize = 64 * 1024;
        let mut buffer = [0u8; BUFFER_SIZE];

        match algorithm.to_lowercase().as_str() {
            "sha256" | "sha-256" => {
                let mut hasher = Sha256::new();
                loop {
                    let bytes_read = file.read(&mut buffer)?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                let hash = hasher.finalize();
                Ok(format!("{:x}", hash))
            }
            "md5" => {
                let mut context = md5::Context::new();
                loop {
                    let bytes_read = file.read(&mut buffer)?;
                    if bytes_read == 0 {
                        break;
                    }
                    context.consume(&buffer[..bytes_read]);
                }
                let hash = context.compute();
                Ok(format!("{:x}", hash))
            }
            "sha1" | "sha-1" => {
                use sha1::Digest as Sha1Digest;
                let mut hasher = sha1::Sha1::new();
                loop {
                    let bytes_read = file.read(&mut buffer)?;
                    if bytes_read == 0 {
                        break;
                    }
                    hasher.update(&buffer[..bytes_read]);
                }
                let hash = hasher.finalize();
                Ok(format!("{:x}", hash))
            }
            _ => Err(ModuleError::InvalidParameter(format!(
                "Unsupported checksum algorithm '{}'. Use sha256, md5, or sha1",
                algorithm
            ))),
        }
    }
}

/// Statistics about the created archive
#[derive(Debug)]
struct ArchiveStats {
    file_count: usize,
    original_size: u64,
    archive_size: u64,
}

impl Module for ArchiveModule {
    fn name(&self) -> &'static str {
        "archive"
    }

    fn description(&self) -> &'static str {
        "Create compressed archives from files and directories"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::NativeTransport
    }

    fn required_params(&self) -> &[&'static str] {
        &["path", "dest"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Validate path parameter
        if params.get("path").is_none() {
            return Err(ModuleError::MissingParameter("path".to_string()));
        }

        // Validate dest parameter
        if params.get("dest").is_none() {
            return Err(ModuleError::MissingParameter("dest".to_string()));
        }

        // Validate format if provided
        if let Some(format_str) = params.get_string("format")? {
            ArchiveFormat::from_str(&format_str)?;
        }

        // Validate compression level
        if let Some(level) = params.get_u32("compression_level")? {
            if level > 9 {
                return Err(ModuleError::InvalidParameter(
                    "compression_level must be between 0 and 9".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let source_path = params.get_string_required("path")?;
        let dest_path = params.get_string_required("dest")?;
        let source = Path::new(&source_path);
        let dest = Path::new(&dest_path);

        // Validate source exists
        if !source.exists() {
            return Err(ModuleError::ExecutionFailed(format!(
                "Source path '{}' does not exist",
                source_path
            )));
        }

        // Determine format
        let format = if let Some(format_str) = params.get_string("format")? {
            ArchiveFormat::from_str(&format_str)?
        } else {
            ArchiveFormat::from_path(dest).ok_or_else(|| {
                ModuleError::InvalidParameter(
                    "Cannot determine archive format from destination path. Specify 'format' parameter.".to_string()
                )
            })?
        };

        // Get options
        let compression_level = params.get_u32("compression_level")?.unwrap_or(6);
        let remove_source = params.get_bool_or("remove", false);
        let force = params.get_bool_or("force", true);

        // Get exclusion patterns
        let exclude_patterns: Vec<String> =
            params.get_vec_string("exclude_path")?.unwrap_or_default();

        // Check if destination already exists
        if dest.exists() && !force {
            return Ok(ModuleOutput::ok(format!(
                "Archive '{}' already exists and force=false",
                dest_path
            )));
        }

        // Check mode - report what would happen
        if context.check_mode {
            let file_count = Self::collect_files(source, &exclude_patterns)?.len();
            return Ok(ModuleOutput::changed(format!(
                "Would create {:?} archive '{}' from '{}' ({} files)",
                format, dest_path, source_path, file_count
            ))
            .with_diff(Diff::new(
                "archive: absent",
                format!("archive: {} ({} files)", dest_path, file_count),
            )));
        }

        // Create the archive
        let stats = match format {
            ArchiveFormat::Tar => Self::create_tar_archive(
                source,
                dest,
                false,
                compression_level,
                &exclude_patterns,
                remove_source,
            )?,
            ArchiveFormat::TarGz => Self::create_tar_archive(
                source,
                dest,
                true,
                compression_level,
                &exclude_patterns,
                remove_source,
            )?,
            ArchiveFormat::Zip => Self::create_zip_archive(
                source,
                dest,
                compression_level,
                &exclude_patterns,
                remove_source,
            )?,
        };

        // Compute checksum if requested
        let checksum_alg = params.get_string("checksum")?;
        let checksum = if let Some(ref alg) = checksum_alg {
            Some(Self::compute_checksum(dest, alg)?)
        } else {
            None
        };

        let compression_ratio = if stats.original_size > 0 {
            (stats.archive_size as f64 / stats.original_size as f64) * 100.0
        } else {
            100.0
        };

        let mut output = ModuleOutput::changed(format!(
            "Created {:?} archive '{}' from '{}' ({} files, {:.1}% of original size)",
            format, dest_path, source_path, stats.file_count, compression_ratio
        ))
        .with_data("dest", serde_json::json!(dest_path))
        .with_data("format", serde_json::json!(format!("{:?}", format)))
        .with_data("file_count", serde_json::json!(stats.file_count))
        .with_data("original_size", serde_json::json!(stats.original_size))
        .with_data("archive_size", serde_json::json!(stats.archive_size))
        .with_data(
            "compression_ratio",
            serde_json::json!(format!("{:.1}%", compression_ratio)),
        );

        if let Some(ref sum) = checksum {
            output = output.with_data(
                "checksum",
                serde_json::json!({
                    "algorithm": checksum_alg.unwrap_or_else(|| "sha256".to_string()),
                    "value": sum
                }),
            );
        }

        if remove_source {
            output = output.with_data("source_removed", serde_json::json!(true));
        }

        Ok(output)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn create_test_structure(temp: &TempDir) -> std::path::PathBuf {
        let base = temp.path().join("source");
        fs::create_dir_all(&base).unwrap();

        // Create some test files
        fs::write(base.join("file1.txt"), "Content of file 1").unwrap();
        fs::write(base.join("file2.txt"), "Content of file 2").unwrap();

        // Create a subdirectory with files
        let subdir = base.join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("file3.txt"), "Content of file 3").unwrap();

        // Create an excluded directory
        let excluded = base.join("node_modules");
        fs::create_dir(&excluded).unwrap();
        fs::write(excluded.join("module.js"), "module content").unwrap();

        base
    }

    #[test]
    fn test_archive_module_name() {
        let module = ArchiveModule;
        assert_eq!(module.name(), "archive");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_archive_format_from_str() {
        assert_eq!(ArchiveFormat::from_str("tar").unwrap(), ArchiveFormat::Tar);
        assert_eq!(ArchiveFormat::from_str("gz").unwrap(), ArchiveFormat::TarGz);
        assert_eq!(
            ArchiveFormat::from_str("tar.gz").unwrap(),
            ArchiveFormat::TarGz
        );
        assert_eq!(ArchiveFormat::from_str("zip").unwrap(), ArchiveFormat::Zip);
        assert!(ArchiveFormat::from_str("invalid").is_err());
    }

    #[test]
    fn test_archive_format_from_path() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("test.tar")),
            Some(ArchiveFormat::Tar)
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("test.tar.gz")),
            Some(ArchiveFormat::TarGz)
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("test.tgz")),
            Some(ArchiveFormat::TarGz)
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("test.zip")),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(ArchiveFormat::from_path(Path::new("test.unknown")), None);
    }

    #[test]
    fn test_create_tar_gz_archive() {
        let temp = TempDir::new().unwrap();
        let source = create_test_structure(&temp);
        let dest = temp.path().join("archive.tar.gz");

        let module = ArchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(source.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("format".to_string(), serde_json::json!("gz"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(dest.exists());
        assert!(result.data.contains_key("file_count"));
        assert!(result.data.contains_key("archive_size"));
    }

    #[test]
    fn test_create_zip_archive() {
        let temp = TempDir::new().unwrap();
        let source = create_test_structure(&temp);
        let dest = temp.path().join("archive.zip");

        let module = ArchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(source.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(dest.exists());
    }

    #[test]
    fn test_archive_with_exclusions() {
        let temp = TempDir::new().unwrap();
        let source = create_test_structure(&temp);
        let dest = temp.path().join("archive.tar.gz");

        let module = ArchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(source.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert(
            "exclude_path".to_string(),
            serde_json::json!(["node_modules"]),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);

        // The file count should not include node_modules
        let file_count = result.data.get("file_count").unwrap().as_u64().unwrap();
        // source dir + file1 + file2 + subdir + file3 = 5 (excluding node_modules/*)
        assert!(file_count < 7); // Less than total including node_modules
    }

    #[test]
    fn test_archive_check_mode() {
        let temp = TempDir::new().unwrap();
        let source = create_test_structure(&temp);
        let dest = temp.path().join("archive.tar.gz");

        let module = ArchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(source.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would create"));
        assert!(!dest.exists()); // File should not be created
    }

    #[test]
    fn test_archive_with_checksum() {
        let temp = TempDir::new().unwrap();
        let source = create_test_structure(&temp);
        let dest = temp.path().join("archive.tar.gz");

        let module = ArchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "path".to_string(),
            serde_json::json!(source.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("checksum".to_string(), serde_json::json!("sha256"));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.data.contains_key("checksum"));

        let checksum = result.data.get("checksum").unwrap();
        assert!(checksum.get("value").is_some());
        assert_eq!(checksum.get("algorithm").unwrap(), "sha256");
    }

    #[test]
    fn test_archive_missing_source() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("archive.tar.gz");

        let module = ArchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("path".to_string(), serde_json::json!("/nonexistent/path"));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context);

        assert!(result.is_err());
    }

    #[test]
    fn test_archive_validate_params() {
        let module = ArchiveModule;

        // Missing path
        let mut params: ModuleParams = HashMap::new();
        params.insert("dest".to_string(), serde_json::json!("/tmp/test.tar.gz"));
        assert!(module.validate_params(&params).is_err());

        // Missing dest
        let mut params: ModuleParams = HashMap::new();
        params.insert("path".to_string(), serde_json::json!("/tmp/source"));
        assert!(module.validate_params(&params).is_err());

        // Invalid compression level
        let mut params: ModuleParams = HashMap::new();
        params.insert("path".to_string(), serde_json::json!("/tmp/source"));
        params.insert("dest".to_string(), serde_json::json!("/tmp/test.tar.gz"));
        params.insert("compression_level".to_string(), serde_json::json!(15));
        assert!(module.validate_params(&params).is_err());

        // Valid params
        let mut params: ModuleParams = HashMap::new();
        params.insert("path".to_string(), serde_json::json!("/tmp/source"));
        params.insert("dest".to_string(), serde_json::json!("/tmp/test.tar.gz"));
        params.insert("compression_level".to_string(), serde_json::json!(6));
        assert!(module.validate_params(&params).is_ok());
    }
}
