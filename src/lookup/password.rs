//! Password Lookup Plugin
//!
//! Generates random passwords with configurable complexity.
//! Similar to Ansible's `password` lookup plugin.
//!
//! # Usage
//!
//! ```yaml
//! # Generate a random 16-character password
//! password: "{{ lookup('password', 'length=16') }}"
//!
//! # Generate password with specific character sets
//! password: "{{ lookup('password', 'length=20', 'chars=ascii_letters,digits') }}"
//!
//! # Generate password and store to file
//! password: "{{ lookup('password', '/path/to/password_file', 'length=16') }}"
//! ```
//!
//! # Options
//!
//! - `length` (int): Password length (default: 20)
//! - `chars` (string): Character sets to use, comma-separated:
//!   - `ascii_letters`: a-zA-Z
//!   - `digits`: 0-9
//!   - `ascii_lowercase`: a-z
//!   - `ascii_uppercase`: A-Z
//!   - `punctuation`: Special characters
//!   - `hexdigits`: 0-9a-f
//!   - `alphanumeric`: a-zA-Z0-9 (default)
//! - `encrypt` (string): Encryption type for storing (not implemented yet)

use super::{Lookup, LookupContext, LookupError, LookupResult};
use rand::Rng;
use std::fs;
use std::path::PathBuf;

/// Character set constants
const ASCII_LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
const ASCII_UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const HEXDIGITS: &str = "0123456789abcdef";
const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

/// Default password length
const DEFAULT_LENGTH: usize = 20;

/// Password lookup plugin for generating random passwords
#[derive(Debug, Clone, Default)]
pub struct PasswordLookup;

impl PasswordLookup {
    /// Create a new PasswordLookup instance
    pub fn new() -> Self {
        Self
    }

    /// Get the character set for a given name
    fn get_charset(name: &str) -> Option<String> {
        match name.trim().to_lowercase().as_str() {
            "ascii_letters" => Some(format!("{}{}", ASCII_LOWERCASE, ASCII_UPPERCASE)),
            "ascii_lowercase" => Some(ASCII_LOWERCASE.to_string()),
            "ascii_uppercase" => Some(ASCII_UPPERCASE.to_string()),
            "digits" => Some(DIGITS.to_string()),
            "hexdigits" => Some(HEXDIGITS.to_string()),
            "punctuation" => Some(PUNCTUATION.to_string()),
            "alphanumeric" => {
                Some(format!("{}{}{}", ASCII_LOWERCASE, ASCII_UPPERCASE, DIGITS))
            }
            _ => None,
        }
    }

    /// Build the character set from a comma-separated list of set names
    fn build_charset(chars_spec: &str) -> LookupResult<String> {
        if chars_spec.is_empty() {
            // Default to alphanumeric
            return Ok(format!(
                "{}{}{}",
                ASCII_LOWERCASE, ASCII_UPPERCASE, DIGITS
            ));
        }

        let mut charset = String::new();

        for name in chars_spec.split(',') {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }

            // Check if it's a predefined set
            if let Some(chars) = Self::get_charset(name) {
                charset.push_str(&chars);
            } else {
                // Treat as literal characters
                charset.push_str(name);
            }
        }

        if charset.is_empty() {
            return Err(LookupError::InvalidArguments(
                "No valid character sets specified".to_string(),
            ));
        }

        // Remove duplicates while preserving order
        let mut seen = std::collections::HashSet::new();
        let unique: String = charset.chars().filter(|c| seen.insert(*c)).collect();

        Ok(unique)
    }

    /// Generate a random password
    fn generate_password(length: usize, charset: &str) -> LookupResult<String> {
        if charset.is_empty() {
            return Err(LookupError::InvalidArguments(
                "Character set cannot be empty".to_string(),
            ));
        }

        if length == 0 {
            return Err(LookupError::InvalidArguments(
                "Password length must be greater than 0".to_string(),
            ));
        }

        let chars: Vec<char> = charset.chars().collect();
        let mut rng = rand::thread_rng();

        let password: String = (0..length)
            .map(|_| {
                let idx = rng.gen_range(0..chars.len());
                chars[idx]
            })
            .collect();

        Ok(password)
    }

    /// Ensure password meets complexity requirements
    fn ensure_complexity(
        password: &str,
        charset: &str,
        has_lowercase: bool,
        has_uppercase: bool,
        has_digits: bool,
        has_special: bool,
    ) -> bool {
        let mut meets_lowercase = !has_lowercase;
        let mut meets_uppercase = !has_uppercase;
        let mut meets_digits = !has_digits;
        let mut meets_special = !has_special;

        for c in password.chars() {
            if has_lowercase && ASCII_LOWERCASE.contains(c) {
                meets_lowercase = true;
            }
            if has_uppercase && ASCII_UPPERCASE.contains(c) {
                meets_uppercase = true;
            }
            if has_digits && DIGITS.contains(c) {
                meets_digits = true;
            }
            if has_special && PUNCTUATION.contains(c) {
                meets_special = true;
            }
        }

        meets_lowercase && meets_uppercase && meets_digits && meets_special
    }

    /// Save password to file if a file path is provided
    fn save_to_file(&self, path: &PathBuf, password: &str) -> LookupResult<()> {
        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                LookupError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to create directory: {}", e),
                ))
            })?;
        }

        fs::write(path, password).map_err(|e| {
            LookupError::Io(std::io::Error::new(
                e.kind(),
                format!("Failed to write password file: {}", e),
            ))
        })?;

        // Set restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, perms)?;
        }

        Ok(())
    }

    /// Load existing password from file
    fn load_from_file(&self, path: &PathBuf) -> LookupResult<Option<String>> {
        match fs::read_to_string(path) {
            Ok(content) => Ok(Some(content.trim().to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(LookupError::Io(e)),
        }
    }
}

impl Lookup for PasswordLookup {
    fn name(&self) -> &'static str {
        "password"
    }

    fn description(&self) -> &'static str {
        "Generates random passwords with configurable complexity"
    }

    fn lookup(&self, args: &[&str], context: &LookupContext) -> LookupResult<Vec<String>> {
        // Parse options
        let options = self.parse_options(args);

        // Get password length
        let length: usize = options
            .get("length")
            .map(|s| {
                s.parse().map_err(|_| {
                    LookupError::InvalidArguments(format!("Invalid length value: {}", s))
                })
            })
            .transpose()?
            .unwrap_or(DEFAULT_LENGTH);

        // Get character sets
        let chars_spec = options
            .get("chars")
            .map(|s| s.as_str())
            .unwrap_or("alphanumeric");
        let charset = Self::build_charset(chars_spec)?;

        // Check if a file path was provided (first non-option argument)
        let file_path: Option<PathBuf> = args
            .iter()
            .find(|arg| !arg.contains('='))
            .map(|path| {
                if PathBuf::from(path).is_absolute() {
                    PathBuf::from(path)
                } else if let Some(ref base) = context.base_dir {
                    base.join(path)
                } else {
                    PathBuf::from(path)
                }
            });

        // If file exists and we're not regenerating, return existing password
        if let Some(ref path) = file_path {
            if let Some(existing) = self.load_from_file(path)? {
                return Ok(vec![existing]);
            }
        }

        // Generate new password
        let password = Self::generate_password(length, &charset)?;

        // Save to file if path provided
        if let Some(ref path) = file_path {
            self.save_to_file(path, &password)?;
        }

        Ok(vec![password])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_password_lookup_default() {
        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].len(), DEFAULT_LENGTH);
    }

    #[test]
    fn test_password_lookup_custom_length() {
        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["length=32"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values[0].len(), 32);
    }

    #[test]
    fn test_password_lookup_digits_only() {
        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["length=10", "chars=digits"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values[0].len(), 10);
        assert!(values[0].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_password_lookup_hexdigits() {
        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["length=16", "chars=hexdigits"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values[0].len(), 16);
        assert!(values[0]
            .chars()
            .all(|c| HEXDIGITS.contains(c)));
    }

    #[test]
    fn test_password_lookup_lowercase_only() {
        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["length=20", "chars=ascii_lowercase"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert!(values[0].chars().all(|c| c.is_ascii_lowercase()));
    }

    #[test]
    fn test_password_lookup_mixed_charsets() {
        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["length=50", "chars=ascii_letters,digits"], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values[0].len(), 50);
        assert!(values[0]
            .chars()
            .all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_password_lookup_uniqueness() {
        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result1 = lookup.lookup(&["length=20"], &context).unwrap();
        let result2 = lookup.lookup(&["length=20"], &context).unwrap();

        // Very unlikely to be the same
        assert_ne!(result1[0], result2[0]);
    }

    #[test]
    fn test_password_lookup_invalid_length() {
        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["length=invalid"], &context);
        assert!(matches!(result, Err(LookupError::InvalidArguments(_))));
    }

    #[test]
    fn test_password_lookup_save_to_file() {
        let temp_dir = tempdir().unwrap();
        let password_file = temp_dir.path().join("password.txt");

        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(
            &[password_file.to_str().unwrap(), "length=16"],
            &context,
        );
        assert!(result.is_ok());

        // Verify file was created
        assert!(password_file.exists());

        // Verify content matches
        let saved_content = fs::read_to_string(&password_file).unwrap();
        assert_eq!(saved_content.trim(), result.unwrap()[0]);
    }

    #[test]
    fn test_password_lookup_read_existing_file() {
        let temp_dir = tempdir().unwrap();
        let password_file = temp_dir.path().join("existing_password.txt");

        // Create a password file first
        fs::write(&password_file, "existing_password_123").unwrap();

        let lookup = PasswordLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[password_file.to_str().unwrap()], &context);
        assert!(result.is_ok());
        let values = result.unwrap();
        assert_eq!(values[0], "existing_password_123");
    }

    #[test]
    fn test_build_charset() {
        // Test alphanumeric
        let charset = PasswordLookup::build_charset("alphanumeric").unwrap();
        assert!(charset.contains('a'));
        assert!(charset.contains('A'));
        assert!(charset.contains('0'));
        assert!(!charset.contains('!'));

        // Test combined
        let charset = PasswordLookup::build_charset("digits,punctuation").unwrap();
        assert!(charset.contains('0'));
        assert!(charset.contains('!'));
        assert!(!charset.contains('a'));

        // Test empty
        let charset = PasswordLookup::build_charset("").unwrap();
        assert!(!charset.is_empty()); // Should default to alphanumeric
    }
}
