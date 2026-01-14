//! Package module - Package management
//!
//! This module manages packages on the system using the appropriate package manager
//! (apt, dnf, yum, pacman, zypper, etc.).

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use std::collections::HashMap;
use std::process::Command;

/// Supported package managers
#[derive(Debug, Clone, PartialEq)]
pub enum PackageManager {
    Apt,
    Dnf,
    Yum,
    Pacman,
    Zypper,
    Apk,
    Brew,
}

impl PackageManager {
    pub fn detect() -> Option<Self> {
        // Check for package managers in order of preference
        let managers = [
            ("apt-get", PackageManager::Apt),
            ("dnf", PackageManager::Dnf),
            ("yum", PackageManager::Yum),
            ("pacman", PackageManager::Pacman),
            ("zypper", PackageManager::Zypper),
            ("apk", PackageManager::Apk),
            ("brew", PackageManager::Brew),
        ];

        for (cmd, manager) in managers {
            if Command::new("which")
                .arg(cmd)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return Some(manager);
            }
        }

        None
    }

    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "apt" | "apt-get" => Ok(PackageManager::Apt),
            "dnf" => Ok(PackageManager::Dnf),
            "yum" => Ok(PackageManager::Yum),
            "pacman" => Ok(PackageManager::Pacman),
            "zypper" => Ok(PackageManager::Zypper),
            "apk" => Ok(PackageManager::Apk),
            "brew" | "homebrew" => Ok(PackageManager::Brew),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Unknown package manager: {}",
                s
            ))),
        }
    }

    pub fn install_cmd(&self) -> Vec<&'static str> {
        match self {
            PackageManager::Apt => vec!["apt-get", "install", "-y"],
            PackageManager::Dnf => vec!["dnf", "install", "-y"],
            PackageManager::Yum => vec!["yum", "install", "-y"],
            PackageManager::Pacman => vec!["pacman", "-S", "--noconfirm"],
            PackageManager::Zypper => vec!["zypper", "install", "-y"],
            PackageManager::Apk => vec!["apk", "add"],
            PackageManager::Brew => vec!["brew", "install"],
        }
    }

    pub fn remove_cmd(&self) -> Vec<&'static str> {
        match self {
            PackageManager::Apt => vec!["apt-get", "remove", "-y"],
            PackageManager::Dnf => vec!["dnf", "remove", "-y"],
            PackageManager::Yum => vec!["yum", "remove", "-y"],
            PackageManager::Pacman => vec!["pacman", "-R", "--noconfirm"],
            PackageManager::Zypper => vec!["zypper", "remove", "-y"],
            PackageManager::Apk => vec!["apk", "del"],
            PackageManager::Brew => vec!["brew", "uninstall"],
        }
    }

    pub fn update_cmd(&self) -> Vec<&'static str> {
        match self {
            PackageManager::Apt => vec!["apt-get", "update"],
            PackageManager::Dnf => vec!["dnf", "makecache"],
            PackageManager::Yum => vec!["yum", "makecache"],
            PackageManager::Pacman => vec!["pacman", "-Sy"],
            PackageManager::Zypper => vec!["zypper", "refresh"],
            PackageManager::Apk => vec!["apk", "update"],
            PackageManager::Brew => vec!["brew", "update"],
        }
    }

    pub fn upgrade_cmd(&self) -> Vec<&'static str> {
        match self {
            PackageManager::Apt => vec!["apt-get", "upgrade", "-y"],
            PackageManager::Dnf => vec!["dnf", "upgrade", "-y"],
            PackageManager::Yum => vec!["yum", "upgrade", "-y"],
            PackageManager::Pacman => vec!["pacman", "-Su", "--noconfirm"],
            PackageManager::Zypper => vec!["zypper", "update", "-y"],
            PackageManager::Apk => vec!["apk", "upgrade"],
            PackageManager::Brew => vec!["brew", "upgrade"],
        }
    }

    pub fn is_installed(&self, package: &str) -> ModuleResult<bool> {
        let result = match self {
            PackageManager::Apt => Command::new("dpkg")
                .args(["-s", package])
                .output()
                .map(|o| o.status.success()),
            PackageManager::Dnf | PackageManager::Yum => Command::new("rpm")
                .args(["-q", package])
                .output()
                .map(|o| o.status.success()),
            PackageManager::Pacman => Command::new("pacman")
                .args(["-Q", package])
                .output()
                .map(|o| o.status.success()),
            PackageManager::Zypper => Command::new("rpm")
                .args(["-q", package])
                .output()
                .map(|o| o.status.success()),
            PackageManager::Apk => Command::new("apk")
                .args(["info", "-e", package])
                .output()
                .map(|o| o.status.success()),
            PackageManager::Brew => Command::new("brew")
                .args(["list", package])
                .output()
                .map(|o| o.status.success()),
        };

        result.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to check package status: {}", e))
        })
    }

    pub fn get_installed_version(&self, package: &str) -> ModuleResult<Option<String>> {
        let output = match self {
            PackageManager::Apt => Command::new("dpkg-query")
                .args(["-W", "-f=${Version}", package])
                .output(),
            PackageManager::Dnf | PackageManager::Yum | PackageManager::Zypper => {
                Command::new("rpm")
                    .args(["-q", "--qf", "%{VERSION}-%{RELEASE}", package])
                    .output()
            }
            PackageManager::Pacman => Command::new("pacman").args(["-Q", package]).output(),
            PackageManager::Apk => Command::new("apk").args(["version", package]).output(),
            PackageManager::Brew => Command::new("brew")
                .args(["info", "--json=v1", package])
                .output(),
        };

        match output {
            Ok(o) if o.status.success() => {
                let version = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if version.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(version))
                }
            }
            _ => Ok(None),
        }
    }
}

/// Desired state for a package
#[derive(Debug, Clone, PartialEq)]
pub enum PackageState {
    Present,
    Absent,
    Latest,
}

impl PackageState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "installed" => Ok(PackageState::Present),
            "absent" | "removed" => Ok(PackageState::Absent),
            "latest" => Ok(PackageState::Latest),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, latest",
                s
            ))),
        }
    }
}

/// Module for package management
#[derive(Default)]
pub struct PackageModule;

impl PackageModule {
    pub fn run_package_command(
        cmd: &[&str],
        packages: &[String],
    ) -> ModuleResult<(bool, String, String)> {
        if cmd.is_empty() {
            return Err(ModuleError::ExecutionFailed("Empty command".to_string()));
        }

        let mut command = Command::new(cmd[0]);
        if cmd.len() > 1 {
            command.args(&cmd[1..]);
        }
        command.args(packages);

        let output = command.output().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to execute package command: {}", e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok((output.status.success(), stdout, stderr))
    }
}

impl Module for PackageModule {
    fn name(&self) -> &'static str {
        "package"
    }

    fn description(&self) -> &'static str {
        "Manage packages using the system package manager"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Package managers use locks - only one can run per host at a time
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Get package manager
        let pkg_manager = if let Some(pm_str) = params.get_string("use")? {
            PackageManager::from_str(&pm_str)?
        } else {
            PackageManager::detect().ok_or_else(|| {
                ModuleError::ExecutionFailed("Could not detect package manager".to_string())
            })?
        };

        // Get packages - can be a single package or a list
        let packages: Vec<String> = if let Some(names) = params.get_vec_string("name")? {
            names
        } else {
            vec![params.get_string_required("name")?]
        };

        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = PackageState::from_str(&state_str)?;
        let update_cache = params.get_bool_or("update_cache", false);

        // Update cache if requested
        if update_cache {
            if context.check_mode {
                // In check mode, just note we would update
            } else {
                let update_cmd = pkg_manager.update_cmd();
                let mut cmd = Command::new(update_cmd[0]);
                if update_cmd.len() > 1 {
                    cmd.args(&update_cmd[1..]);
                }
                let _ = cmd.output(); // Ignore errors for cache update
            }
        }

        // Track what we'll do
        let mut to_install: Vec<String> = Vec::new();
        let mut to_remove: Vec<String> = Vec::new();
        let mut already_ok: Vec<String> = Vec::new();

        for package in &packages {
            let is_installed = pkg_manager.is_installed(package)?;

            match state {
                PackageState::Present => {
                    if is_installed {
                        already_ok.push(package.clone());
                    } else {
                        to_install.push(package.clone());
                    }
                }
                PackageState::Absent => {
                    if is_installed {
                        to_remove.push(package.clone());
                    } else {
                        already_ok.push(package.clone());
                    }
                }
                PackageState::Latest => {
                    // For 'latest', we always try to install/upgrade
                    to_install.push(package.clone());
                }
            }
        }

        // Check mode - return what would happen
        if context.check_mode {
            if to_install.is_empty() && to_remove.is_empty() {
                return Ok(ModuleOutput::ok(format!(
                    "All packages already in desired state: {}",
                    already_ok.join(", ")
                )));
            }

            let mut msg = String::new();
            if !to_install.is_empty() {
                msg.push_str(&format!("Would install: {}. ", to_install.join(", ")));
            }
            if !to_remove.is_empty() {
                msg.push_str(&format!("Would remove: {}. ", to_remove.join(", ")));
            }

            return Ok(ModuleOutput::changed(msg.trim().to_string()));
        }

        // Perform the actual operations
        let mut changed = false;
        let mut results: HashMap<String, String> = HashMap::new();

        if !to_install.is_empty() {
            let install_cmd = pkg_manager.install_cmd();
            let (success, stdout, stderr) = Self::run_package_command(&install_cmd, &to_install)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to install packages: {}",
                    if stderr.is_empty() { stdout } else { stderr }
                )));
            }

            changed = true;
            for pkg in &to_install {
                results.insert(pkg.clone(), "installed".to_string());
            }
        }

        if !to_remove.is_empty() {
            let remove_cmd = pkg_manager.remove_cmd();
            let (success, stdout, stderr) = Self::run_package_command(&remove_cmd, &to_remove)?;

            if !success {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Failed to remove packages: {}",
                    if stderr.is_empty() { stdout } else { stderr }
                )));
            }

            changed = true;
            for pkg in &to_remove {
                results.insert(pkg.clone(), "removed".to_string());
            }
        }

        for pkg in &already_ok {
            results.insert(pkg.clone(), "ok".to_string());
        }

        if changed {
            let mut msg = String::new();
            if !to_install.is_empty() {
                msg.push_str(&format!("Installed: {}. ", to_install.join(", ")));
            }
            if !to_remove.is_empty() {
                msg.push_str(&format!("Removed: {}. ", to_remove.join(", ")));
            }

            Ok(ModuleOutput::changed(msg.trim().to_string())
                .with_data("results", serde_json::json!(results)))
        } else {
            Ok(
                ModuleOutput::ok("All packages already in desired state".to_string())
                    .with_data("results", serde_json::json!(results)),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_state_from_str() {
        assert_eq!(
            PackageState::from_str("present").unwrap(),
            PackageState::Present
        );
        assert_eq!(
            PackageState::from_str("installed").unwrap(),
            PackageState::Present
        );
        assert_eq!(
            PackageState::from_str("absent").unwrap(),
            PackageState::Absent
        );
        assert_eq!(
            PackageState::from_str("removed").unwrap(),
            PackageState::Absent
        );
        assert_eq!(
            PackageState::from_str("latest").unwrap(),
            PackageState::Latest
        );
        assert!(PackageState::from_str("invalid").is_err());
    }

    #[test]
    fn test_package_manager_from_str() {
        assert_eq!(
            PackageManager::from_str("apt").unwrap(),
            PackageManager::Apt
        );
        assert_eq!(
            PackageManager::from_str("dnf").unwrap(),
            PackageManager::Dnf
        );
        assert_eq!(
            PackageManager::from_str("pacman").unwrap(),
            PackageManager::Pacman
        );
        assert!(PackageManager::from_str("invalid").is_err());
    }

    #[test]
    fn test_package_manager_commands() {
        let apt = PackageManager::Apt;
        assert_eq!(apt.install_cmd(), vec!["apt-get", "install", "-y"]);
        assert_eq!(apt.remove_cmd(), vec!["apt-get", "remove", "-y"]);

        let pacman = PackageManager::Pacman;
        assert_eq!(pacman.install_cmd(), vec!["pacman", "-S", "--noconfirm"]);
        assert_eq!(pacman.remove_cmd(), vec!["pacman", "-R", "--noconfirm"]);
    }

    // Integration tests would require actual package manager access
    // These are unit tests for the parsing/configuration logic
}
