//! Native bindings for system operations
//!
//! This module provides native API bindings for common system operations,
//! reducing shell overhead and improving performance. Each submodule provides
//! native implementations with fallbacks to shell commands when native APIs
//! are unavailable.
//!
//! # Modules
//!
//! - [`apt`]: Native APT package management (Debian/Ubuntu)
//! - [`systemd`]: Native systemd service management via D-Bus
//! - [`users`]: Native user and group management
//!
//! # Design
//!
//! Each module follows a consistent pattern:
//!
//! 1. **Native API**: Direct system calls or D-Bus communication
//! 2. **Fallback**: Shell command execution when native API unavailable
//! 3. **Detection**: Runtime capability detection for choosing implementation
//!
//! # Example
//!
//! ```rust,no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use rustible::native::{apt, systemd, users};
//!
//! // Check if native apt is available
//! if apt::is_native_available() {
//!     let mut apt_native = apt::AptNative::new()?;
//!     let packages = apt_native.list_installed()?;
//! }
//!
//! // Use native systemd if available
//! if systemd::is_native_available() {
//!     let sd = systemd::SystemdNative::new()?;
//!     let status = sd.get_unit_status("nginx.service")?;
//! }
//!
//! // Native user lookup
//! let user = users::get_user_by_name("www-data")?;
//! # Ok(())
//! # }
//! ```

pub mod apt;
pub mod systemd;
pub mod users;

use thiserror::Error;

/// Errors from native operations
#[derive(Error, Debug)]
pub enum NativeError {
    /// Native API not available on this system
    #[error("Native API not available: {0}")]
    NotAvailable(String),

    /// Permission denied for operation
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Resource not found (user, group, package, service)
    #[error("Not found: {0}")]
    NotFound(String),

    /// Invalid argument or parameter
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// I/O error during operation
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Parse error (for config files, status output, etc.)
    #[error("Parse error: {0}")]
    Parse(String),

    /// D-Bus communication error
    #[error("D-Bus error: {0}")]
    DBus(String),

    /// Operation timed out
    #[error("Operation timed out: {0}")]
    Timeout(String),

    /// Generic error with message
    #[error("{0}")]
    Other(String),
}

/// Result type for native operations
pub type NativeResult<T> = Result<T, NativeError>;

/// Capability detection for native APIs
#[derive(Debug, Clone, Default)]
pub struct NativeCapabilities {
    /// APT native support (dpkg status parsing)
    pub apt: bool,
    /// Systemd D-Bus support
    pub systemd: bool,
    /// Native user/group support (libc)
    pub users: bool,
}

impl NativeCapabilities {
    /// Detect native capabilities on the current system
    pub fn detect() -> Self {
        Self {
            apt: apt::is_native_available(),
            systemd: systemd::is_native_available(),
            users: users::is_native_available(),
        }
    }

    /// Check if any native capability is available
    pub fn any_available(&self) -> bool {
        self.apt || self.systemd || self.users
    }

    /// Check if all native capabilities are available
    pub fn all_available(&self) -> bool {
        self.apt && self.systemd && self.users
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_capabilities_detect() {
        let caps = NativeCapabilities::detect();
        // Users should always be available on Unix
        #[cfg(unix)]
        assert!(caps.users);
    }

    #[test]
    fn test_native_error_display() {
        let err = NativeError::NotFound("user 'nobody'".to_string());
        assert!(err.to_string().contains("Not found"));
    }
}
