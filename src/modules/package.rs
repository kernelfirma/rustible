//! Package module - Package management
//!
//! This module manages packages on the system using the appropriate package manager
//! (apt, dnf, yum, pacman, zypper, etc.).

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions};
use crate::utils::shell_escape;
use std::collections::HashMap;

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
    pub async fn detect_remote(
        conn: &(dyn Connection + Send + Sync),
        options: Option<ExecuteOptions>,
    ) -> Option<Self> {
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
            let which_cmd = format!("which {}", cmd);
            if let Ok(result) = conn.execute(&which_cmd, options.clone()).await {
                if result.success {
                    return Some(manager);
                }
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

    pub async fn is_installed_remote(
        &self,
        package: &str,
        conn: &(dyn Connection + Send + Sync),
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<bool> {
        let escaped_pkg = shell_escape(package);
        let cmd = match self {
            PackageManager::Apt => format!("dpkg -s {} 2>/dev/null", escaped_pkg),
            PackageManager::Dnf | PackageManager::Yum => {
                format!("rpm -q {} 2>/dev/null", escaped_pkg)
            }
            PackageManager::Pacman => format!("pacman -Q {} 2>/dev/null", escaped_pkg),
            PackageManager::Zypper => format!("rpm -q {} 2>/dev/null", escaped_pkg),
            PackageManager::Apk => format!("apk info -e {} 2>/dev/null", escaped_pkg),
            PackageManager::Brew => format!("brew list {} 2>/dev/null", escaped_pkg),
        };

        match conn.execute(&cmd, options).await {
            Ok(result) => Ok(result.success),
            Err(e) => Err(ModuleError::ExecutionFailed(format!(
                "Failed to check package status: {}",
                e
            ))),
        }
    }

    pub async fn get_installed_version_remote(
        &self,
        package: &str,
        conn: &(dyn Connection + Send + Sync),
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<Option<String>> {
        let escaped_pkg = shell_escape(package);
        let cmd = match self {
            PackageManager::Apt => {
                format!(
                    "dpkg-query -W -f='${{Version}}' {} 2>/dev/null",
                    escaped_pkg
                )
            }
            PackageManager::Dnf | PackageManager::Yum | PackageManager::Zypper => {
                format!(
                    "rpm -q --qf '%{{VERSION}}-%{{RELEASE}}' {} 2>/dev/null",
                    escaped_pkg
                )
            }
            PackageManager::Pacman => format!("pacman -Q {} 2>/dev/null", escaped_pkg),
            PackageManager::Apk => format!("apk version {} 2>/dev/null", escaped_pkg),
            PackageManager::Brew => {
                format!("brew info --json=v1 {} 2>/dev/null", escaped_pkg)
            }
        };

        match conn.execute(&cmd, options).await {
            Ok(result) if result.success => {
                let version = result.stdout.trim().to_string();
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

impl std::str::FromStr for PackageManager {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PackageManager::from_str(s)
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

impl std::str::FromStr for PackageState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PackageState::from_str(s)
    }
}

/// Module for package management
#[derive(Default)]
pub struct PackageModule;

impl PackageModule {
    /// Build execution options with become/sudo if needed
    fn build_exec_options(context: &ModuleContext) -> ExecuteOptions {
        let mut options = ExecuteOptions::new();

        if context.r#become {
            options.escalate = true;
            options.escalate_user = context
                .become_user
                .clone()
                .or_else(|| Some("root".to_string()));
            options.escalate_method = context.become_method.clone();
            if let Some(ref password) = context.become_password {
                options.escalate_password = Some(password.clone());
            }
        }

        if let Some(ref work_dir) = context.work_dir {
            options = options.with_cwd(work_dir);
        }

        options
    }

    /// Run a package command remotely via the connection
    async fn run_package_command_remote(
        cmd: &[&str],
        packages: &[String],
        conn: &(dyn Connection + Send + Sync),
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        if cmd.is_empty() {
            return Err(ModuleError::ExecutionFailed("Empty command".to_string()));
        }

        let escaped_packages: Vec<std::borrow::Cow<'_, str>> =
            packages.iter().map(|p| shell_escape(p)).collect();

        let cmd_string = format!("{} {}", cmd.join(" "), escaped_packages.join(" "));

        let result = conn.execute(&cmd_string, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to execute package command: {}", e))
        })?;

        Ok((result.success, result.stdout, result.stderr))
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
        // Get connection from context
        let conn = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available in context. Package module requires a remote connection."
                    .to_string(),
            )
        })?;

        // Build execution options with become/sudo
        let exec_options = Self::build_exec_options(context);

        // Use tokio runtime to execute async operations
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Get package manager
                let pkg_manager = if let Some(pm_str) = params.get_string("use")? {
                    PackageManager::from_str(&pm_str)?
                } else {
                    PackageManager::detect_remote(conn.as_ref(), Some(exec_options.clone()))
                        .await
                        .ok_or_else(|| {
                            ModuleError::ExecutionFailed(
                                "Could not detect package manager".to_string(),
                            )
                        })?
                };

                // Get packages - can be a single package or a list
                let packages: Vec<String> =
                    if let Some(names) = params.get_vec_string("name")? {
                        names
                    } else {
                        vec![params.get_string_required("name")?]
                    };

                let state_str = params
                    .get_string("state")?
                    .unwrap_or_else(|| "present".to_string());
                let state = PackageState::from_str(&state_str)?;
                let update_cache = params.get_bool_or("update_cache", false);

                let mut changed = false;
                let mut messages: Vec<String> = Vec::new();
                let mut all_stdout = String::new();
                let mut all_stderr = String::new();

                // Update cache if requested
                if update_cache {
                    if context.check_mode {
                        messages.push("Would update cache".to_string());
                    } else {
                        let update_cmd = pkg_manager.update_cmd();
                        let cmd_string = update_cmd.join(" ");
                        // Ignore errors for cache update (matches previous behavior)
                        let _ = conn
                            .execute(&cmd_string, Some(exec_options.clone()))
                            .await;
                    }
                }

                // Track what we'll do
                let mut to_install: Vec<String> = Vec::new();
                let mut to_remove: Vec<String> = Vec::new();
                let mut already_ok: Vec<String> = Vec::new();

                for package in &packages {
                    let is_installed = pkg_manager
                        .is_installed_remote(
                            package,
                            conn.as_ref(),
                            Some(exec_options.clone()),
                        )
                        .await?;

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
                        let mut msg = if !already_ok.is_empty() {
                            format!(
                                "All packages already in desired state: {}",
                                already_ok.join(", ")
                            )
                        } else {
                            String::new()
                        };
                        if !messages.is_empty() {
                            if !msg.is_empty() {
                                msg.push_str(". ");
                            }
                            msg.push_str(&messages.join(". "));
                        }
                        if messages.iter().any(|m| m.starts_with("Would")) {
                            return Ok(ModuleOutput::changed(msg));
                        }
                        return Ok(ModuleOutput::ok(msg));
                    }

                    if !to_install.is_empty() {
                        messages
                            .push(format!("Would install: {}", to_install.join(", ")));
                    }
                    if !to_remove.is_empty() {
                        messages
                            .push(format!("Would remove: {}", to_remove.join(", ")));
                    }

                    return Ok(ModuleOutput::changed(messages.join(". ")));
                }

                // Perform the actual operations
                let mut results: HashMap<String, String> = HashMap::new();

                if !to_install.is_empty() {
                    let install_cmd = pkg_manager.install_cmd();
                    let (success, stdout, stderr) =
                        Self::run_package_command_remote(
                            &install_cmd,
                            &to_install,
                            conn.as_ref(),
                            Some(exec_options.clone()),
                        )
                        .await?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to install packages: {}",
                            if stderr.is_empty() {
                                stdout
                            } else {
                                stderr
                            }
                        )));
                    }

                    changed = true;
                    for pkg in &to_install {
                        results.insert(pkg.clone(), "installed".to_string());
                    }
                    messages
                        .push(format!("Installed: {}", to_install.join(", ")));
                    all_stdout.push_str(&stdout);
                    all_stderr.push_str(&stderr);
                }

                if !to_remove.is_empty() {
                    let remove_cmd = pkg_manager.remove_cmd();
                    let (success, stdout, stderr) =
                        Self::run_package_command_remote(
                            &remove_cmd,
                            &to_remove,
                            conn.as_ref(),
                            Some(exec_options.clone()),
                        )
                        .await?;

                    if !success {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to remove packages: {}",
                            if stderr.is_empty() {
                                stdout
                            } else {
                                stderr
                            }
                        )));
                    }

                    changed = true;
                    for pkg in &to_remove {
                        results.insert(pkg.clone(), "removed".to_string());
                    }
                    messages.push(format!("Removed: {}", to_remove.join(", ")));
                    all_stdout.push_str(&stdout);
                    all_stderr.push_str(&stderr);
                }

                for pkg in &already_ok {
                    results.insert(pkg.clone(), "ok".to_string());
                }

                // Build final output
                let msg = if messages.is_empty() {
                    "All packages already in desired state".to_string()
                } else {
                    messages.join(". ")
                };

                let mut output = if changed {
                    ModuleOutput::changed(msg)
                } else {
                    ModuleOutput::ok(msg)
                };

                output = output.with_data("results", serde_json::json!(results));

                // Add stdout/stderr if present
                if !all_stdout.is_empty() || !all_stderr.is_empty() {
                    output = output.with_command_output(
                        if all_stdout.is_empty() {
                            None
                        } else {
                            Some(all_stdout)
                        },
                        if all_stderr.is_empty() {
                            None
                        } else {
                            Some(all_stderr)
                        },
                        Some(0),
                    );
                }

                Ok(output)
            })
        });

        result
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
