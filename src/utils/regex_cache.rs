//! Global Regex cache to prevent recompilation
//!
//! Compiling regular expressions is an expensive operation.
//! This module provides a thread-safe cache for compiled regexes.

use dashmap::DashMap;
use once_cell::sync::Lazy;
use regex::Regex;

/// Global regex cache
static REGEX_CACHE: Lazy<DashMap<String, Regex>> = Lazy::new(DashMap::new);

/// Maximum number of regexes to cache to prevent memory leaks
const MAX_CACHE_SIZE: usize = 1000;

/// Get a compiled regex from cache or compile it if missing.
///
/// This function is thread-safe and uses a global cache.
/// `regex::Regex` uses `Arc` internally, so cloning is cheap.
///
/// To prevent unbounded memory growth, the cache is cleared when it exceeds
/// `MAX_CACHE_SIZE`. This is a simple eviction policy that avoids the complexity
/// of an LRU cache while preventing OOM.
///
/// # Arguments
///
/// * `pattern` - The regex pattern string
///
/// # Returns
///
/// * `Result<Regex, regex::Error>` - The compiled regex or error
pub fn get_regex(pattern: &str) -> Result<Regex, regex::Error> {
    if let Some(re) = REGEX_CACHE.get(pattern) {
        return Ok(re.clone());
    }

    // Simple eviction policy: if cache is too big, clear it.
    // This is rare but safe.
    if REGEX_CACHE.len() >= MAX_CACHE_SIZE {
        REGEX_CACHE.clear();
    }

    let re = Regex::new(pattern)?;
    REGEX_CACHE.insert(pattern.to_string(), re.clone());
    Ok(re)
}
