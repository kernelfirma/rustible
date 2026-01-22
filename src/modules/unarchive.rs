//! Unarchive module - Extract compressed archives
//!
//! This module extracts compressed archives to a destination directory.
//! Supports tar, tar.gz, tar.bz2, and zip formats with configurable extraction
//! options and checksum verification.
//!
//! # Supported Formats
//!
//! - **tar**: Plain tar archive (no compression)
//! - **tar.gz / tgz**: Gzip-compressed tar archive
//! - **zip**: Zip archive
//!
//! # Features
//!
//! - Remote URL downloading with extraction
//! - Checksum verification (md5, sha1, sha256)
//! - Selective extraction with include/exclude patterns
//! - Permission preservation
//! - Remote file support via connection
//!
//! # Examples
//!
//! ```yaml
//! # Extract a local archive
//! - unarchive:
//!     src: /path/to/archive.tar.gz
//!     dest: /path/to/destination
//!
//! # Download and extract from URL
//! - unarchive:
//!     src: https://example.com/archive.tar.gz
//!     dest: /opt/app
//!     remote_src: true
//!     checksum: sha256:abc123...
//!
//! # Extract with exclusions
//! - unarchive:
//!     src: /tmp/backup.zip
//!     dest: /restore
//!     exclude:
//!       - "*.log"
//!       - "tmp/*"
//! ```

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use flate2::read::GzDecoder;
#[cfg(test)]
use flate2::write::GzEncoder;
#[cfg(test)]
use flate2::Compression;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

/// Supported archive formats for extraction
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
    /// Infer format from file extension or magic bytes
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

    /// Detect format from magic bytes
    fn from_magic_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 4 {
            return None;
        }

        // Gzip magic bytes: 1f 8b
        if data[0] == 0x1f && data[1] == 0x8b {
            return Some(ArchiveFormat::TarGz);
        }

        // Zip magic bytes: 50 4b 03 04
        if data[0] == 0x50 && data[1] == 0x4b && data[2] == 0x03 && data[3] == 0x04 {
            return Some(ArchiveFormat::Zip);
        }

        // Tar files have "ustar" at offset 257
        if data.len() >= 262 {
            let magic = &data[257..262];
            if magic == b"ustar" {
                return Some(ArchiveFormat::Tar);
            }
        }

        None
    }

    /// Parse format from string
    fn from_str(s: &str) -> ModuleResult<Self> {
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
}

/// Module for extracting archives
pub struct UnarchiveModule;

impl UnarchiveModule {
    /// Check if URL is a valid remote source
    fn is_url(src: &str) -> bool {
        src.starts_with("http://") || src.starts_with("https://") || src.starts_with("ftp://")
    }

    /// Download a file from a URL
    fn download_file(url: &str, dest: &Path) -> ModuleResult<()> {
        // Use blocking reqwest client
        let response = reqwest::blocking::get(url).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to download '{}': {}", url, e))
        })?;

        if !response.status().is_success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "HTTP error {} downloading '{}'",
                response.status(),
                url
            )));
        }

        let bytes = response.bytes().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to read response from '{}': {}", url, e))
        })?;

        // Create parent directories
        if let Some(parent) = dest.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let mut file = File::create(dest)?;
        file.write_all(&bytes)?;

        Ok(())
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

    /// Verify checksum of a file
    /// Checksum format: "algorithm:hash" e.g., "sha256:abc123..."
    fn verify_checksum(path: &Path, expected: &str) -> ModuleResult<bool> {
        let parts: Vec<&str> = expected.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid checksum format '{}'. Expected 'algorithm:hash'",
                expected
            )));
        }

        let algorithm = parts[0];
        let expected_hash = parts[1].to_lowercase();
        let actual_hash = Self::compute_checksum(path, algorithm)?;

        Ok(actual_hash == expected_hash)
    }

    /// Check if a path matches any exclusion pattern
    fn is_excluded(path: &str, exclude_patterns: &[String], include_patterns: &[String]) -> bool {
        // If include patterns are specified, path must match at least one
        if !include_patterns.is_empty() {
            let matches_include = include_patterns
                .iter()
                .any(|pattern| Self::pattern_matches(path, pattern));
            if !matches_include {
                return true;
            }
        }

        // Check exclusion patterns
        for pattern in exclude_patterns {
            if Self::pattern_matches(path, pattern) {
                return true;
            }
        }

        false
    }

    /// Simple glob-like pattern matching
    fn pattern_matches(path: &str, pattern: &str) -> bool {
        if pattern.contains('*') {
            // Simple wildcard matching
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                let prefix = parts[0];
                let suffix = parts[1];
                if prefix.is_empty() && suffix.is_empty() {
                    return true; // "*" matches everything
                }
                if prefix.is_empty() {
                    return path.ends_with(suffix);
                }
                if suffix.is_empty() {
                    return path.starts_with(prefix);
                }
                return path.starts_with(prefix) && path.ends_with(suffix);
            }
        }

        // Exact match or contains
        path == pattern || path.contains(pattern)
    }

    /// Validate that an archive entry path is safe (no traversal, no absolute)
    fn validate_entry_path(path: &Path) -> bool {
        if path.is_absolute() {
            return false;
        }

        for component in path.components() {
            match component {
                std::path::Component::ParentDir => return false,
                std::path::Component::Prefix(_) => return false,
                std::path::Component::RootDir => return false,
                _ => {}
            }
        }
        true
    }

    /// Extract a tar archive (optionally gzip-compressed)
    fn extract_tar(
        src: &Path,
        dest: &Path,
        compressed: bool,
        exclude_patterns: &[String],
        include_patterns: &[String],
        keep_newer: bool,
    ) -> ModuleResult<ExtractionStats> {
        let file = File::open(src)?;
        let mut extracted_count = 0;
        let mut skipped_count = 0;
        let mut total_size: u64 = 0;

        // Create destination directory
        if !dest.exists() {
            fs::create_dir_all(dest)?;
        }

        if compressed {
            let decoder = GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);
            archive.set_preserve_permissions(true);
            archive.set_preserve_mtime(true);

            for entry in archive.entries()? {
                let mut entry = entry?;
                let entry_path = entry.path()?.into_owned();

                // Security check: Prevent path traversal
                if !Self::validate_entry_path(&entry_path) {
                    skipped_count += 1;
                    continue;
                }

                let entry_path_str = entry_path.to_string_lossy();

                // Check exclusions
                if Self::is_excluded(&entry_path_str, exclude_patterns, include_patterns) {
                    skipped_count += 1;
                    continue;
                }

                let target_path = dest.join(&entry_path);

                // Check if target is newer
                if keep_newer && target_path.exists() {
                    if let Ok(target_meta) = fs::metadata(&target_path) {
                        let entry_mtime = entry.header().mtime().unwrap_or(0);
                        let target_mtime = target_meta
                            .modified()
                            .map(|t| {
                                t.duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0)
                            })
                            .unwrap_or(0);

                        if target_mtime > entry_mtime {
                            skipped_count += 1;
                            continue;
                        }
                    }
                }

                total_size += entry.header().size().unwrap_or(0);
                entry.unpack_in(dest)?;
                extracted_count += 1;
            }
        } else {
            let mut archive = tar::Archive::new(file);
            archive.set_preserve_permissions(true);
            archive.set_preserve_mtime(true);

            for entry in archive.entries()? {
                let mut entry = entry?;
                let entry_path = entry.path()?.into_owned();

                // Security check: Prevent path traversal
                if !Self::validate_entry_path(&entry_path) {
                    skipped_count += 1;
                    continue;
                }

                let entry_path_str = entry_path.to_string_lossy();

                // Check exclusions
                if Self::is_excluded(&entry_path_str, exclude_patterns, include_patterns) {
                    skipped_count += 1;
                    continue;
                }

                let target_path = dest.join(&entry_path);

                // Check if target is newer
                if keep_newer && target_path.exists() {
                    if let Ok(target_meta) = fs::metadata(&target_path) {
                        let entry_mtime = entry.header().mtime().unwrap_or(0);
                        let target_mtime = target_meta
                            .modified()
                            .map(|t| {
                                t.duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0)
                            })
                            .unwrap_or(0);

                        if target_mtime > entry_mtime {
                            skipped_count += 1;
                            continue;
                        }
                    }
                }

                total_size += entry.header().size().unwrap_or(0);
                entry.unpack_in(dest)?;
                extracted_count += 1;
            }
        }

        Ok(ExtractionStats {
            extracted_count,
            skipped_count,
            total_size,
        })
    }

    /// Extract a zip archive
    fn extract_zip(
        src: &Path,
        dest: &Path,
        exclude_patterns: &[String],
        include_patterns: &[String],
        keep_newer: bool,
    ) -> ModuleResult<ExtractionStats> {
        let file = File::open(src)?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to open zip archive: {}", e))
        })?;

        let mut extracted_count = 0;
        let mut skipped_count = 0;
        let mut total_size: u64 = 0;

        // Create destination directory
        if !dest.exists() {
            fs::create_dir_all(dest)?;
        }

        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to read zip entry: {}", e))
            })?;

            let entry_path = match file.enclosed_name() {
                Some(p) => p.to_owned(),
                None => continue, // Skip entries with invalid paths
            };

            let entry_path_str = entry_path.to_string_lossy();

            // Check exclusions
            if Self::is_excluded(&entry_path_str, exclude_patterns, include_patterns) {
                skipped_count += 1;
                continue;
            }

            let target_path = dest.join(&entry_path);

            // Check if target is newer
            if keep_newer && target_path.exists() && file.is_file() {
                if let Ok(target_meta) = fs::metadata(&target_path) {
                    let entry_mtime = file
                        .last_modified()
                        .and_then(|dt| {
                            #[allow(deprecated)]
                            dt.to_time().ok()
                        })
                        .map(|dt| dt.unix_timestamp() as u64)
                        .unwrap_or(0);
                    let target_mtime = target_meta
                        .modified()
                        .map(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0)
                        })
                        .unwrap_or(0);

                    if target_mtime > entry_mtime {
                        skipped_count += 1;
                        continue;
                    }
                }
            }

            if file.is_dir() {
                fs::create_dir_all(&target_path)?;
            } else {
                // Create parent directories
                if let Some(parent) = target_path.parent() {
                    if !parent.exists() {
                        fs::create_dir_all(parent)?;
                    }
                }

                let mut outfile = File::create(&target_path)?;
                std::io::copy(&mut file, &mut outfile)?;

                total_size += file.size();

                // Set permissions on Unix
                #[cfg(unix)]
                if let Some(mode) = file.unix_mode() {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&target_path, fs::Permissions::from_mode(mode))?;
                }
            }

            extracted_count += 1;
        }

        Ok(ExtractionStats {
            extracted_count,
            skipped_count,
            total_size,
        })
    }

    /// Create a marker file to track extraction state
    fn create_marker(dest: &Path, archive_checksum: &str) -> ModuleResult<()> {
        let marker_path = dest.join(".unarchive_marker");
        let content = format!(
            "{{\"checksum\": \"{}\", \"timestamp\": {}}}",
            archive_checksum,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );
        fs::write(marker_path, content)?;
        Ok(())
    }

    /// Check if archive was already extracted (idempotency)
    fn check_marker(dest: &Path, archive_checksum: &str) -> bool {
        let marker_path = dest.join(".unarchive_marker");
        if !marker_path.exists() {
            return false;
        }

        if let Ok(content) = fs::read_to_string(&marker_path) {
            content.contains(archive_checksum)
        } else {
            false
        }
    }
}

/// Statistics about the extraction
#[derive(Debug)]
struct ExtractionStats {
    extracted_count: usize,
    skipped_count: usize,
    total_size: u64,
}

impl Module for UnarchiveModule {
    fn name(&self) -> &'static str {
        "unarchive"
    }

    fn description(&self) -> &'static str {
        "Extract compressed archives with optional URL download"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::NativeTransport
    }

    fn required_params(&self) -> &[&'static str] {
        &["src", "dest"]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        if params.get("src").is_none() {
            return Err(ModuleError::MissingParameter("src".to_string()));
        }

        if params.get("dest").is_none() {
            return Err(ModuleError::MissingParameter("dest".to_string()));
        }

        // Validate format if provided
        if let Some(format_str) = params.get_string("format")? {
            ArchiveFormat::from_str(&format_str)?;
        }

        // Validate checksum format if provided
        if let Some(checksum) = params.get_string("checksum")? {
            let parts: Vec<&str> = checksum.splitn(2, ':').collect();
            if parts.len() != 2 {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid checksum format '{}'. Expected 'algorithm:hash'",
                    checksum
                )));
            }
            let algorithm = parts[0].to_lowercase();
            if !["sha256", "sha-256", "md5", "sha1", "sha-1"].contains(&algorithm.as_str()) {
                return Err(ModuleError::InvalidParameter(format!(
                    "Unsupported checksum algorithm '{}'. Use sha256, md5, or sha1",
                    algorithm
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
        let src_str = params.get_string_required("src")?;
        let dest_str = params.get_string_required("dest")?;
        let dest = Path::new(&dest_str);

        // Get options
        let remote_src = params.get_bool_or("remote_src", Self::is_url(&src_str));
        let creates = params.get_string("creates")?;
        let keep_newer = params.get_bool_or("keep_newer", false);
        let list_files = params.get_bool_or("list_files", false);
        let checksum = params.get_string("checksum")?;

        // Get patterns
        let exclude_patterns: Vec<String> = params.get_vec_string("exclude")?.unwrap_or_default();
        let include_patterns: Vec<String> = params.get_vec_string("include")?.unwrap_or_default();

        // Check creates condition - if path exists, skip extraction
        if let Some(creates_path) = &creates {
            if Path::new(creates_path).exists() {
                return Ok(ModuleOutput::ok(format!(
                    "Skipped extraction - '{}' already exists",
                    creates_path
                )));
            }
        }

        // Determine source file path
        let temp_dir = tempfile::TempDir::new()?;
        let src_path = if remote_src || Self::is_url(&src_str) {
            // Download the file
            let filename = src_str.rsplit('/').next().unwrap_or("archive.download");
            let download_path = temp_dir.path().join(filename);

            if context.check_mode {
                return Ok(ModuleOutput::changed(format!(
                    "Would download '{}' and extract to '{}'",
                    src_str, dest_str
                )));
            }

            Self::download_file(&src_str, &download_path)?;
            download_path
        } else {
            // Local file
            let path = Path::new(&src_str);
            if !path.exists() {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Source archive '{}' does not exist",
                    src_str
                )));
            }
            path.to_path_buf()
        };

        // Verify checksum if provided
        if let Some(ref expected_checksum) = checksum {
            if !Self::verify_checksum(&src_path, expected_checksum)? {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Checksum verification failed for '{}'",
                    src_str
                )));
            }
        }

        // Compute archive checksum for idempotency tracking
        let archive_checksum = Self::compute_checksum(&src_path, "sha256")?;

        // Check if already extracted
        let force = params.get_bool_or("force", false);
        if !force && Self::check_marker(dest, &archive_checksum) {
            return Ok(ModuleOutput::ok(format!(
                "Archive '{}' already extracted to '{}'",
                src_str, dest_str
            )));
        }

        // Determine format
        let format = if let Some(format_str) = params.get_string("format")? {
            ArchiveFormat::from_str(&format_str)?
        } else {
            // Try to detect from path
            ArchiveFormat::from_path(&src_path)
                .or_else(|| {
                    // Try magic bytes
                    if let Ok(mut file) = File::open(&src_path) {
                        let mut buffer = [0u8; 512];
                        if file.read(&mut buffer).is_ok() {
                            return ArchiveFormat::from_magic_bytes(&buffer);
                        }
                    }
                    None
                })
                .ok_or_else(|| {
                    ModuleError::InvalidParameter(
                        "Cannot determine archive format. Specify 'format' parameter.".to_string(),
                    )
                })?
        };

        // Check mode
        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would extract {:?} archive '{}' to '{}'",
                format, src_str, dest_str
            ))
            .with_diff(Diff::new(
                format!("dest: {}", if dest.exists() { "exists" } else { "absent" }),
                format!("dest: extracted from {}", src_str),
            )));
        }

        // Extract the archive
        let stats = match format {
            ArchiveFormat::Tar => Self::extract_tar(
                &src_path,
                dest,
                false,
                &exclude_patterns,
                &include_patterns,
                keep_newer,
            )?,
            ArchiveFormat::TarGz => Self::extract_tar(
                &src_path,
                dest,
                true,
                &exclude_patterns,
                &include_patterns,
                keep_newer,
            )?,
            ArchiveFormat::Zip => Self::extract_zip(
                &src_path,
                dest,
                &exclude_patterns,
                &include_patterns,
                keep_newer,
            )?,
        };

        // Create marker for idempotency
        Self::create_marker(dest, &archive_checksum)?;

        let mut output = ModuleOutput::changed(format!(
            "Extracted {:?} archive '{}' to '{}' ({} files, {} skipped)",
            format, src_str, dest_str, stats.extracted_count, stats.skipped_count
        ))
        .with_data("dest", serde_json::json!(dest_str))
        .with_data("src", serde_json::json!(src_str))
        .with_data("format", serde_json::json!(format!("{:?}", format)))
        .with_data("extracted_count", serde_json::json!(stats.extracted_count))
        .with_data("skipped_count", serde_json::json!(stats.skipped_count))
        .with_data("total_size", serde_json::json!(stats.total_size));

        if checksum.is_some() {
            output = output.with_data("checksum_verified", serde_json::json!(true));
        }

        if remote_src || Self::is_url(&src_str) {
            output = output.with_data("downloaded_from", serde_json::json!(src_str));
        }

        // List extracted files if requested
        if list_files {
            let mut files: Vec<String> = Vec::new();
            for entry in walkdir::WalkDir::new(dest)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if let Ok(relative) = entry.path().strip_prefix(dest) {
                    if !relative.as_os_str().is_empty() {
                        files.push(relative.to_string_lossy().to_string());
                    }
                }
            }
            output = output.with_data("files", serde_json::json!(files));
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn create_test_tar_gz(temp: &TempDir) -> std::path::PathBuf {
        let archive_path = temp.path().join("test.tar.gz");
        let source_dir = temp.path().join("source");
        fs::create_dir(&source_dir).unwrap();

        // Create test files
        fs::write(source_dir.join("file1.txt"), "Content 1").unwrap();
        fs::write(source_dir.join("file2.txt"), "Content 2").unwrap();
        let subdir = source_dir.join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("file3.txt"), "Content 3").unwrap();

        // Create the archive
        let file = File::create(&archive_path).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);

        builder
            .append_path_with_name(source_dir.join("file1.txt"), "file1.txt")
            .unwrap();
        builder
            .append_path_with_name(source_dir.join("file2.txt"), "file2.txt")
            .unwrap();
        builder.append_dir("subdir", &subdir).unwrap();
        builder
            .append_path_with_name(subdir.join("file3.txt"), "subdir/file3.txt")
            .unwrap();
        builder.finish().unwrap();

        archive_path
    }

    fn create_test_zip(temp: &TempDir) -> std::path::PathBuf {
        use zip::write::SimpleFileOptions;

        let archive_path = temp.path().join("test.zip");
        let file = File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);

        let options = SimpleFileOptions::default();

        zip.start_file("file1.txt", options).unwrap();
        zip.write_all(b"Content 1").unwrap();

        zip.start_file("file2.txt", options).unwrap();
        zip.write_all(b"Content 2").unwrap();

        zip.add_directory("subdir/", options).unwrap();

        zip.start_file("subdir/file3.txt", options).unwrap();
        zip.write_all(b"Content 3").unwrap();

        zip.finish().unwrap();

        archive_path
    }

    #[test]
    fn test_unarchive_module_name() {
        let module = UnarchiveModule;
        assert_eq!(module.name(), "unarchive");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_is_url() {
        assert!(UnarchiveModule::is_url("http://example.com/file.tar.gz"));
        assert!(UnarchiveModule::is_url("https://example.com/file.zip"));
        assert!(UnarchiveModule::is_url("ftp://example.com/file.tar"));
        assert!(!UnarchiveModule::is_url("/path/to/file.tar.gz"));
        assert!(!UnarchiveModule::is_url("relative/path.zip"));
    }

    #[test]
    fn test_format_detection() {
        assert_eq!(
            ArchiveFormat::from_path(Path::new("test.tar.gz")),
            Some(ArchiveFormat::TarGz)
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("test.tgz")),
            Some(ArchiveFormat::TarGz)
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("test.tar")),
            Some(ArchiveFormat::Tar)
        );
        assert_eq!(
            ArchiveFormat::from_path(Path::new("test.zip")),
            Some(ArchiveFormat::Zip)
        );
    }

    #[test]
    fn test_extract_tar_gz() {
        let temp = TempDir::new().unwrap();
        let archive = create_test_tar_gz(&temp);
        let dest = temp.path().join("extracted");

        let module = UnarchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!(archive.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(dest.exists());
        assert!(dest.join("file1.txt").exists());
        assert!(dest.join("file2.txt").exists());
        assert!(dest.join("subdir").join("file3.txt").exists());
    }

    #[test]
    fn test_extract_zip() {
        let temp = TempDir::new().unwrap();
        let archive = create_test_zip(&temp);
        let dest = temp.path().join("extracted");

        let module = UnarchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!(archive.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(dest.exists());
        assert!(dest.join("file1.txt").exists());
    }

    #[test]
    fn test_unarchive_idempotent() {
        let temp = TempDir::new().unwrap();
        let archive = create_test_tar_gz(&temp);
        let dest = temp.path().join("extracted");

        let module = UnarchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!(archive.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default();

        // First extraction
        let result1 = module.execute(&params, &context).unwrap();
        assert!(result1.changed);

        // Second extraction - should be idempotent
        let result2 = module.execute(&params, &context).unwrap();
        assert!(!result2.changed);
        assert!(result2.msg.contains("already extracted"));
    }

    #[test]
    fn test_unarchive_with_exclusions() {
        let temp = TempDir::new().unwrap();
        let archive = create_test_tar_gz(&temp);
        let dest = temp.path().join("extracted");

        let module = UnarchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!(archive.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert("exclude".to_string(), serde_json::json!(["file1.txt"]));
        params.insert("force".to_string(), serde_json::json!(true));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        // file1.txt should be excluded
        assert!(!dest.join("file1.txt").exists());
        assert!(dest.join("file2.txt").exists());
    }

    #[test]
    fn test_unarchive_check_mode() {
        let temp = TempDir::new().unwrap();
        let archive = create_test_tar_gz(&temp);
        let dest = temp.path().join("extracted");

        let module = UnarchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!(archive.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would extract"));
        assert!(!dest.exists()); // Nothing extracted
    }

    #[test]
    fn test_unarchive_creates_condition() {
        let temp = TempDir::new().unwrap();
        let archive = create_test_tar_gz(&temp);
        let dest = temp.path().join("extracted");
        let marker = temp.path().join("marker");

        // Create marker file
        fs::write(&marker, "exists").unwrap();

        let module = UnarchiveModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "src".to_string(),
            serde_json::json!(archive.to_str().unwrap()),
        );
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );
        params.insert(
            "creates".to_string(),
            serde_json::json!(marker.to_str().unwrap()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Skipped"));
        assert!(!dest.exists()); // Nothing extracted
    }

    #[test]
    fn test_validate_params() {
        let module = UnarchiveModule;

        // Missing src
        let mut params: ModuleParams = HashMap::new();
        params.insert("dest".to_string(), serde_json::json!("/tmp/dest"));
        assert!(module.validate_params(&params).is_err());

        // Missing dest
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("/tmp/archive.tar.gz"));
        assert!(module.validate_params(&params).is_err());

        // Invalid checksum format
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("/tmp/archive.tar.gz"));
        params.insert("dest".to_string(), serde_json::json!("/tmp/dest"));
        params.insert("checksum".to_string(), serde_json::json!("invalid"));
        assert!(module.validate_params(&params).is_err());

        // Valid params
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!("/tmp/archive.tar.gz"));
        params.insert("dest".to_string(), serde_json::json!("/tmp/dest"));
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pattern_matching() {
        assert!(UnarchiveModule::pattern_matches("file.log", "*.log"));
        assert!(UnarchiveModule::pattern_matches("test.txt", "test*"));
        assert!(UnarchiveModule::pattern_matches("anything", "*"));
        assert!(UnarchiveModule::pattern_matches(
            "path/to/file.log",
            "*.log"
        ));
        assert!(!UnarchiveModule::pattern_matches("file.txt", "*.log"));
    }
}
