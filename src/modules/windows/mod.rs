//! Windows-specific modules for Rustible.
//!
//! This module provides Windows-specific functionality for managing Windows systems:
//!
//! - **win_copy**: Copy files on Windows systems with ACL support
//! - **win_service**: Manage Windows services (start, stop, configure)
//! - **win_package**: Package management via Chocolatey and MSI
//! - **win_user**: Windows user and group management
//! - **win_feature**: Windows Features and optional components
//!
//! ## Platform Detection
//!
//! These modules are designed to work when Rustible connects to a Windows target.
//! They detect the Windows version and use appropriate APIs (PowerShell, WMI, etc.).
//!
//! ## Example Usage
//!
//! ```yaml
//! - name: Install packages via Chocolatey
//!   win_package:
//!     name: git
//!     provider: chocolatey
//!     state: present
//!
//! - name: Configure Windows service
//!   win_service:
//!     name: wuauserv
//!     state: started
//!     start_mode: auto
//!
//! - name: Enable Windows feature
//!   win_feature:
//!     name: IIS-WebServerRole
//!     state: present
//!     include_management_tools: true
//! ```

pub mod win_copy;
pub mod win_feature;
pub mod win_package;
pub mod win_service;
pub mod win_user;

pub use win_copy::WinCopyModule;
pub use win_feature::WinFeatureModule;
pub use win_package::WinPackageModule;
pub use win_service::WinServiceModule;
pub use win_user::WinUserModule;

use crate::connection::Connection;
use crate::modules::{ModuleError, ModuleResult};

// Re-export escaping functions from utils
pub use crate::utils::{powershell_escape, powershell_escape_double_quoted};

/// Validates a Windows path for safety.
///
/// Prevents common path injection attacks and validates basic path structure.
pub fn validate_windows_path(path: &str) -> ModuleResult<()> {
    if path.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Path cannot be empty".to_string(),
        ));
    }

    // Check for null bytes
    if path.contains('\0') {
        return Err(ModuleError::InvalidParameter(
            "Path contains invalid null byte".to_string(),
        ));
    }

    // Check for newlines (could be used for command injection)
    if path.contains('\n') || path.contains('\r') {
        return Err(ModuleError::InvalidParameter(
            "Path contains invalid newline characters".to_string(),
        ));
    }

    // Check for suspicious patterns that could indicate command injection
    let suspicious_patterns = ["$(", "`", ";", "|", "&", ">", "<"];
    for pattern in suspicious_patterns {
        if path.contains(pattern) {
            return Err(ModuleError::InvalidParameter(format!(
                "Path contains suspicious pattern: {}",
                pattern
            )));
        }
    }

    Ok(())
}

/// Validates a Windows service name.
pub fn validate_service_name(name: &str) -> ModuleResult<()> {
    if name.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Service name cannot be empty".to_string(),
        ));
    }

    // Service names should be alphanumeric with underscores and hyphens
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid service name '{}': must contain only alphanumeric characters, underscores, and hyphens",
            name
        )));
    }

    Ok(())
}

/// Validates a Windows username.
pub fn validate_windows_username(name: &str) -> ModuleResult<()> {
    if name.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Username cannot be empty".to_string(),
        ));
    }

    // Windows usernames have specific restrictions
    let invalid_chars = [
        '/', '\\', '[', ']', ':', ';', '|', '=', ',', '+', '*', '?', '<', '>',
    ];
    for c in invalid_chars {
        if name.contains(c) {
            return Err(ModuleError::InvalidParameter(format!(
                "Username '{}' contains invalid character '{}'",
                name, c
            )));
        }
    }

    // Username cannot be just dots or spaces
    if name.chars().all(|c| c == '.' || c == ' ') {
        return Err(ModuleError::InvalidParameter(format!(
            "Username '{}' cannot consist only of dots and spaces",
            name
        )));
    }

    Ok(())
}

/// Validates a Windows package name for Chocolatey or MSI.
pub fn validate_package_name(name: &str) -> ModuleResult<()> {
    if name.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Package name cannot be empty".to_string(),
        ));
    }

    // Package names should be safe - alphanumeric, dots, hyphens, underscores
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid package name '{}': must contain only alphanumeric characters, dots, hyphens, and underscores",
            name
        )));
    }

    Ok(())
}

/// Validates a Windows feature name.
pub fn validate_feature_name(name: &str) -> ModuleResult<()> {
    if name.is_empty() {
        return Err(ModuleError::InvalidParameter(
            "Feature name cannot be empty".to_string(),
        ));
    }

    // Feature names should be alphanumeric with hyphens
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return Err(ModuleError::InvalidParameter(format!(
            "Invalid feature name '{}': must contain only alphanumeric characters and hyphens",
            name
        )));
    }

    Ok(())
}

/// Helper to execute PowerShell command via connection.
pub async fn execute_powershell(
    connection: &dyn Connection,
    script: &str,
) -> ModuleResult<(bool, String, String)> {
    // Encode the script as base64 for safe transport
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        script
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect::<Vec<u8>>(),
    );

    let command = format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        encoded
    );

    let result = connection
        .execute(&command, None)
        .await
        .map_err(|e| ModuleError::ExecutionFailed(format!("PowerShell execution failed: {}", e)))?;

    Ok((result.success, result.stdout, result.stderr))
}

/// Helper to execute PowerShell command synchronously (for Module trait).
pub fn execute_powershell_sync(
    connection: &std::sync::Arc<dyn Connection + Send + Sync>,
    script: &str,
) -> ModuleResult<(bool, String, String)> {
    let handle = tokio::runtime::Handle::try_current()
        .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

    let connection = connection.clone();
    let script = script.to_string();

    std::thread::scope(|s| {
        s.spawn(|| {
            handle.block_on(async { execute_powershell(connection.as_ref(), &script).await })
        })
        .join()
        .unwrap()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_windows_path() {
        assert!(validate_windows_path("C:\\Users\\test").is_ok());
        assert!(validate_windows_path("D:\\Program Files\\App").is_ok());
        assert!(validate_windows_path("").is_err());
        assert!(validate_windows_path("path\0null").is_err());
        assert!(validate_windows_path("path\nnewline").is_err());
        assert!(validate_windows_path("$(evil)").is_err());
    }

    #[test]
    fn test_validate_service_name() {
        assert!(validate_service_name("wuauserv").is_ok());
        assert!(validate_service_name("Windows-Update").is_ok());
        assert!(validate_service_name("my_service").is_ok());
        assert!(validate_service_name("").is_err());
        assert!(validate_service_name("evil;rm").is_err());
    }

    #[test]
    fn test_validate_windows_username() {
        assert!(validate_windows_username("Administrator").is_ok());
        assert!(validate_windows_username("john.doe").is_ok());
        assert!(validate_windows_username("").is_err());
        assert!(validate_windows_username("user/name").is_err());
        assert!(validate_windows_username("...").is_err());
    }

    #[test]
    fn test_validate_package_name() {
        assert!(validate_package_name("git").is_ok());
        assert!(validate_package_name("visual-studio-code").is_ok());
        assert!(validate_package_name("python3.11").is_ok());
        assert!(validate_package_name("").is_err());
        assert!(validate_package_name("evil;cmd").is_err());
    }

    #[test]
    fn test_validate_feature_name() {
        assert!(validate_feature_name("IIS-WebServerRole").is_ok());
        assert!(validate_feature_name("NetFx4-AdvSrvs").is_ok());
        assert!(validate_feature_name("").is_err());
        assert!(validate_feature_name("evil;feature").is_err());
    }
}
