//! File Lookup Plugin
//!
//! Reads file contents from the filesystem. Similar to Ansible's `file` lookup plugin.
//!
//! # Usage
//!
//! ```yaml
//! # Read a single file
//! content: "{{ lookup('file', '/etc/hostname') }}"
//!
//! # Read multiple files
//! contents: "{{ lookup('file', '/etc/hosts', '/etc/resolv.conf') }}"
//!
//! # With options
//! content: "{{ lookup('file', '/path/to/file', lstrip=true, rstrip=true) }}"
//! ```
//!
//! # Options
//!
//! - `lstrip` (bool): Strip leading whitespace from content
//! - `rstrip` (bool): Strip trailing whitespace from content
//! - `errors` (string): How to handle errors - 'strict' (default), 'warn', 'ignore'

use super::{Lookup, LookupContext, LookupError, LookupResult};
use std::fs;
use std::path::{Path, PathBuf};

/// File lookup plugin for reading file contents
#[derive(Debug, Clone, Default)]
pub struct FileLookup;

impl FileLookup {
    /// Create a new FileLookup instance
    pub fn new() -> Self {
        Self
    }

    /// Resolve a path relative to the base directory if provided
    fn resolve_path(&self, path: &str, context: &LookupContext) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else if let Some(ref base) = context.base_dir {
            base.join(&path)
        } else {
            path
        }
    }

    /// Validate that a path is safe to read
    fn validate_path(&self, path: &Path) -> LookupResult<()> {
        // Check for null bytes (path injection attack)
        if path.to_string_lossy().contains('\0') {
            return Err(LookupError::InvalidArguments(
                "Path contains null byte".to_string(),
            ));
        }

        // Check for path traversal attempts in a potentially dangerous way
        // We allow .. in general but log a warning for absolute paths with ..
        if path.is_absolute() && path.to_string_lossy().contains("..") {
            tracing::warn!(
                "File lookup with path traversal in absolute path: {}",
                path.display()
            );
        }

        Ok(())
    }
}

impl Lookup for FileLookup {
    fn name(&self) -> &'static str {
        "file"
    }

    fn description(&self) -> &'static str {
        "Reads file contents from the filesystem"
    }

    fn lookup(&self, args: &[&str], context: &LookupContext) -> LookupResult<Vec<String>> {
        if args.is_empty() {
            return Err(LookupError::MissingArgument(
                "file path required".to_string(),
            ));
        }

        // Parse options
        let options = self.parse_options(args);
        let lstrip = options
            .get("lstrip")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1" || v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false);
        let rstrip = options
            .get("rstrip")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1" || v.eq_ignore_ascii_case("yes"))
            .unwrap_or(false);
        let error_mode = options
            .get("errors")
            .map(|s| s.as_str())
            .unwrap_or("strict");

        let mut results = Vec::new();

        // Process each non-option argument as a file path
        for arg in args {
            // Skip option arguments
            if arg.contains('=') {
                continue;
            }

            let path = self.resolve_path(arg, context);

            // Validate path safety
            if let Err(e) = self.validate_path(&path) {
                match error_mode {
                    "ignore" => continue,
                    "warn" => {
                        tracing::warn!("File lookup path validation failed: {}", e);
                        continue;
                    }
                    _ => return Err(e),
                }
            }

            // Read the file
            match fs::read_to_string(&path) {
                Ok(mut file_content) => {
                    if lstrip {
                        file_content = file_content.trim_start().to_string();
                    }
                    if rstrip {
                        file_content = file_content.trim_end().to_string();
                    }
                    results.push(file_content);
                }
                Err(e) => {
                    match error_mode {
                        "ignore" => continue,
                        "warn" => {
                            tracing::warn!("Failed to read file '{}': {}", path.display(), e);
                            continue;
                        }
                        _ => {
                            if e.kind() == std::io::ErrorKind::NotFound {
                                return Err(LookupError::FileNotFound(path));
                            } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                                return Err(LookupError::PermissionDenied(path.display().to_string()));
                            }
                            return Err(LookupError::Io(e));
                        }
                    }
                }
            }
        }

        if results.is_empty() && context.fail_on_error {
            return Err(LookupError::Other(
                "No files could be read".to_string(),
            ));
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_file_lookup_basic() {
        // Create a temp file with content
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "Hello, World!").unwrap();

        let lookup = FileLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[temp.path().to_str().unwrap()], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].trim(), "Hello, World!");
    }

    #[test]
    fn test_file_lookup_with_rstrip() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "Hello, World!  \n\n").unwrap();

        let lookup = FileLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[temp.path().to_str().unwrap(), "rstrip=true"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values[0], "Hello, World!");
    }

    #[test]
    fn test_file_lookup_with_lstrip() {
        let mut temp = NamedTempFile::new().unwrap();
        write!(temp, "  \n\nHello, World!").unwrap();

        let lookup = FileLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[temp.path().to_str().unwrap(), "lstrip=true"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values[0], "Hello, World!");
    }

    #[test]
    fn test_file_lookup_multiple_files() {
        let mut temp1 = NamedTempFile::new().unwrap();
        let mut temp2 = NamedTempFile::new().unwrap();
        writeln!(temp1, "File 1").unwrap();
        writeln!(temp2, "File 2").unwrap();

        let lookup = FileLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(
            &[temp1.path().to_str().unwrap(), temp2.path().to_str().unwrap()],
            &context,
        );
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn test_file_lookup_not_found() {
        let lookup = FileLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["/nonexistent/path/to/file.txt"], &context);
        assert!(matches!(result, Err(LookupError::FileNotFound(_))));
    }

    #[test]
    fn test_file_lookup_not_found_ignore() {
        let lookup = FileLookup::new();
        let context = LookupContext::new().with_fail_on_error(false);

        let result = lookup.lookup(&["/nonexistent/path/to/file.txt", "errors=ignore"], &context);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_file_lookup_missing_path() {
        let lookup = FileLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    #[test]
    fn test_file_lookup_relative_path() {
        let temp = NamedTempFile::new().unwrap();
        let temp_dir = temp.path().parent().unwrap();
        let file_name = temp.path().file_name().unwrap().to_str().unwrap();

        let lookup = FileLookup::new();
        let context = LookupContext::new().with_base_dir(temp_dir);

        // Just verify it can resolve relative paths - the file might be empty but readable
        let result = lookup.lookup(&[file_name], &context);
        assert!(result.is_ok());
    }
}
