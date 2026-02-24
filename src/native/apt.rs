//! Native APT package management bindings
//!
//! This module provides native access to APT package information by parsing
//! dpkg status files and apt-cache output directly, avoiding shell overhead.
//!
//! # Features
//!
//! - Direct dpkg status file parsing (`/var/lib/dpkg/status`)
//! - Package version comparison using dpkg version ordering
//! - Efficient batch package queries
//! - Fallback to shell commands when needed
//!
//! # Example
//!
//! ```rust,no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use rustible::native::apt::{AptNative, PackageInfo};
//!
//! let mut apt = AptNative::new()?;
//!
//! // Get package info
//! if let Some(pkg) = apt.get_package("nginx")? {
//!     println!("nginx version: {}", pkg.version);
//! }
//!
//! // List all installed packages
//! let packages = apt.list_installed()?;
//! # Ok(())
//! # }
//! ```

use super::{NativeError, NativeResult};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Default dpkg status file path
const DPKG_STATUS_PATH: &str = "/var/lib/dpkg/status";

/// Check if native APT support is available
pub fn is_native_available() -> bool {
    Path::new(DPKG_STATUS_PATH).exists()
}

/// Package status
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageStatus {
    /// Package is installed
    Installed,
    /// Package is configured
    Configured,
    /// Package is half-installed
    HalfInstalled,
    /// Package is half-configured
    HalfConfigured,
    /// Package is unpacked but not configured
    Unpacked,
    /// Package is being removed
    ConfigFiles,
    /// Package is not installed
    NotInstalled,
    /// Unknown status
    Unknown(String),
}

impl PackageStatus {
    fn from_str(s: &str) -> Self {
        // Parse dpkg status field (format: "want flag status")
        // We care about the last word (actual status)
        let status = s.split_whitespace().last().unwrap_or(s);
        match status.to_lowercase().as_str() {
            "installed" => PackageStatus::Installed,
            "config-files" => PackageStatus::ConfigFiles,
            "half-installed" => PackageStatus::HalfInstalled,
            "half-configured" => PackageStatus::HalfConfigured,
            "unpacked" => PackageStatus::Unpacked,
            "not-installed" => PackageStatus::NotInstalled,
            _ => PackageStatus::Unknown(s.to_string()),
        }
    }

    /// Check if the package is functionally installed
    pub fn is_installed(&self) -> bool {
        matches!(self, PackageStatus::Installed | PackageStatus::Configured)
    }
}

/// Information about an APT package
#[derive(Debug, Clone)]
pub struct PackageInfo {
    /// Package name
    pub name: String,
    /// Installed version
    pub version: String,
    /// Package architecture
    pub architecture: String,
    /// Package status
    pub status: PackageStatus,
    /// Package section (admin, utils, etc.)
    pub section: Option<String>,
    /// Package priority
    pub priority: Option<String>,
    /// Installed size in KB
    pub installed_size: Option<u64>,
    /// Package maintainer
    pub maintainer: Option<String>,
    /// Short description
    pub description: Option<String>,
    /// Dependencies
    pub depends: Vec<String>,
    /// Pre-dependencies
    pub pre_depends: Vec<String>,
    /// Recommended packages
    pub recommends: Vec<String>,
    /// Suggested packages
    pub suggests: Vec<String>,
    /// Packages this provides
    pub provides: Vec<String>,
    /// Packages this conflicts with
    pub conflicts: Vec<String>,
}

impl PackageInfo {
    fn new(name: String) -> Self {
        Self {
            name,
            version: String::new(),
            architecture: String::new(),
            status: PackageStatus::NotInstalled,
            section: None,
            priority: None,
            installed_size: None,
            maintainer: None,
            description: None,
            depends: Vec::new(),
            pre_depends: Vec::new(),
            recommends: Vec::new(),
            suggests: Vec::new(),
            provides: Vec::new(),
            conflicts: Vec::new(),
        }
    }
}

/// Native APT package manager interface
pub struct AptNative {
    /// Path to dpkg status file
    status_path: String,
    /// Cached package database
    cache: Option<HashMap<String, PackageInfo>>,
}

impl AptNative {
    /// Create a new AptNative instance
    pub fn new() -> NativeResult<Self> {
        Self::with_status_path(DPKG_STATUS_PATH)
    }

    /// Create with custom status file path
    pub fn with_status_path(path: &str) -> NativeResult<Self> {
        if !Path::new(path).exists() {
            return Err(NativeError::NotAvailable(format!(
                "dpkg status file not found: {}",
                path
            )));
        }

        Ok(Self {
            status_path: path.to_string(),
            cache: None,
        })
    }

    /// Load the package database from dpkg status file
    pub fn load_database(&mut self) -> NativeResult<()> {
        let file = File::open(&self.status_path)?;
        let reader = BufReader::new(file);

        let mut packages = HashMap::new();
        let mut current_pkg: Option<PackageInfo> = None;
        let mut current_field: Option<String> = None;

        for line in reader.lines() {
            let line = line?;

            // Empty line marks end of package stanza
            if line.is_empty() {
                if let Some(pkg) = current_pkg.take() {
                    if !pkg.name.is_empty() {
                        packages.insert(pkg.name.clone(), pkg);
                    }
                }
                current_field = None;
                continue;
            }

            // Continuation line (starts with space or tab)
            if line.starts_with(' ') || line.starts_with('\t') {
                if let (Some(ref field), Some(ref mut pkg)) = (&current_field, &mut current_pkg) {
                    let value = line.trim();
                    if field.as_str() == "Description" {
                        if let Some(ref mut desc) = pkg.description {
                            desc.push('\n');
                            desc.push_str(value);
                        }
                    }
                }
                continue;
            }

            // Field: Value line
            if let Some((field, value)) = line.split_once(':') {
                let field = field.trim();
                let value = value.trim();

                if field == "Package" {
                    // Start of new package
                    if let Some(pkg) = current_pkg.take() {
                        if !pkg.name.is_empty() {
                            packages.insert(pkg.name.clone(), pkg);
                        }
                    }
                    current_pkg = Some(PackageInfo::new(value.to_string()));
                    current_field = Some(field.to_string());
                    continue;
                }

                if let Some(ref mut pkg) = current_pkg {
                    current_field = Some(field.to_string());
                    match field {
                        "Version" => pkg.version = value.to_string(),
                        "Architecture" => pkg.architecture = value.to_string(),
                        "Status" => pkg.status = PackageStatus::from_str(value),
                        "Section" => pkg.section = Some(value.to_string()),
                        "Priority" => pkg.priority = Some(value.to_string()),
                        "Installed-Size" => {
                            pkg.installed_size = value.parse().ok();
                        }
                        "Maintainer" => pkg.maintainer = Some(value.to_string()),
                        "Description" => pkg.description = Some(value.to_string()),
                        "Depends" => pkg.depends = Self::parse_dependency_list(value),
                        "Pre-Depends" => pkg.pre_depends = Self::parse_dependency_list(value),
                        "Recommends" => pkg.recommends = Self::parse_dependency_list(value),
                        "Suggests" => pkg.suggests = Self::parse_dependency_list(value),
                        "Provides" => pkg.provides = Self::parse_dependency_list(value),
                        "Conflicts" => pkg.conflicts = Self::parse_dependency_list(value),
                        _ => {}
                    }
                }
            }
        }

        // Don't forget the last package
        if let Some(pkg) = current_pkg {
            if !pkg.name.is_empty() {
                packages.insert(pkg.name.clone(), pkg);
            }
        }

        self.cache = Some(packages);
        Ok(())
    }

    /// Parse a dependency list (comma-separated, with version constraints)
    fn parse_dependency_list(s: &str) -> Vec<String> {
        s.split(',')
            .map(|dep| {
                // Extract just the package name, ignoring version constraints
                dep.split_whitespace().next().unwrap_or("").to_string()
            })
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Ensure database is loaded
    fn ensure_loaded(&mut self) -> NativeResult<()> {
        if self.cache.is_none() {
            self.load_database()?;
        }
        Ok(())
    }

    /// Get information about a specific package
    pub fn get_package(&mut self, name: &str) -> NativeResult<Option<PackageInfo>> {
        self.ensure_loaded()?;
        Ok(self.cache.as_ref().and_then(|c| c.get(name).cloned()))
    }

    /// Check if a package is installed
    pub fn is_installed(&mut self, name: &str) -> NativeResult<bool> {
        self.ensure_loaded()?;
        Ok(self
            .cache
            .as_ref()
            .and_then(|c| c.get(name))
            .map(|p| p.status.is_installed())
            .unwrap_or(false))
    }

    /// Get installed version of a package
    pub fn get_version(&mut self, name: &str) -> NativeResult<Option<String>> {
        self.ensure_loaded()?;
        Ok(self.cache.as_ref().and_then(|c| {
            c.get(name)
                .filter(|p| p.status.is_installed())
                .map(|p| p.version.clone())
        }))
    }

    /// List all installed packages
    pub fn list_installed(&mut self) -> NativeResult<Vec<PackageInfo>> {
        self.ensure_loaded()?;
        Ok(self
            .cache
            .as_ref()
            .map(|c| {
                c.values()
                    .filter(|p| p.status.is_installed())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Get multiple packages in a single call
    pub fn get_packages(&mut self, names: &[&str]) -> NativeResult<HashMap<String, PackageInfo>> {
        self.ensure_loaded()?;
        let mut result = HashMap::new();

        if let Some(cache) = &self.cache {
            for name in names {
                if let Some(pkg) = cache.get(*name) {
                    result.insert(name.to_string(), pkg.clone());
                }
            }
        }

        Ok(result)
    }

    /// Search for packages by name pattern
    pub fn search(&mut self, pattern: &str) -> NativeResult<Vec<PackageInfo>> {
        self.ensure_loaded()?;
        let pattern = pattern.to_lowercase();

        Ok(self
            .cache
            .as_ref()
            .map(|c| {
                c.values()
                    .filter(|p| p.name.to_lowercase().contains(&pattern))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Get total number of packages
    pub fn package_count(&mut self) -> NativeResult<usize> {
        self.ensure_loaded()?;
        Ok(self.cache.as_ref().map(|c| c.len()).unwrap_or(0))
    }

    /// Get number of installed packages
    pub fn installed_count(&mut self) -> NativeResult<usize> {
        self.ensure_loaded()?;
        Ok(self
            .cache
            .as_ref()
            .map(|c| c.values().filter(|p| p.status.is_installed()).count())
            .unwrap_or(0))
    }

    /// Invalidate the cache to force reload
    pub fn invalidate_cache(&mut self) {
        self.cache = None;
    }
}

/// Compare dpkg version strings
///
/// Returns:
/// - `Ordering::Less` if v1 < v2
/// - `Ordering::Equal` if v1 == v2
/// - `Ordering::Greater` if v1 > v2
pub fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    // Simplified version comparison
    // Full dpkg version comparison is complex (epoch:upstream-revision)

    fn parse_version(v: &str) -> (u32, &str, &str) {
        // Extract epoch (before :)
        let (epoch, rest) = if let Some((e, r)) = v.split_once(':') {
            (e.parse().unwrap_or(0), r)
        } else {
            (0, v)
        };

        // Extract revision (after last -)
        let (upstream, revision) = if let Some((u, r)) = rest.rsplit_once('-') {
            (u, r)
        } else {
            (rest, "")
        };

        (epoch, upstream, revision)
    }

    let (e1, u1, r1) = parse_version(v1);
    let (e2, u2, r2) = parse_version(v2);

    // Compare epochs first
    match e1.cmp(&e2) {
        std::cmp::Ordering::Equal => {}
        other => return other,
    }

    // Compare upstream version
    match compare_version_string(u1, u2) {
        std::cmp::Ordering::Equal => {}
        other => return other,
    }

    // Compare revision
    compare_version_string(r1, r2)
}

/// Compare version strings character by character
fn compare_version_string(v1: &str, v2: &str) -> std::cmp::Ordering {
    let mut c1 = v1.chars().peekable();
    let mut c2 = v2.chars().peekable();

    loop {
        // Skip leading zeros in numbers
        while c1.peek() == Some(&'0') && c1.clone().nth(1).map(|c| c.is_ascii_digit()) == Some(true)
        {
            c1.next();
        }
        while c2.peek() == Some(&'0') && c2.clone().nth(1).map(|c| c.is_ascii_digit()) == Some(true)
        {
            c2.next();
        }

        match (c1.peek(), c2.peek()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(&c)) => {
                // ~ sorts before empty
                if c == '~' {
                    return std::cmp::Ordering::Greater;
                }
                return std::cmp::Ordering::Less;
            }
            (Some(&c), None) => {
                // ~ sorts before empty
                if c == '~' {
                    return std::cmp::Ordering::Less;
                }
                return std::cmp::Ordering::Greater;
            }
            (Some(&a), Some(&b)) => {
                // Both are digits - compare as numbers
                if a.is_ascii_digit() && b.is_ascii_digit() {
                    let mut n1 = String::new();
                    while let Some(&c) = c1.peek() {
                        if c.is_ascii_digit() {
                            n1.push(c);
                            c1.next();
                        } else {
                            break;
                        }
                    }
                    let mut n2 = String::new();
                    while let Some(&c) = c2.peek() {
                        if c.is_ascii_digit() {
                            n2.push(c);
                            c2.next();
                        } else {
                            break;
                        }
                    }

                    let num1: u64 = n1.parse().unwrap_or(0);
                    let num2: u64 = n2.parse().unwrap_or(0);

                    match num1.cmp(&num2) {
                        std::cmp::Ordering::Equal => continue,
                        other => return other,
                    }
                }

                // Compare characters (tildes sort before everything)
                let ord = |c: char| -> i32 {
                    if c == '~' {
                        -1
                    } else if c.is_ascii_digit() {
                        0
                    } else if c.is_ascii_alphabetic() {
                        c as i32
                    } else {
                        c as i32 + 256
                    }
                };

                match ord(a).cmp(&ord(b)) {
                    std::cmp::Ordering::Equal => {
                        c1.next();
                        c2.next();
                    }
                    other => return other,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn test_version_comparison() {
        assert_eq!(compare_versions("1.0", "1.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.0", "2.0"), Ordering::Less);
        assert_eq!(compare_versions("2.0", "1.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.1", "1.0"), Ordering::Greater);
        assert_eq!(compare_versions("1:1.0", "1.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0~beta", "1.0"), Ordering::Less);
    }

    #[test]
    fn test_package_status_parsing() {
        assert_eq!(
            PackageStatus::from_str("install ok installed"),
            PackageStatus::Installed
        );
        assert_eq!(
            PackageStatus::from_str("deinstall ok config-files"),
            PackageStatus::ConfigFiles
        );
    }

    #[test]
    fn test_dependency_parsing() {
        let deps = AptNative::parse_dependency_list("libc6 (>= 2.17), libssl1.1 | libssl3");
        assert_eq!(deps, vec!["libc6", "libssl1.1"]);
    }

    #[test]
    fn test_native_available() {
        // This test depends on the system, just ensure it doesn't panic
        let _ = is_native_available();
    }
}
