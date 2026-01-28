//! Filesystem utility functions.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Write content to a file with secure permission handling.
///
/// This function uses `OpenOptions` to atomically set permissions when creating a new file
/// on Unix systems, preventing TOCTOU race conditions where a file might be created
/// with default permissions (e.g., world-readable) before being restricted.
///
/// # Arguments
///
/// * `path` - Path to the file
/// * `content` - Content to write
/// * `create` - Whether to create the file if it doesn't exist. If false and file doesn't exist, fails.
/// * `mode` - Optional file mode (permissions) to set (Unix only)
pub fn secure_write_file(
    path: &Path,
    content: &str,
    create: bool,
    mode: Option<u32>,
) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).truncate(true);

    if create {
        options.create(true);
    } else {
        options.create(false);
    }

    #[cfg(unix)]
    if let Some(m) = mode {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(m);
    }

    let mut file = options.open(path)?;
    file.write_all(content.as_bytes())?;

    // On Unix, OpenOptions.mode() only applies when a NEW file is created.
    // If the file already existed, we must explicitly set permissions to ensure they match.
    // If it was just created, this is redundant but harmless (and ensures consistency).
    #[cfg(unix)]
    if let Some(m) = mode {
        use std::os::unix::fs::PermissionsExt;
        let metadata = file.metadata()?;
        if (metadata.permissions().mode() & 0o7777) != m {
            let mut perms = metadata.permissions();
            perms.set_mode(m);
            file.set_permissions(perms)?;
        }
    }

    // On non-Unix systems, we can't easily set mode atomically or via OpenOptions,
    // but we can try to set it after writing if supported/needed.
    // For now, this is a no-op as Rust's std::fs::Permissions handles basic readonly/etc.
    // but not full octal modes on Windows.
    #[cfg(not(unix))]
    if let Some(_m) = mode {
        // Mode setting not fully supported on non-Unix platforms in this context
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_secure_write_file_create() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        let content = "hello world";

        secure_write_file(&path, content, true, None).unwrap();

        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn test_secure_write_file_no_create_fails() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        let content = "hello world";

        let result = secure_write_file(&path, content, false, None);
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn test_secure_write_file_overwrite() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        fs::write(&path, "old content").unwrap();

        secure_write_file(&path, "new content", true, None).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new content");
    }

    #[cfg(unix)]
    #[test]
    fn test_secure_write_file_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("secret.txt");
        let content = "secret";
        let mode = 0o600;

        secure_write_file(&path, content, true, Some(mode)).unwrap();

        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, mode);
    }

    #[cfg(unix)]
    #[test]
    fn test_secure_write_file_update_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().unwrap();
        let path = temp.path().join("secret.txt");
        let content = "secret";

        // Create with 0644
        secure_write_file(&path, content, true, Some(0o644)).unwrap();
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o644);

        // Update to 0600
        secure_write_file(&path, content, true, Some(0o600)).unwrap();
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o600);
    }
}
