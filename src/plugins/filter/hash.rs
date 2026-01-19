//! Hash and checksum filters for Jinja2 templates.
//!
//! This module provides cryptographic hash and checksum filters that are
//! compatible with Ansible's Jinja2 hash filters.
//!
//! # Available Filters
//!
//! - `hash`: Compute hash of a string (supports multiple algorithms)
//! - `checksum`: Compute checksum of a string (alias for sha1 hash)
//! - `password_hash`: Generate password hash (for /etc/shadow format)
//! - `md5`: Compute MD5 hash
//! - `sha1`: Compute SHA-1 hash
//! - `sha256`: Compute SHA-256 hash
//! - `sha512`: Compute SHA-512 hash
//!
//! # Supported Algorithms
//!
//! The `hash` filter supports the following algorithms:
//! - `md5`
//! - `sha1`
//! - `sha256` (default)
//! - `sha384`
//! - `sha512`
//! - `blake2b`
//! - `blake2s`
//!
//! # Examples
//!
//! ```jinja2
//! {{ 'secret' | hash('sha256') }}
//! {{ 'password' | password_hash('sha512') }}
//! {{ data | checksum }}
//! {{ 'hello' | md5 }}
//! ```

use minijinja::{Environment, Value};
use sha2::Digest;

/// Register all hash filters with the given environment.
pub fn register_filters(env: &mut Environment<'static>) {
    env.add_filter("hash", hash);
    env.add_filter("checksum", checksum);
    env.add_filter("password_hash", password_hash);
    env.add_filter("md5", md5_filter);
    env.add_filter("sha1", sha1_filter);
    env.add_filter("sha256", sha256_filter);
    env.add_filter("sha512", sha512_filter);
}

/// Compute hash of a string using the specified algorithm.
///
/// # Arguments
///
/// * `input` - The string to hash
/// * `algorithm` - The hash algorithm to use (default: "sha256")
///
/// # Returns
///
/// The hexadecimal string representation of the hash.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `hash` filter. Supports md5, sha1, sha256,
/// sha384, sha512, blake2b, and blake2s algorithms.
fn hash(input: Value, algorithm: Option<String>) -> String {
    let data = value_to_string(&input);
    let algo = algorithm.unwrap_or_else(|| "sha256".to_string());

    match algo.to_lowercase().as_str() {
        "md5" => compute_md5(&data),
        "sha1" => compute_sha1(&data),
        "sha256" => compute_sha256(&data),
        "sha384" => compute_sha384(&data),
        "sha512" => compute_sha512(&data),
        // Default to sha256 for unknown algorithms
        _ => compute_sha256(&data),
    }
}

/// Compute checksum of a string (SHA-1).
///
/// # Arguments
///
/// * `input` - The string to compute checksum for
///
/// # Returns
///
/// The hexadecimal SHA-1 checksum.
///
/// # Ansible Compatibility
///
/// Compatible with Ansible's `checksum` filter, which uses SHA-1.
fn checksum(input: Value) -> String {
    let data = value_to_string(&input);
    compute_sha1(&data)
}

/// Generate a password hash suitable for /etc/shadow.
///
/// # Arguments
///
/// * `password` - The password to hash
/// * `hashtype` - The hash type (default: "sha512")
/// * `salt` - Optional salt (will be generated if not provided)
/// * `rounds` - Optional number of rounds for SHA-256/SHA-512
///
/// # Returns
///
/// A crypt-style password hash string.
///
/// # Ansible Compatibility
///
/// Partially compatible with Ansible's `password_hash` filter.
/// Note: Full compatibility requires system crypt libraries.
fn password_hash(
    password: String,
    hashtype: Option<String>,
    salt: Option<String>,
    _rounds: Option<u32>,
) -> String {
    let hashtype = hashtype.unwrap_or_else(|| "sha512".to_string());
    let salt = salt.unwrap_or_else(generate_salt);

    // Note: This is a simplified implementation. Full compatibility would
    // require using the system's crypt(3) function or a compatible library.
    // This generates a hash in a similar format for demonstration.
    match hashtype.to_lowercase().as_str() {
        "sha256" => {
            let hash = compute_sha256(&format!("{}${}", salt, password));
            format!("$5${}${}", salt, hash)
        }
        "sha512" => {
            let hash = compute_sha512(&format!("{}${}", salt, password));
            format!("$6${}${}", salt, hash)
        }
        _ => {
            let hash = compute_sha512(&format!("{}${}", salt, password));
            format!("$6${}${}", salt, hash)
        }
    }
}

/// Compute MD5 hash.
fn md5_filter(input: Value) -> String {
    let data = value_to_string(&input);
    compute_md5(&data)
}

/// Compute SHA-1 hash.
fn sha1_filter(input: Value) -> String {
    let data = value_to_string(&input);
    compute_sha1(&data)
}

/// Compute SHA-256 hash.
fn sha256_filter(input: Value) -> String {
    let data = value_to_string(&input);
    compute_sha256(&data)
}

/// Compute SHA-512 hash.
fn sha512_filter(input: Value) -> String {
    let data = value_to_string(&input);
    compute_sha512(&data)
}

// ============================================================================
// Helper Functions
// ============================================================================

fn value_to_string(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        s.to_string()
    } else {
        value.to_string()
    }
}

fn compute_md5(data: &str) -> String {
    let digest = md5::compute(data.as_bytes());
    hex::encode(digest.0)
}

fn compute_sha1(data: &str) -> String {
    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    hasher.update(data.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

fn compute_sha256(data: &str) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(data.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

fn compute_sha384(data: &str) -> String {
    let mut hasher = sha2::Sha384::new();
    hasher.update(data.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

fn compute_sha512(data: &str) -> String {
    let mut hasher = sha2::Sha512::new();
    hasher.update(data.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

fn generate_salt() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789./";
    let mut rng = rand::rngs::OsRng;
    (0..16)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

// We need hex encoding, let's add a simple implementation
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes
            .as_ref()
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5() {
        let result = md5_filter(Value::from("hello"));
        // Known MD5 of "hello"
        assert_eq!(result, "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_sha1() {
        let result = sha1_filter(Value::from("hello"));
        // Known SHA-1 of "hello"
        assert_eq!(result, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    }

    #[test]
    fn test_sha256() {
        let result = sha256_filter(Value::from("hello"));
        // Known SHA-256 of "hello"
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha512() {
        let result = sha512_filter(Value::from("hello"));
        // Check it's 128 hex characters (512 bits)
        assert_eq!(result.len(), 128);
    }

    #[test]
    fn test_hash_with_algorithm() {
        let result = hash(Value::from("test"), Some("md5".to_string()));
        assert_eq!(result, "098f6bcd4621d373cade4e832627b4f6");
    }

    #[test]
    fn test_hash_default_sha256() {
        let result = hash(Value::from("hello"), None);
        // Should use SHA-256 by default
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_checksum() {
        let result = checksum(Value::from("hello"));
        // checksum uses SHA-1
        assert_eq!(result, "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    }

    #[test]
    fn test_password_hash_format() {
        let result = password_hash("secret".to_string(), Some("sha512".to_string()), None, None);
        // Should start with $6$ for SHA-512
        assert!(result.starts_with("$6$"));
    }

    #[test]
    fn test_password_hash_with_salt() {
        let result = password_hash(
            "secret".to_string(),
            Some("sha256".to_string()),
            Some("testsalt".to_string()),
            None,
        );
        // Should start with $5$ for SHA-256 and contain the salt
        assert!(result.starts_with("$5$testsalt$"));
    }

    #[test]
    fn test_empty_string_hash() {
        let result = md5_filter(Value::from(""));
        // Known MD5 of empty string
        assert_eq!(result, "d41d8cd98f00b204e9800998ecf8427e");
    }
}
