//! Path security utilities
//!
//! Provides functions to validate filesystem paths and prevent:
//! - Path traversal attacks (../)
//! - Symlink attacks
//! - Null byte injection
//! - Access to sensitive system paths

use super::{SecurityError, SecurityResult};
use std::path::{Component, Path, PathBuf};

/// Errors specific to path validation
#[derive(Debug, Clone, thiserror::Error)]
pub enum PathSecurityError {
    #[error("Path traversal detected: {0}")]
    Traversal(String),

    #[error("Path escapes base directory: {0}")]
    EscapesBase(String),

    #[error("Path contains null byte")]
    NullByte,

    #[error("Path is empty")]
    Empty,

    #[error("Absolute path not allowed: {0}")]
    AbsoluteNotAllowed(String),

    #[error("Access to sensitive path denied: {0}")]
    SensitivePath(String),
}

/// List of sensitive system paths that should be protected
const SENSITIVE_PATHS: &[&str] = &[
    "/etc/shadow",
    "/etc/sudoers",
    "/etc/passwd",
    "/etc/ssh/sshd_config",
    "/root/.ssh",
    "/proc",
    "/sys",
    "/dev",
];

/// Validate that a path does not contain traversal sequences.
///
/// This function rejects paths containing:
/// - Parent directory references (..)
/// - Null bytes
/// - Empty paths
///
/// # Arguments
///
/// * `path` - The path string to validate
///
/// # Returns
///
/// * `Ok(())` if the path is safe
/// * `Err(SecurityError::PathTraversal)` if traversal is detected
///
/// # Examples
///
/// ```
/// use rustible::security::validate_path_no_traversal;
///
/// assert!(validate_path_no_traversal("/var/log/app.log").is_ok());
/// assert!(validate_path_no_traversal("./config/app.yaml").is_ok());
/// assert!(validate_path_no_traversal("../../../etc/passwd").is_err());
/// assert!(validate_path_no_traversal("/var/../etc/passwd").is_err());
/// ```
pub fn validate_path_no_traversal(path: &str) -> SecurityResult<()> {
    // Reject empty paths
    if path.is_empty() {
        return Err(SecurityError::PathTraversal(
            "Path cannot be empty".to_string(),
        ));
    }

    // Reject paths with null bytes
    if path.contains('\0') {
        return Err(SecurityError::PathTraversal(
            "Path contains null byte".to_string(),
        ));
    }

    // Reject paths with newlines (log injection)
    if path.contains('\n') || path.contains('\r') {
        return Err(SecurityError::PathTraversal(
            "Path contains newline characters".to_string(),
        ));
    }

    // Parse and validate path components
    let path_obj = Path::new(path);
    for component in path_obj.components() {
        match component {
            Component::ParentDir => {
                return Err(SecurityError::PathTraversal(format!(
                    "Path '{}' contains parent directory reference (..)",
                    path
                )));
            }
            Component::Normal(s) => {
                // Check for encoded traversal attempts
                if let Some(s_str) = s.to_str() {
                    if s_str.contains("..") {
                        return Err(SecurityError::PathTraversal(format!(
                            "Path component '{}' contains traversal sequence",
                            s_str
                        )));
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Validate that a path stays within a base directory.
///
/// This function ensures that the resolved path is a descendant of the
/// base directory, preventing directory escape attacks.
///
/// # Arguments
///
/// * `base` - The base directory that the path must stay within
/// * `path` - The path to validate (can be absolute or relative)
///
/// # Returns
///
/// * `Ok(PathBuf)` - The canonicalized path if it's within the base
/// * `Err(SecurityError)` - If the path escapes the base directory
///
/// # Examples
///
/// ```
/// use rustible::security::validate_path_within_base;
/// use std::path::Path;
///
/// let base = Path::new("/var/app");
/// assert!(validate_path_within_base(base, "data/file.txt").is_ok());
/// assert!(validate_path_within_base(base, "../../../etc/passwd").is_err());
/// ```
pub fn validate_path_within_base(base: &Path, path: &str) -> SecurityResult<PathBuf> {
    // First, check for obvious traversal
    validate_path_no_traversal(path)?;

    // Resolve the path relative to base
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };

    // Normalize the path by resolving . and .. components
    let normalized = normalize_path(&resolved);

    // Get the canonical base path for comparison
    // Use normalized version if canonicalize fails (e.g., path doesn't exist yet)
    let canonical_base = base.canonicalize().unwrap_or_else(|_| normalize_path(base));

    // Check that the resolved path starts with the base
    if !normalized.starts_with(&canonical_base) {
        return Err(SecurityError::PathTraversal(format!(
            "Path '{}' escapes base directory '{}'",
            path,
            base.display()
        )));
    }

    Ok(normalized)
}

/// Normalize a path by resolving . and .. components without filesystem access.
///
/// This is useful for paths that may not exist yet.
fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => {
                normalized.push(other);
            }
        }
    }

    normalized
}

/// Check if a path refers to a sensitive system location.
///
/// # Arguments
///
/// * `path` - The path to check
///
/// # Returns
///
/// * `Ok(())` if the path is not sensitive
/// * `Err(SecurityError)` if the path is sensitive
pub fn check_sensitive_path(path: &str) -> SecurityResult<()> {
    let path_obj = Path::new(path);
    let normalized = normalize_path(path_obj);
    let path_str = normalized.to_string_lossy();

    for sensitive in SENSITIVE_PATHS {
        if path_str.starts_with(sensitive) || path_str == *sensitive {
            return Err(SecurityError::PathTraversal(format!(
                "Access to sensitive path '{}' is restricted",
                sensitive
            )));
        }
    }

    Ok(())
}

/// Validate a path parameter with strict security checks.
///
/// This is a comprehensive validation function that combines:
/// - Traversal prevention
/// - Null byte rejection
/// - Newline rejection
/// - Length limits
///
/// # Arguments
///
/// * `path` - The path string to validate
/// * `param_name` - Name of the parameter for error messages
/// * `max_length` - Maximum allowed path length
///
/// # Returns
///
/// * `Ok(())` if the path is valid
/// * `Err(SecurityError)` if validation fails
pub fn validate_path_strict(path: &str, param_name: &str, max_length: usize) -> SecurityResult<()> {
    // Check length
    if path.len() > max_length {
        return Err(SecurityError::InvalidInput(format!(
            "{} path exceeds maximum length of {} bytes",
            param_name, max_length
        )));
    }

    // Run traversal check
    validate_path_no_traversal(path)?;

    Ok(())
}

/// Sanitize a filename by removing dangerous characters.
///
/// This function removes or replaces characters that could be
/// problematic in filenames across different filesystems.
///
/// # Arguments
///
/// * `filename` - The filename to sanitize
///
/// # Returns
///
/// A sanitized version of the filename
pub fn sanitize_filename(filename: &str) -> String {
    filename
        .chars()
        .filter(|c| {
            c.is_ascii_alphanumeric()
                || *c == '_'
                || *c == '-'
                || *c == '.'
                || *c == ' '
        })
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path_no_traversal_valid() {
        assert!(validate_path_no_traversal("/var/log/app.log").is_ok());
        assert!(validate_path_no_traversal("./config/app.yaml").is_ok());
        assert!(validate_path_no_traversal("relative/path/file.txt").is_ok());
        assert!(validate_path_no_traversal("/absolute/path").is_ok());
    }

    #[test]
    fn test_validate_path_no_traversal_invalid() {
        // Direct traversal
        assert!(validate_path_no_traversal("../etc/passwd").is_err());
        assert!(validate_path_no_traversal("../../root").is_err());

        // Traversal in middle of path
        assert!(validate_path_no_traversal("/var/../etc/passwd").is_err());
        assert!(validate_path_no_traversal("./subdir/../../../etc/passwd").is_err());

        // Null byte injection
        assert!(validate_path_no_traversal("/var/log\0/etc/passwd").is_err());

        // Newline injection
        assert!(validate_path_no_traversal("/var/log\n/etc/passwd").is_err());

        // Empty path
        assert!(validate_path_no_traversal("").is_err());
    }

    #[test]
    fn test_validate_path_within_base() {
        let base = Path::new("/var/app");

        // Valid paths
        assert!(validate_path_within_base(base, "data/file.txt").is_ok());
        assert!(validate_path_within_base(base, "config/app.yaml").is_ok());

        // Traversal attempts should be caught by validate_path_no_traversal first
        assert!(validate_path_within_base(base, "../../../etc/passwd").is_err());
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(
            normalize_path(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            normalize_path(Path::new("/a/./b/./c")),
            PathBuf::from("/a/b/c")
        );
        assert_eq!(
            normalize_path(Path::new("/a/b/../../c")),
            PathBuf::from("/c")
        );
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("file name.txt"), "file name.txt");
        assert_eq!(sanitize_filename("file;rm -rf /"), "filerm -rf");
        assert_eq!(sanitize_filename("../../etc/passwd"), "....etcpasswd");
        assert_eq!(sanitize_filename("file\0name"), "filename");
    }

    #[test]
    fn test_check_sensitive_path() {
        assert!(check_sensitive_path("/etc/shadow").is_err());
        assert!(check_sensitive_path("/etc/sudoers").is_err());
        assert!(check_sensitive_path("/root/.ssh/id_rsa").is_err());
        assert!(check_sensitive_path("/var/log/app.log").is_ok());
        assert!(check_sensitive_path("/home/user/file.txt").is_ok());
    }
}
