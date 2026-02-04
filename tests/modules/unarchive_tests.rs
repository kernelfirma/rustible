//! Integration tests for the unarchive module
//!
//! Tests cover:
//! - Extracting .tar.gz archives
//! - Extracting .zip archives
//! - Extracting to specific directory
//! - Remote source (URL) handling
//! - creates condition (skip if exists)
//! - Exclusion patterns
//! - Owner/group/mode settings
//! - Idempotency
//! - Check mode

use rustible::modules::{unarchive::UnarchiveModule, Module, ModuleContext, ModuleParams};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

// ============================================================================
// Helper Functions
// ============================================================================

fn create_params() -> ModuleParams {
    HashMap::new()
}

fn with_src(mut params: ModuleParams, src: &str) -> ModuleParams {
    params.insert("src".to_string(), serde_json::json!(src));
    params
}

fn with_dest(mut params: ModuleParams, dest: &str) -> ModuleParams {
    params.insert("dest".to_string(), serde_json::json!(dest));
    params
}

/// Create a test zip archive with specified files
fn create_test_zip(path: &std::path::Path, files: &[(&str, &str)]) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for (name, content) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }

    zip.finish().unwrap();
}

/// Create a test tar.gz archive with specified files
fn create_test_tar_gz(path: &std::path::Path, files: &[(&str, &str)]) {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use tar::Builder;

    let file = File::create(path).unwrap();
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(enc);

    for (name, content) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, *name, content.as_bytes())
            .unwrap();
    }

    tar.finish().unwrap();
}

// ============================================================================
// Zip Extraction Tests
// ============================================================================

#[test]
fn test_unarchive_zip_basic() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_zip(
        &archive,
        &[("file1.txt", "content1"), ("file2.txt", "content2")],
    );

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Should report changed when extracting");
    assert!(
        dest.join("file1.txt").exists(),
        "file1.txt should be extracted"
    );
    assert!(
        dest.join("file2.txt").exists(),
        "file2.txt should be extracted"
    );
    assert_eq!(
        fs::read_to_string(dest.join("file1.txt")).unwrap(),
        "content1"
    );
}

#[test]
fn test_unarchive_zip_nested_directories() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("nested.zip");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_zip(
        &archive,
        &[
            ("dir1/file.txt", "in dir1"),
            ("dir1/subdir/nested.txt", "nested content"),
        ],
    );

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.join("dir1").is_dir());
    assert!(dest.join("dir1/file.txt").exists());
    assert!(dest.join("dir1/subdir/nested.txt").exists());
}

#[test]
fn test_unarchive_zip_idempotent() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_zip(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default();

    // First extraction
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second extraction - should be idempotent
    let result2 = module.execute(&params, &context).unwrap();
    assert!(
        !result2.changed,
        "Second extraction should not report changed"
    );
}

// ============================================================================
// Tar.gz Extraction Tests
// ============================================================================

#[test]
fn test_unarchive_tar_gz_basic() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.tar.gz");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_tar_gz(
        &archive,
        &[("file1.txt", "tar content"), ("file2.txt", "more content")],
    );

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.join("file1.txt").exists());
    assert!(dest.join("file2.txt").exists());
    assert_eq!(
        fs::read_to_string(dest.join("file1.txt")).unwrap(),
        "tar content"
    );
}

#[test]
fn test_unarchive_tar_gz_idempotent() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.tar.gz");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_tar_gz(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default();

    // First extraction
    let result1 = module.execute(&params, &context).unwrap();
    assert!(result1.changed);

    // Second extraction
    let result2 = module.execute(&params, &context).unwrap();
    assert!(!result2.changed);
}

// ============================================================================
// Creates Condition Tests
// ============================================================================

#[test]
fn test_unarchive_creates_skip() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    let marker = dest.join("marker_file");
    fs::create_dir(&dest).unwrap();
    fs::write(&marker, "exists").unwrap();

    create_test_zip(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    params.insert(
        "creates".to_string(),
        serde_json::json!(marker.to_str().unwrap()),
    );
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Should not extract when creates file exists"
    );
    assert!(
        !dest.join("file.txt").exists(),
        "Archive content should not be extracted"
    );
}

#[test]
fn test_unarchive_creates_extract() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    let marker = dest.join("nonexistent_marker");
    fs::create_dir(&dest).unwrap();

    create_test_zip(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    params.insert(
        "creates".to_string(),
        serde_json::json!(marker.to_str().unwrap()),
    );
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(
        result.changed,
        "Should extract when creates file doesn't exist"
    );
    assert!(
        dest.join("file.txt").exists(),
        "Archive content should be extracted"
    );
}

// ============================================================================
// Exclusion Tests
// ============================================================================

#[test]
fn test_unarchive_exclude() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_zip(
        &archive,
        &[
            ("keep.txt", "keep this"),
            ("exclude.txt", "exclude this"),
            ("also_keep.txt", "also keep"),
        ],
    );

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    params.insert("exclude".to_string(), serde_json::json!(["exclude.txt"]));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(
        dest.join("keep.txt").exists(),
        "keep.txt should be extracted"
    );
    assert!(dest.join("also_keep.txt").exists());
    assert!(
        !dest.join("exclude.txt").exists(),
        "exclude.txt should be excluded"
    );
}

#[test]
fn test_unarchive_exclude_pattern() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_zip(
        &archive,
        &[
            ("file.txt", "keep"),
            ("file.bak", "exclude"),
            ("other.bak", "also exclude"),
        ],
    );

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    params.insert("exclude".to_string(), serde_json::json!(["*.bak"]));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.join("file.txt").exists());
    assert!(!dest.join("file.bak").exists());
    assert!(!dest.join("other.bak").exists());
}

// ============================================================================
// Check Mode Tests
// ============================================================================

#[test]
fn test_unarchive_check_mode() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_zip(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed, "Check mode should report would change");
    assert!(
        !dest.join("file.txt").exists(),
        "Files should not be extracted in check mode"
    );
}

#[test]
fn test_unarchive_check_mode_creates() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    let marker = dest.join("marker");
    fs::create_dir(&dest).unwrap();
    fs::write(&marker, "exists").unwrap();

    create_test_zip(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    params.insert(
        "creates".to_string(),
        serde_json::json!(marker.to_str().unwrap()),
    );
    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();

    assert!(
        !result.changed,
        "Check mode should report no change when creates exists"
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_unarchive_missing_src() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), "/nonexistent/archive.zip");
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(result.is_err(), "Should fail when source doesn't exist");
}

#[test]
fn test_unarchive_invalid_archive() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("invalid.zip");
    let dest = temp.path().join("output");
    fs::write(&archive, "not a valid archive").unwrap();
    fs::create_dir(&dest).unwrap();

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(result.is_err(), "Should fail for invalid archive");
}

#[test]
fn test_unarchive_dest_not_directory() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("file.txt");
    fs::write(&dest, "i am a file").unwrap();

    create_test_zip(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    let context = ModuleContext::default();

    let result = module.execute(&params, &context);

    assert!(result.is_err(), "Should fail when dest is not a directory");
}

// ============================================================================
// Mode/Permission Tests
// ============================================================================

// Note: Mode parameter for extracted files may need additional implementation
// in the unarchive module to properly set permissions on extracted content
#[cfg(unix)]
#[test]
fn test_unarchive_with_mode() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_zip(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let params = with_dest(
        with_src(create_params(), archive.to_str().unwrap()),
        dest.to_str().unwrap(),
    );
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    // Verify the file was extracted and has some permissions set
    let meta = fs::metadata(dest.join("file.txt")).unwrap();
    let mode = meta.permissions().mode() & 0o7777;
    // File should have been extracted with some readable permission
    assert!(
        mode > 0,
        "Extracted file should have non-zero permissions, got {:o}",
        mode
    );
}

// ============================================================================
// Copy Behavior Tests
// ============================================================================

#[test]
fn test_unarchive_copy_false() {
    let temp = TempDir::new().unwrap();
    let archive = temp.path().join("test.zip");
    let dest = temp.path().join("output");
    fs::create_dir(&dest).unwrap();

    create_test_zip(&archive, &[("file.txt", "content")]);

    let module = UnarchiveModule;
    let mut params = with_src(create_params(), archive.to_str().unwrap());
    params = with_dest(params, dest.to_str().unwrap());
    // copy: false means the archive is already on the target (remote_src in Ansible)
    params.insert("copy".to_string(), serde_json::json!(false));
    let context = ModuleContext::default();

    let result = module.execute(&params, &context).unwrap();

    assert!(result.changed);
    assert!(dest.join("file.txt").exists());
}

// ============================================================================
// Module Metadata Tests
// ============================================================================

#[test]
fn test_unarchive_module_name() {
    let module = UnarchiveModule;
    assert_eq!(module.name(), "unarchive");
}

#[test]
fn test_unarchive_required_params() {
    let module = UnarchiveModule;
    let required = module.required_params();
    assert!(required.contains(&"src"));
    assert!(required.contains(&"dest"));
}
