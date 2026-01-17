//! DNF module - Fedora/RHEL package management
//!
//! This module manages packages using the DNF package manager on Fedora,
//! RHEL 8+, CentOS 8+, and other RPM-based distributions.
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

/// DNF module configuration options
#[derive(Debug, Clone, Default)]
struct DnfOptions {
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
    /// Allow erasing of installed packages to resolve dependencies
    allowerasing: bool,
    /// Do not limit transactions to best candidate
    nobest: bool,
}

/// Desired state for a package
#[derive(Debug, Clone, PartialEq)]
pub enum DnfState {
    /// Package should be installed
    Present,
    /// Package should be removed
    Absent,
    /// Package should be at the latest version
    Latest,
}

impl DnfState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "installed" => Ok(DnfState::Present),
            "absent" | "removed" => Ok(DnfState::Absent),
            "latest" => Ok(DnfState::Latest),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, latest",
                s
            ))),
        }
    }
}

/// Module for DNF package management
#[derive(Default)]
pub struct DnfModule;

impl DnfModule {
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
    async fn get_package_version_remote(
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

    /// Build DNF command arguments from options
    fn build_dnf_args(base_args: &[&str], dnf_options: &DnfOptions) -> Vec<String> {
        let mut args: Vec<String> = base_args.iter().map(|s| s.to_string()).collect();

        if dnf_options.disable_gpg_check {
            args.push("--nogpgcheck".to_string());
        }

        if let Some(ref repo) = dnf_options.enablerepo {
            args.push(format!("--enablerepo={}", repo));
        }

        if let Some(ref repo) = dnf_options.disablerepo {
            args.push(format!("--disablerepo={}", repo));
        }

        if dnf_options.security {
            args.push("--security".to_string());
        }

        if dnf_options.bugfix {
            args.push("--bugfix".to_string());
        }

        if let Some(ref exclude) = dnf_options.exclude {
            args.push(format!("--exclude={}", exclude));
        }

        if let Some(ref installroot) = dnf_options.installroot {
            args.push(format!("--installroot={}", installroot));
        }

        if let Some(ref releasever) = dnf_options.releasever {
            args.push(format!("--releasever={}", releasever));
        }

        if dnf_options.allowerasing {
            args.push("--allowerasing".to_string());
        }

        if dnf_options.nobest {
            args.push("--nobest".to_string());
        }

        args
    }

    /// Check if a name represents a package group
    fn is_package_group(name: &str) -> bool {
        name.starts_with('@')
    }

    /// Run a DNF command via remote connection
    async fn run_dnf_command_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        args: &[String],
        packages: &[String],
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let mut cmd_parts: Vec<std::borrow::Cow<'_, str>> = vec![std::borrow::Cow::Borrowed("dnf")];
        cmd_parts.extend(args.iter().map(|s| std::borrow::Cow::Borrowed(s.as_str())));
        cmd_parts.extend(packages.iter().map(|s| shell_escape(s)));

        let cmd = cmd_parts.join(" ");

        let result = conn
            .execute(&cmd, options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to execute dnf: {}", e)))?;

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
            "dnf group list installed 2>/dev/null | grep -qi {}",
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
            "dnf repoquery --qf '%{{VERSION}}-%{{RELEASE}}' {} 2>/dev/null | tail -1",
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

    /// Update DNF cache via remote connection
    async fn update_cache_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<()> {
        let cmd = "dnf makecache";
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
}

impl Module for DnfModule {
    fn name(&self) -> &'static str {
        "dnf"
    }

    fn description(&self) -> &'static str {
        "Manage packages with the DNF package manager"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // DNF uses locks - only one can run per host at a time
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
        let state = DnfState::from_str(&state_str)?;
        let update_cache = params.get_bool_or("update_cache", false);

        // Build DNF options from parameters
        let dnf_options = DnfOptions {
            enablerepo: params.get_string("enablerepo")?,
            disablerepo: params.get_string("disablerepo")?,
            disable_gpg_check: params.get_bool_or("disable_gpg_check", false),
            security: params.get_bool_or("security", false),
            bugfix: params.get_bool_or("bugfix", false),
            exclude: params.get_string("exclude")?,
            installroot: params.get_string("installroot")?,
            releasever: params.get_string("releasever")?,
            allowerasing: params.get_bool_or("allowerasing", false),
            nobest: params.get_bool_or("nobest", false),
        };

        // Get connection from context
        let conn = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available in context. DNF module requires a remote connection."
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
                        DnfState::Present => {
                            if is_installed {
                                already_ok.push(package.clone());
                            } else {
                                to_install.push(package.clone());
                            }
                        }
                        DnfState::Absent => {
                            if is_installed {
                                to_remove.push(package.clone());
                            } else {
                                already_ok.push(package.clone());
                            }
                        }
                        DnfState::Latest => {
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
                        DnfState::Present => {
                            if is_installed {
                                already_ok.push(group.clone());
                            } else {
                                to_install.push(group.clone());
                            }
                        }
                        DnfState::Absent => {
                            if is_installed {
                                to_remove.push(group.clone());
                            } else {
                                already_ok.push(group.clone());
                            }
                        }
                        DnfState::Latest => {
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
                        let install_args = Self::build_dnf_args(&["install", "-y"], &dnf_options);
                        let pkgs_owned: Vec<String> = pkgs.into_iter().cloned().collect();
                        let (success, stdout, stderr) = Self::run_dnf_command_remote(
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
                            Self::build_dnf_args(&["group", "install", "-y"], &dnf_options);
                        let groups_owned: Vec<String> = groups.into_iter().cloned().collect();
                        let (success, stdout, stderr) = Self::run_dnf_command_remote(
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
                        let remove_args = Self::build_dnf_args(&["remove", "-y"], &dnf_options);
                        let pkgs_owned: Vec<String> = pkgs.into_iter().cloned().collect();
                        let (success, stdout, stderr) = Self::run_dnf_command_remote(
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
                        let group_args =
                            Self::build_dnf_args(&["group", "remove", "-y"], &dnf_options);
                        let groups_owned: Vec<String> = groups.into_iter().cloned().collect();
                        let (success, stdout, stderr) = Self::run_dnf_command_remote(
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
    fn test_dnf_state_from_str() {
        assert_eq!(DnfState::from_str("present").unwrap(), DnfState::Present);
        assert_eq!(DnfState::from_str("installed").unwrap(), DnfState::Present);
        assert_eq!(DnfState::from_str("absent").unwrap(), DnfState::Absent);
        assert_eq!(DnfState::from_str("removed").unwrap(), DnfState::Absent);
        assert_eq!(DnfState::from_str("latest").unwrap(), DnfState::Latest);
        assert!(DnfState::from_str("invalid").is_err());
    }

    #[test]
    fn test_dnf_module_name() {
        let module = DnfModule;
        assert_eq!(module.name(), "dnf");
    }

    #[test]
    fn test_dnf_module_classification() {
        let module = DnfModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_dnf_module_parallelization_hint() {
        let module = DnfModule;
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }

    #[test]
    fn test_dnf_module_required_params() {
        let module = DnfModule;
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_is_package_group() {
        assert!(DnfModule::is_package_group("@development-tools"));
        assert!(DnfModule::is_package_group("@Web Server"));
        assert!(!DnfModule::is_package_group("nginx"));
        assert!(!DnfModule::is_package_group("httpd"));
    }

    #[test]
    fn test_build_dnf_args_basic() {
        let options = DnfOptions::default();
        let args = DnfModule::build_dnf_args(&["install", "-y"], &options);
        assert_eq!(args, vec!["install", "-y"]);
    }

    #[test]
    fn test_build_dnf_args_with_options() {
        let options = DnfOptions {
            enablerepo: Some("epel".to_string()),
            disablerepo: Some("updates".to_string()),
            disable_gpg_check: true,
            security: true,
            bugfix: false,
            exclude: Some("kernel*".to_string()),
            installroot: Some("/mnt/sysimage".to_string()),
            releasever: Some("8".to_string()),
            allowerasing: true,
            nobest: true,
        };
        let args = DnfModule::build_dnf_args(&["install", "-y"], &options);

        assert!(args.contains(&"--nogpgcheck".to_string()));
        assert!(args.contains(&"--enablerepo=epel".to_string()));
        assert!(args.contains(&"--disablerepo=updates".to_string()));
        assert!(args.contains(&"--security".to_string()));
        assert!(args.contains(&"--exclude=kernel*".to_string()));
        assert!(args.contains(&"--installroot=/mnt/sysimage".to_string()));
        assert!(args.contains(&"--releasever=8".to_string()));
        assert!(args.contains(&"--allowerasing".to_string()));
        assert!(args.contains(&"--nobest".to_string()));
        assert!(!args.contains(&"--bugfix".to_string()));
    }

    #[test]
    fn test_build_dnf_args_security_only() {
        let options = DnfOptions {
            security: true,
            ..Default::default()
        };
        let args = DnfModule::build_dnf_args(&["update", "-y"], &options);
        assert!(args.contains(&"--security".to_string()));
        assert!(!args.contains(&"--bugfix".to_string()));
    }

    #[test]
    fn test_build_dnf_args_bugfix_only() {
        let options = DnfOptions {
            bugfix: true,
            ..Default::default()
        };
        let args = DnfModule::build_dnf_args(&["update", "-y"], &options);
        assert!(args.contains(&"--bugfix".to_string()));
        assert!(!args.contains(&"--security".to_string()));
    }
}
