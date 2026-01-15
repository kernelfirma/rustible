//! Yum module - Package management for RHEL/CentOS/Fedora systems
//!
//! This module manages packages using the yum package manager.
//! It supports installing, removing, and upgrading packages.
//!
//! # Supported Features
//!
//! - Individual package installation/removal
//! - Package groups (use @group syntax)
//! - Repository management (enablerepo/disablerepo)
//! - Security and bugfix updates
//! - Alternate installation roots
//! - Release version specification

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::connection::ExecuteOptions;
use crate::utils::shell_escape;
use std::collections::HashMap;

/// YUM module configuration options
#[derive(Debug, Clone, Default)]
struct YumOptions {
    /// Repository to enable for this operation
    enablerepo: Option<String>,
    /// Repository to disable for this operation
    disablerepo: Option<String>,
    /// Disable GPG signature checking
    disable_gpg_check: bool,
    /// Only apply security updates
    security: bool,
    /// Only apply bugfix updates
    bugfix: bool,
    /// Packages to exclude from operations
    exclude: Option<String>,
    /// Alternate installation root
    installroot: Option<String>,
    /// Release version to use
    releasever: Option<String>,
}

/// Desired state for a package
#[derive(Debug, Clone, PartialEq)]
pub enum YumState {
    Present,
    Absent,
    Latest,
}

impl YumState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "installed" => Ok(YumState::Present),
            "absent" | "removed" => Ok(YumState::Absent),
            "latest" => Ok(YumState::Latest),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, latest",
                s
            ))),
        }
    }
}

/// Module for yum package management
pub struct YumModule;

impl YumModule {
    /// Check if a package is installed via remote connection
    async fn is_package_installed_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        package: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<bool> {
        let cmd = format!("rpm -q {}", shell_escape(package));
        match conn.execute(&cmd, options).await {
            Ok(result) => Ok(result.success),
            Err(_) => Ok(false),
        }
    }

    /// Get installed package version via remote connection
    async fn get_installed_version_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        package: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<Option<String>> {
        let cmd = format!(
            "rpm -q --qf '%{{VERSION}}-%{{RELEASE}}' {}",
            shell_escape(package)
        );
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

    /// Build YUM command arguments from options
    fn build_yum_args(base_args: &[&str], yum_options: &YumOptions) -> Vec<String> {
        let mut args: Vec<String> = base_args.iter().map(|s| s.to_string()).collect();

        if yum_options.disable_gpg_check {
            args.push("--nogpgcheck".to_string());
        }

        if let Some(ref repo) = yum_options.enablerepo {
            args.push(format!("--enablerepo={}", repo));
        }

        if let Some(ref repo) = yum_options.disablerepo {
            args.push(format!("--disablerepo={}", repo));
        }

        if yum_options.security {
            args.push("--security".to_string());
        }

        if yum_options.bugfix {
            args.push("--bugfix".to_string());
        }

        if let Some(ref exclude) = yum_options.exclude {
            args.push(format!("--exclude={}", exclude));
        }

        if let Some(ref installroot) = yum_options.installroot {
            args.push(format!("--installroot={}", installroot));
        }

        if let Some(ref releasever) = yum_options.releasever {
            args.push(format!("--releasever={}", releasever));
        }

        args
    }

    /// Check if a name represents a package group
    fn is_package_group(name: &str) -> bool {
        name.starts_with('@')
    }

    /// Execute yum command via remote connection
    async fn run_yum_command_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        args: &[String],
        packages: &[String],
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let mut cmd_parts: Vec<String> = vec!["yum".to_string()];
        cmd_parts.extend(args.iter().cloned());
        cmd_parts.extend(packages.iter().map(|s| shell_escape(s).into_owned()));

        let cmd = cmd_parts.join(" ");

        let result = conn
            .execute(&cmd, options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to execute yum: {}", e)))?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Check if a package group is installed
    async fn is_group_installed_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        group: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<bool> {
        // Remove @ prefix for group check
        let group_name = group.trim_start_matches('@');
        let cmd = format!(
            "yum group list installed 2>/dev/null | grep -qi {}",
            shell_escape(group_name)
        );
        match conn.execute(&cmd, options).await {
            Ok(result) => Ok(result.success),
            Err(_) => Ok(false),
        }
    }

    /// Get available version for a package (for state=latest diff)
    async fn get_available_version_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        package: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<Option<String>> {
        let cmd = format!(
            "yum info available {} 2>/dev/null | grep -i '^Version' | head -1 | awk '{{print $3}}'",
            shell_escape(package)
        );
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

    /// Update yum cache via remote connection
    async fn update_cache_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<()> {
        let cmd = "yum makecache";
        let result = conn
            .execute(cmd, options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to update cache: {}", e)))?;

        if result.success {
            Ok(())
        } else {
            Err(ModuleError::ExecutionFailed(format!(
                "Failed to update cache: {}",
                result.stderr
            )))
        }
    }

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
        }

        if let Some(ref work_dir) = context.work_dir {
            options = options.with_cwd(work_dir);
        }

        options
    }
}

impl Module for YumModule {
    fn name(&self) -> &'static str {
        "yum"
    }

    fn description(&self) -> &'static str {
        "Manage packages with the yum package manager"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Yum uses locks - only one can run per host at a time
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
        // Get packages - can be a single package or a list
        let packages: Vec<String> = if let Some(names) = params.get_vec_string("name")? {
            names
        } else {
            vec![params.get_string_required("name")?]
        };

        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = YumState::from_str(&state_str)?;
        let update_cache = params.get_bool_or("update_cache", false);

        // Build YUM options from parameters
        let yum_options = YumOptions {
            enablerepo: params.get_string("enablerepo")?,
            disablerepo: params.get_string("disablerepo")?,
            disable_gpg_check: params.get_bool_or("disable_gpg_check", false),
            security: params.get_bool_or("security", false),
            bugfix: params.get_bool_or("bugfix", false),
            exclude: params.get_string("exclude")?,
            installroot: params.get_string("installroot")?,
            releasever: params.get_string("releasever")?,
        };

        // Get connection from context
        let conn = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available in context. YUM module requires a remote connection."
                    .to_string(),
            )
        })?;

        // Build execution options with become/sudo
        let exec_options = Self::build_exec_options(context);

        // Use tokio runtime to execute async operations
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Update cache if requested
                if update_cache && !context.check_mode {
                    Self::update_cache_remote(conn.as_ref(), Some(exec_options.clone())).await?;
                }

                // Separate packages and groups
                let mut packages_to_check: Vec<String> = Vec::new();
                let mut groups_to_check: Vec<String> = Vec::new();

                for item in &packages {
                    if Self::is_package_group(item) {
                        groups_to_check.push(item.clone());
                    } else {
                        packages_to_check.push(item.clone());
                    }
                }

                // Track what we'll do
                let mut to_install: Vec<String> = Vec::new();
                let mut to_remove: Vec<String> = Vec::new();
                let mut already_ok: Vec<String> = Vec::new();

                // Check current state of packages
                for package in &packages_to_check {
                    let is_installed = Self::is_package_installed_remote(
                        conn.as_ref(),
                        package,
                        Some(exec_options.clone()),
                    )
                    .await?;

                    match state {
                        YumState::Present => {
                            if is_installed {
                                already_ok.push(package.clone());
                            } else {
                                to_install.push(package.clone());
                            }
                        }
                        YumState::Absent => {
                            if is_installed {
                                to_remove.push(package.clone());
                            } else {
                                already_ok.push(package.clone());
                            }
                        }
                        YumState::Latest => {
                            // For 'latest', we always try to install/upgrade
                            to_install.push(package.clone());
                        }
                    }
                }

                // Check current state of groups
                for group in &groups_to_check {
                    let is_installed = Self::is_group_installed_remote(
                        conn.as_ref(),
                        group,
                        Some(exec_options.clone()),
                    )
                    .await?;

                    match state {
                        YumState::Present => {
                            if is_installed {
                                already_ok.push(group.clone());
                            } else {
                                to_install.push(group.clone());
                            }
                        }
                        YumState::Absent => {
                            if is_installed {
                                to_remove.push(group.clone());
                            } else {
                                already_ok.push(group.clone());
                            }
                        }
                        YumState::Latest => {
                            // For groups, latest is same as present (upgrade group)
                            to_install.push(group.clone());
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
                    // Separate groups and packages for installation
                    let (groups, pkgs): (Vec<_>, Vec<_>) =
                        to_install.iter().partition(|p| Self::is_package_group(p));

                    // Install packages
                    if !pkgs.is_empty() {
                        let install_args = Self::build_yum_args(&["install", "-y"], &yum_options);
                        let pkgs_owned: Vec<String> = pkgs.into_iter().cloned().collect();
                        let (success, stdout, stderr) = Self::run_yum_command_remote(
                            conn.as_ref(),
                            &install_args,
                            &pkgs_owned,
                            Some(exec_options.clone()),
                        )
                        .await?;

                        if !success {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to install packages: {}",
                                if stderr.is_empty() { stdout } else { stderr }
                            )));
                        }

                        changed = true;
                        for pkg in &pkgs_owned {
                            results.insert(pkg.clone(), "installed".to_string());
                        }
                    }

                    // Install groups
                    if !groups.is_empty() {
                        let group_args =
                            Self::build_yum_args(&["groupinstall", "-y"], &yum_options);
                        let groups_owned: Vec<String> = groups.into_iter().cloned().collect();
                        let (success, stdout, stderr) = Self::run_yum_command_remote(
                            conn.as_ref(),
                            &group_args,
                            &groups_owned,
                            Some(exec_options.clone()),
                        )
                        .await?;

                        if !success {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to install groups: {}",
                                if stderr.is_empty() { stdout } else { stderr }
                            )));
                        }

                        changed = true;
                        for grp in &groups_owned {
                            results.insert(grp.clone(), "installed".to_string());
                        }
                    }
                }

                if !to_remove.is_empty() {
                    // Separate groups and packages for removal
                    let (groups, pkgs): (Vec<_>, Vec<_>) =
                        to_remove.iter().partition(|p| Self::is_package_group(p));

                    // Remove packages
                    if !pkgs.is_empty() {
                        let remove_args = Self::build_yum_args(&["remove", "-y"], &yum_options);
                        let pkgs_owned: Vec<String> = pkgs.into_iter().cloned().collect();
                        let (success, stdout, stderr) = Self::run_yum_command_remote(
                            conn.as_ref(),
                            &remove_args,
                            &pkgs_owned,
                            Some(exec_options.clone()),
                        )
                        .await?;

                        if !success {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to remove packages: {}",
                                if stderr.is_empty() { stdout } else { stderr }
                            )));
                        }

                        changed = true;
                        for pkg in &pkgs_owned {
                            results.insert(pkg.clone(), "removed".to_string());
                        }
                    }

                    // Remove groups
                    if !groups.is_empty() {
                        let group_args = Self::build_yum_args(&["groupremove", "-y"], &yum_options);
                        let groups_owned: Vec<String> = groups.into_iter().cloned().collect();
                        let (success, stdout, stderr) = Self::run_yum_command_remote(
                            conn.as_ref(),
                            &group_args,
                            &groups_owned,
                            Some(exec_options.clone()),
                        )
                        .await?;

                        if !success {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to remove groups: {}",
                                if stderr.is_empty() { stdout } else { stderr }
                            )));
                        }

                        changed = true;
                        for grp in &groups_owned {
                            results.insert(grp.clone(), "removed".to_string());
                        }
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
            })
        });

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yum_state_from_str() {
        assert_eq!(YumState::from_str("present").unwrap(), YumState::Present);
        assert_eq!(YumState::from_str("installed").unwrap(), YumState::Present);
        assert_eq!(YumState::from_str("absent").unwrap(), YumState::Absent);
        assert_eq!(YumState::from_str("removed").unwrap(), YumState::Absent);
        assert_eq!(YumState::from_str("latest").unwrap(), YumState::Latest);
        assert!(YumState::from_str("invalid").is_err());
    }

    #[test]
    fn test_yum_module_name() {
        let module = YumModule;
        assert_eq!(module.name(), "yum");
    }

    #[test]
    fn test_yum_module_classification() {
        let module = YumModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_yum_module_parallelization() {
        let module = YumModule;
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }

    #[test]
    fn test_yum_required_params() {
        let module = YumModule;
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_is_package_group() {
        assert!(YumModule::is_package_group("@development"));
        assert!(YumModule::is_package_group("@Web Server"));
        assert!(!YumModule::is_package_group("httpd"));
        assert!(!YumModule::is_package_group("nginx"));
    }

    #[test]
    fn test_build_yum_args_basic() {
        let options = YumOptions::default();
        let args = YumModule::build_yum_args(&["install", "-y"], &options);
        assert_eq!(args, vec!["install", "-y"]);
    }

    #[test]
    fn test_build_yum_args_with_options() {
        let options = YumOptions {
            enablerepo: Some("epel".to_string()),
            disablerepo: Some("updates".to_string()),
            disable_gpg_check: true,
            security: true,
            bugfix: false,
            exclude: Some("kernel*".to_string()),
            installroot: Some("/mnt/sysimage".to_string()),
            releasever: Some("7".to_string()),
        };
        let args = YumModule::build_yum_args(&["install", "-y"], &options);

        assert!(args.contains(&"--nogpgcheck".to_string()));
        assert!(args.contains(&"--enablerepo=epel".to_string()));
        assert!(args.contains(&"--disablerepo=updates".to_string()));
        assert!(args.contains(&"--security".to_string()));
        assert!(args.contains(&"--exclude=kernel*".to_string()));
        assert!(args.contains(&"--installroot=/mnt/sysimage".to_string()));
        assert!(args.contains(&"--releasever=7".to_string()));
        assert!(!args.contains(&"--bugfix".to_string()));
    }

    #[test]
    fn test_build_yum_args_security_only() {
        let options = YumOptions {
            security: true,
            ..Default::default()
        };
        let args = YumModule::build_yum_args(&["update", "-y"], &options);
        assert!(args.contains(&"--security".to_string()));
        assert!(!args.contains(&"--bugfix".to_string()));
    }

    #[test]
    fn test_build_yum_args_bugfix_only() {
        let options = YumOptions {
            bugfix: true,
            ..Default::default()
        };
        let args = YumModule::build_yum_args(&["update", "-y"], &options);
        assert!(args.contains(&"--bugfix".to_string()));
        assert!(!args.contains(&"--security".to_string()));
    }
}
