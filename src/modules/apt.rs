//! Apt module - Debian/Ubuntu package management
//!
//! This module manages packages using the APT package manager on Debian-based systems.
//! It supports installing, removing, and upgrading packages, as well as updating the package cache.
//!
//! Full Ansible compatibility with support for:
//! - update_cache / cache_valid_time
//! - force / purge / autoremove
//! - dpkg_options / install_recommends
//! - default_release / deb
//! - upgrade (dist, full, yes, safe)
//! - allow_downgrade / only_upgrade

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::connection::ExecuteOptions;
use crate::utils::shell_escape;
use std::collections::HashMap;

/// Desired state for a package
#[derive(Debug, Clone, PartialEq)]
pub enum AptState {
    /// Package is installed (any version)
    Present,
    /// Package is removed
    Absent,
    /// Package is at the latest version
    Latest,
    /// Install build dependencies for the package
    BuildDep,
    /// Fixed - attempt to correct broken dependencies
    Fixed,
}

impl AptState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "installed" => Ok(AptState::Present),
            "absent" | "removed" => Ok(AptState::Absent),
            "latest" => Ok(AptState::Latest),
            "build-dep" | "build_dep" | "builddep" => Ok(AptState::BuildDep),
            "fixed" => Ok(AptState::Fixed),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, latest, build-dep, fixed",
                s
            ))),
        }
    }
}

/// Upgrade mode for apt
#[derive(Debug, Clone, PartialEq)]
pub enum UpgradeMode {
    /// No upgrade
    No,
    /// Safe upgrade (apt-get upgrade)
    Yes,
    /// Safe upgrade (alias for yes)
    Safe,
    /// Full upgrade (apt-get dist-upgrade)
    Full,
    /// Distribution upgrade (apt-get dist-upgrade)
    Dist,
}

impl UpgradeMode {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "no" | "false" => Ok(UpgradeMode::No),
            "yes" | "true" | "safe" => Ok(UpgradeMode::Yes),
            "full" | "dist" => Ok(UpgradeMode::Dist),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid upgrade mode '{}'. Valid modes: no, yes, safe, full, dist",
                s
            ))),
        }
    }
}

/// Parameters for apt module operations
#[derive(Debug, Clone)]
pub struct AptParams {
    /// Package names to manage
    pub packages: Vec<String>,
    /// Desired package state
    pub state: AptState,
    /// Update apt cache before operation
    pub update_cache: bool,
    /// Cache validity time in seconds (0 = always update)
    pub cache_valid_time: u64,
    /// Force operations (--force-yes equivalent)
    pub force: bool,
    /// Purge configuration files on removal
    pub purge: bool,
    /// Remove unused dependencies
    pub autoremove: bool,
    /// dpkg options (comma-separated)
    pub dpkg_options: String,
    /// Install recommended packages
    pub install_recommends: Option<bool>,
    /// Default release for pinning
    pub default_release: Option<String>,
    /// Path to .deb file to install
    pub deb: Option<String>,
    /// Upgrade mode
    pub upgrade: UpgradeMode,
    /// Allow downgrading packages
    pub allow_downgrade: bool,
    /// Allow unauthenticated packages
    pub allow_unauthenticated: bool,
    /// Only upgrade if already installed
    pub only_upgrade: bool,
    /// Force apt-get instead of aptitude
    pub force_apt_get: bool,
    /// Clean apt cache
    pub autoclean: bool,
    /// Fail on warnings
    pub fail_on_autoremove: bool,
}

impl AptParams {
    /// Parse parameters from ModuleParams
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        // Get packages - can be a single package or a list
        let packages: Vec<String> = if let Some(names) = params.get_vec_string("name")? {
            names
        } else if let Some(name) = params.get_string("name")? {
            vec![name]
        } else if let Some(names) = params.get_vec_string("package")? {
            // Ansible alias
            names
        } else if let Some(name) = params.get_string("package")? {
            vec![name]
        } else if let Some(names) = params.get_vec_string("pkg")? {
            // Ansible alias
            names
        } else if let Some(name) = params.get_string("pkg")? {
            vec![name]
        } else {
            Vec::new()
        };

        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = AptState::from_str(&state_str)?;

        let upgrade_str = params.get_string("upgrade")?.unwrap_or_default();
        let upgrade = if upgrade_str.is_empty() {
            UpgradeMode::No
        } else {
            UpgradeMode::from_str(&upgrade_str)?
        };

        // Handle update_cache and update-cache alias
        let update_cache =
            params.get_bool_or("update_cache", false) || params.get_bool_or("update-cache", false);

        // cache_valid_time - if set, implies update_cache=true
        let cache_valid_time = params.get_i64("cache_valid_time")?.unwrap_or(0) as u64;

        // dpkg_options with sensible default
        let dpkg_options = params
            .get_string("dpkg_options")?
            .unwrap_or_else(|| "force-confdef,force-confold".to_string());

        // install_recommends - None means use system default
        let install_recommends = params
            .get_bool("install_recommends")?
            .or_else(|| params.get_bool("install-recommends").ok().flatten());

        Ok(AptParams {
            packages,
            state,
            update_cache: update_cache || cache_valid_time > 0,
            cache_valid_time,
            force: params.get_bool_or("force", false),
            purge: params.get_bool_or("purge", false),
            autoremove: params.get_bool_or("autoremove", false),
            dpkg_options,
            install_recommends,
            default_release: params
                .get_string("default_release")?
                .or_else(|| params.get_string("default-release").ok().flatten()),
            deb: params.get_string("deb")?,
            upgrade,
            allow_downgrade: params.get_bool_or("allow_downgrade", false),
            allow_unauthenticated: params.get_bool_or("allow_unauthenticated", false),
            only_upgrade: params.get_bool_or("only_upgrade", false),
            force_apt_get: params.get_bool_or("force_apt_get", false),
            autoclean: params.get_bool_or("autoclean", false),
            fail_on_autoremove: params.get_bool_or("fail_on_autoremove", false),
        })
    }

    /// Build apt-get options based on parameters
    fn build_apt_options(&self) -> Vec<String> {
        let mut opts = vec!["-y".to_string()];

        // dpkg options
        if !self.dpkg_options.is_empty() {
            for opt in self.dpkg_options.split(',') {
                opts.push(format!("-o Dpkg::Options::=--{}", opt.trim()));
            }
        }

        // install_recommends
        if let Some(install_recommends) = self.install_recommends {
            if !install_recommends {
                opts.push("--no-install-recommends".to_string());
            } else {
                opts.push("--install-recommends".to_string());
            }
        }

        // default_release
        if let Some(ref release) = self.default_release {
            opts.push("-t".to_string());
            opts.push(shell_escape(release));
        }

        // force
        if self.force {
            opts.push("--allow-unauthenticated".to_string());
            opts.push("--allow-downgrades".to_string());
            opts.push("--allow-remove-essential".to_string());
            opts.push("--allow-change-held-packages".to_string());
        }

        // allow_downgrade
        if self.allow_downgrade && !self.force {
            opts.push("--allow-downgrades".to_string());
        }

        // allow_unauthenticated
        if self.allow_unauthenticated && !self.force {
            opts.push("--allow-unauthenticated".to_string());
        }

        opts
    }
}

/// Module for APT package management
pub struct AptModule;

impl AptModule {
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

    /// Check if a package is installed using dpkg via remote connection
    async fn is_package_installed_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        package: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<bool> {
        // Extract package name without version specifier
        let pkg_name = package.split('=').next().unwrap_or(package);
        let cmd = format!(
            "dpkg -s {} 2>/dev/null | grep -q '^Status:.*installed'",
            shell_escape(pkg_name)
        );
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
        // Extract package name without version specifier
        let pkg_name = package.split('=').next().unwrap_or(package);
        let cmd = format!(
            "dpkg-query -W -f='${{Version}}' {} 2>/dev/null",
            shell_escape(pkg_name)
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

    /// Get available package version from apt-cache
    async fn get_available_version_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        package: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<Option<String>> {
        let pkg_name = package.split('=').next().unwrap_or(package);
        let cmd = format!(
            "apt-cache policy {} 2>/dev/null | grep 'Candidate:' | awk '{{print $2}}'",
            shell_escape(pkg_name)
        );
        match conn.execute(&cmd, options).await {
            Ok(result) if result.success => {
                let version = result.stdout.trim().to_string();
                if version.is_empty() || version == "(none)" {
                    Ok(None)
                } else {
                    Ok(Some(version))
                }
            }
            _ => Ok(None),
        }
    }

    /// Check if cache needs update based on cache_valid_time
    async fn cache_needs_update(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        cache_valid_time: u64,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<bool> {
        if cache_valid_time == 0 {
            return Ok(true);
        }

        // Check the age of the apt cache
        let cmd = format!(
            "stat -c %Y /var/lib/apt/periodic/update-success-stamp 2>/dev/null || stat -c %Y /var/cache/apt/pkgcache.bin 2>/dev/null || echo 0"
        );

        match conn.execute(&cmd, options).await {
            Ok(result) if result.success => {
                let cache_time: u64 = result.stdout.trim().parse().unwrap_or(0);
                if cache_time == 0 {
                    return Ok(true);
                }
                let now_cmd = "date +%s";
                match conn.execute(now_cmd, None).await {
                    Ok(now_result) if now_result.success => {
                        let now: u64 = now_result.stdout.trim().parse().unwrap_or(0);
                        Ok(now - cache_time > cache_valid_time)
                    }
                    _ => Ok(true),
                }
            }
            _ => Ok(true),
        }
    }

    /// Update the apt cache via remote connection
    async fn update_cache_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<bool> {
        let cmd = "apt-get update -qq";
        let result = conn.execute(cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to update apt cache: {}", e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to update apt cache: {}",
                result.stderr
            )));
        }

        Ok(true)
    }

    /// Install packages via remote connection
    async fn install_packages_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        packages: &[String],
        apt_params: &AptParams,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let pkg_list: Vec<String> = packages.iter().map(|p| shell_escape(p)).collect();
        let apt_opts = apt_params.build_apt_options().join(" ");

        let only_upgrade_flag = if apt_params.only_upgrade {
            "--only-upgrade"
        } else {
            ""
        };

        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get install {} {} {}",
            apt_opts,
            only_upgrade_flag,
            pkg_list.join(" ")
        );

        let result = conn.execute(&cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to install packages: {}", e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to install packages: {}",
                result.stderr
            )));
        }

        Ok((true, result.stdout, result.stderr))
    }

    /// Remove packages via remote connection
    async fn remove_packages_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        packages: &[String],
        apt_params: &AptParams,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let pkg_list: Vec<String> = packages.iter().map(|p| shell_escape(p)).collect();
        let apt_opts = apt_params.build_apt_options().join(" ");

        let purge_flag = if apt_params.purge { "--purge" } else { "" };

        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get remove {} {} {}",
            apt_opts,
            purge_flag,
            pkg_list.join(" ")
        );

        let result = conn.execute(&cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to remove packages: {}", e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to remove packages: {}",
                result.stderr
            )));
        }

        Ok((true, result.stdout, result.stderr))
    }

    /// Upgrade packages to latest version via remote connection
    async fn upgrade_packages_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        packages: &[String],
        apt_params: &AptParams,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let pkg_list: Vec<String> = packages.iter().map(|p| shell_escape(p)).collect();
        let apt_opts = apt_params.build_apt_options().join(" ");

        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get install {} {}",
            apt_opts,
            pkg_list.join(" ")
        );

        let result = conn.execute(&cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to upgrade packages: {}", e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to upgrade packages: {}",
                result.stderr
            )));
        }

        Ok((true, result.stdout, result.stderr))
    }

    /// Install build dependencies for packages
    async fn install_build_deps_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        packages: &[String],
        apt_params: &AptParams,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let pkg_list: Vec<String> = packages.iter().map(|p| shell_escape(p)).collect();
        let apt_opts = apt_params.build_apt_options().join(" ");

        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get build-dep {} {}",
            apt_opts,
            pkg_list.join(" ")
        );

        let result = conn.execute(&cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to install build deps: {}", e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to install build deps: {}",
                result.stderr
            )));
        }

        Ok((true, result.stdout, result.stderr))
    }

    /// Fix broken dependencies
    async fn fix_dependencies_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        apt_params: &AptParams,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let apt_opts = apt_params.build_apt_options().join(" ");

        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get install {} -f",
            apt_opts
        );

        let result = conn.execute(&cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to fix dependencies: {}", e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to fix dependencies: {}",
                result.stderr
            )));
        }

        Ok((true, result.stdout, result.stderr))
    }

    /// Install a .deb file
    async fn install_deb_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        deb_path: &str,
        apt_params: &AptParams,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        // Build dpkg options
        let dpkg_opts: Vec<String> = apt_params
            .dpkg_options
            .split(',')
            .map(|o| format!("--{}", o.trim()))
            .collect();

        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive dpkg {} -i {} || apt-get install -f -y",
            dpkg_opts.join(" "),
            shell_escape(deb_path)
        );

        let result = conn
            .execute(&cmd, options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to install deb: {}", e)))?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to install deb: {}",
                result.stderr
            )));
        }

        Ok((true, result.stdout, result.stderr))
    }

    /// Perform system upgrade
    async fn upgrade_system_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        upgrade_mode: &UpgradeMode,
        apt_params: &AptParams,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let apt_opts = apt_params.build_apt_options().join(" ");

        let upgrade_cmd = match upgrade_mode {
            UpgradeMode::Yes | UpgradeMode::Safe => "upgrade",
            UpgradeMode::Full | UpgradeMode::Dist => "dist-upgrade",
            UpgradeMode::No => return Ok((false, String::new(), String::new())),
        };

        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get {} {}",
            upgrade_cmd, apt_opts
        );

        let result = conn.execute(&cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to upgrade system: {}", e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to upgrade system: {}",
                result.stderr
            )));
        }

        // Check if any packages were upgraded
        let changed = !result.stdout.contains("0 upgraded, 0 newly installed");

        Ok((changed, result.stdout, result.stderr))
    }

    /// Run autoremove to clean up unused packages
    async fn autoremove_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        apt_params: &AptParams,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let apt_opts = apt_params.build_apt_options().join(" ");
        let purge_flag = if apt_params.purge { "--purge" } else { "" };

        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get autoremove {} {}",
            apt_opts, purge_flag
        );

        let result = conn
            .execute(&cmd, options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to autoremove: {}", e)))?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to autoremove: {}",
                result.stderr
            )));
        }

        // Check if any packages were removed
        let changed = !result.stdout.contains("0 to remove");

        Ok((changed, result.stdout, result.stderr))
    }

    /// Run autoclean to clean up apt cache
    async fn autoclean_remote(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let cmd = "apt-get autoclean -y";

        let result = conn
            .execute(cmd, options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to autoclean: {}", e)))?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to autoclean: {}",
                result.stderr
            )));
        }

        Ok((true, result.stdout, result.stderr))
    }
}

impl Module for AptModule {
    fn name(&self) -> &'static str {
        "apt"
    }

    fn description(&self) -> &'static str {
        "Manage packages with the APT package manager (Ansible-compatible)"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // APT uses locks - only one can run per host at a time
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        // name is not strictly required when using upgrade or deb
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        let apt_params = AptParams::from_params(params)?;

        // Validate: must have packages, upgrade, or deb
        let has_packages = !apt_params.packages.is_empty();
        let has_upgrade = apt_params.upgrade != UpgradeMode::No;
        let has_deb = apt_params.deb.is_some();
        let only_update_cache =
            apt_params.update_cache && !has_packages && !has_upgrade && !has_deb;

        if !has_packages && !has_upgrade && !has_deb && !only_update_cache {
            return Err(ModuleError::MissingParameter(
                "Either 'name', 'upgrade', 'deb', or 'update_cache' must be specified".to_string(),
            ));
        }

        // Validate package names for security
        for pkg in &apt_params.packages {
            // Extract package name without version specifier for validation
            let pkg_name = pkg.split('=').next().unwrap_or(pkg);
            super::validate_package_name(pkg_name)?;
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Parse all parameters
        let apt_params = AptParams::from_params(params)?;

        // Get connection from context
        let conn = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available in context. APT module requires a remote connection."
                    .to_string(),
            )
        })?;

        // Build execution options with become/sudo
        let exec_options = Self::build_exec_options(context);

        // Use tokio runtime to execute async operations
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut changed = false;
                let mut messages: Vec<String> = Vec::new();
                let mut all_stdout = String::new();
                let mut all_stderr = String::new();
                let mut results: HashMap<String, serde_json::Value> = HashMap::new();

                // Step 1: Update cache if requested (with cache_valid_time support)
                if apt_params.update_cache {
                    let should_update = if apt_params.cache_valid_time > 0 {
                        Self::cache_needs_update(
                            conn.as_ref(),
                            apt_params.cache_valid_time,
                            Some(exec_options.clone()),
                        )
                        .await?
                    } else {
                        true
                    };

                    if should_update && !context.check_mode {
                        Self::update_cache_remote(conn.as_ref(), Some(exec_options.clone()))
                            .await?;
                        changed = true;
                        messages.push("Cache updated".to_string());
                        results.insert("cache_updated".to_string(), serde_json::json!(true));
                    } else if should_update && context.check_mode {
                        messages.push("Would update cache".to_string());
                    } else {
                        results.insert("cache_updated".to_string(), serde_json::json!(false));
                    }
                }

                // Step 2: Handle .deb file installation
                if let Some(ref deb_path) = apt_params.deb {
                    if context.check_mode {
                        messages.push(format!("Would install deb: {}", deb_path));
                        return Ok(ModuleOutput::changed(messages.join(". ")));
                    }

                    let (deb_changed, stdout, stderr) = Self::install_deb_remote(
                        conn.as_ref(),
                        deb_path,
                        &apt_params,
                        Some(exec_options.clone()),
                    )
                    .await?;

                    if deb_changed {
                        changed = true;
                        messages.push(format!("Installed deb: {}", deb_path));
                        all_stdout.push_str(&stdout);
                        all_stderr.push_str(&stderr);
                    }
                }

                // Step 3: Handle system upgrade
                if apt_params.upgrade != UpgradeMode::No {
                    if context.check_mode {
                        messages.push(format!("Would perform {:?} upgrade", apt_params.upgrade));
                    } else {
                        let (upgrade_changed, stdout, stderr) = Self::upgrade_system_remote(
                            conn.as_ref(),
                            &apt_params.upgrade,
                            &apt_params,
                            Some(exec_options.clone()),
                        )
                        .await?;

                        if upgrade_changed {
                            changed = true;
                            messages
                                .push(format!("System {:?} upgrade completed", apt_params.upgrade));
                            all_stdout.push_str(&stdout);
                            all_stderr.push_str(&stderr);
                        }
                    }
                }

                // Step 4: Handle package operations
                if !apt_params.packages.is_empty() {
                    let mut to_install: Vec<String> = Vec::new();
                    let mut to_remove: Vec<String> = Vec::new();
                    let mut to_upgrade: Vec<String> = Vec::new();
                    let mut to_build_dep: Vec<String> = Vec::new();
                    let mut already_ok: Vec<String> = Vec::new();

                    // Check current state of each package
                    for package in &apt_params.packages {
                        let is_installed = Self::is_package_installed_remote(
                            conn.as_ref(),
                            package,
                            Some(exec_options.clone()),
                        )
                        .await?;

                        match apt_params.state {
                            AptState::Present => {
                                if is_installed {
                                    already_ok.push(package.clone());
                                } else {
                                    to_install.push(package.clone());
                                }
                            }
                            AptState::Absent => {
                                if is_installed {
                                    to_remove.push(package.clone());
                                } else {
                                    already_ok.push(package.clone());
                                }
                            }
                            AptState::Latest => {
                                // For latest state, check if update is available
                                if is_installed {
                                    let installed_ver = Self::get_installed_version_remote(
                                        conn.as_ref(),
                                        package,
                                        Some(exec_options.clone()),
                                    )
                                    .await?;
                                    let available_ver = Self::get_available_version_remote(
                                        conn.as_ref(),
                                        package,
                                        Some(exec_options.clone()),
                                    )
                                    .await?;

                                    if installed_ver != available_ver {
                                        to_upgrade.push(package.clone());
                                    } else {
                                        already_ok.push(package.clone());
                                    }
                                } else {
                                    to_install.push(package.clone());
                                }
                            }
                            AptState::BuildDep => {
                                to_build_dep.push(package.clone());
                            }
                            AptState::Fixed => {
                                // Fixed state is handled at the end
                            }
                        }
                    }

                    // Check mode - return what would happen
                    if context.check_mode {
                        if !to_install.is_empty() {
                            messages.push(format!("Would install: {}", to_install.join(", ")));
                        }
                        if !to_remove.is_empty() {
                            messages.push(format!("Would remove: {}", to_remove.join(", ")));
                        }
                        if !to_upgrade.is_empty() {
                            messages.push(format!("Would upgrade: {}", to_upgrade.join(", ")));
                        }
                        if !to_build_dep.is_empty() {
                            messages.push(format!(
                                "Would install build deps for: {}",
                                to_build_dep.join(", ")
                            ));
                        }
                        if apt_params.state == AptState::Fixed {
                            messages.push("Would fix broken dependencies".to_string());
                        }
                        if !already_ok.is_empty() && messages.is_empty() {
                            return Ok(ModuleOutput::ok(format!(
                                "All packages already in desired state: {}",
                                already_ok.join(", ")
                            )));
                        }
                        if !messages.is_empty() {
                            return Ok(ModuleOutput::changed(messages.join(". ")));
                        }
                    }

                    // Perform actual operations
                    let mut pkg_results: HashMap<String, String> = HashMap::new();

                    if !to_install.is_empty() {
                        let (_, stdout, stderr) = Self::install_packages_remote(
                            conn.as_ref(),
                            &to_install,
                            &apt_params,
                            Some(exec_options.clone()),
                        )
                        .await?;
                        changed = true;
                        for pkg in &to_install {
                            pkg_results.insert(pkg.clone(), "installed".to_string());
                        }
                        messages.push(format!("Installed: {}", to_install.join(", ")));
                        all_stdout.push_str(&stdout);
                        all_stderr.push_str(&stderr);
                    }

                    if !to_upgrade.is_empty() {
                        let (_, stdout, stderr) = Self::upgrade_packages_remote(
                            conn.as_ref(),
                            &to_upgrade,
                            &apt_params,
                            Some(exec_options.clone()),
                        )
                        .await?;
                        changed = true;
                        for pkg in &to_upgrade {
                            pkg_results.insert(pkg.clone(), "upgraded".to_string());
                        }
                        messages.push(format!("Upgraded: {}", to_upgrade.join(", ")));
                        all_stdout.push_str(&stdout);
                        all_stderr.push_str(&stderr);
                    }

                    if !to_remove.is_empty() {
                        let (_, stdout, stderr) = Self::remove_packages_remote(
                            conn.as_ref(),
                            &to_remove,
                            &apt_params,
                            Some(exec_options.clone()),
                        )
                        .await?;
                        changed = true;
                        for pkg in &to_remove {
                            pkg_results.insert(pkg.clone(), "removed".to_string());
                        }
                        messages.push(format!("Removed: {}", to_remove.join(", ")));
                        all_stdout.push_str(&stdout);
                        all_stderr.push_str(&stderr);
                    }

                    if !to_build_dep.is_empty() {
                        let (_, stdout, stderr) = Self::install_build_deps_remote(
                            conn.as_ref(),
                            &to_build_dep,
                            &apt_params,
                            Some(exec_options.clone()),
                        )
                        .await?;
                        changed = true;
                        for pkg in &to_build_dep {
                            pkg_results.insert(pkg.clone(), "build-dep installed".to_string());
                        }
                        messages.push(format!(
                            "Build deps installed for: {}",
                            to_build_dep.join(", ")
                        ));
                        all_stdout.push_str(&stdout);
                        all_stderr.push_str(&stderr);
                    }

                    // Handle fixed state
                    if apt_params.state == AptState::Fixed {
                        let (_, stdout, stderr) = Self::fix_dependencies_remote(
                            conn.as_ref(),
                            &apt_params,
                            Some(exec_options.clone()),
                        )
                        .await?;
                        changed = true;
                        messages.push("Fixed broken dependencies".to_string());
                        all_stdout.push_str(&stdout);
                        all_stderr.push_str(&stderr);
                    }

                    for pkg in &already_ok {
                        pkg_results.insert(pkg.clone(), "ok".to_string());
                    }

                    results.insert("packages".to_string(), serde_json::json!(pkg_results));
                }

                // Step 5: Handle autoremove
                if apt_params.autoremove && !context.check_mode {
                    let (ar_changed, stdout, stderr) = Self::autoremove_remote(
                        conn.as_ref(),
                        &apt_params,
                        Some(exec_options.clone()),
                    )
                    .await?;

                    if ar_changed {
                        changed = true;
                        messages.push("Autoremoved unused packages".to_string());
                        all_stdout.push_str(&stdout);
                        all_stderr.push_str(&stderr);
                    }
                } else if apt_params.autoremove && context.check_mode {
                    messages.push("Would autoremove unused packages".to_string());
                }

                // Step 6: Handle autoclean
                if apt_params.autoclean && !context.check_mode {
                    let (_, stdout, stderr) =
                        Self::autoclean_remote(conn.as_ref(), Some(exec_options.clone())).await?;
                    changed = true;
                    messages.push("Cleaned apt cache".to_string());
                    all_stdout.push_str(&stdout);
                    all_stderr.push_str(&stderr);
                } else if apt_params.autoclean && context.check_mode {
                    messages.push("Would clean apt cache".to_string());
                }

                // Build final output
                let msg = if messages.is_empty() {
                    "No changes required".to_string()
                } else {
                    messages.join(". ")
                };

                let mut output = if changed {
                    ModuleOutput::changed(msg)
                } else {
                    ModuleOutput::ok(msg)
                };

                // Add results data
                for (key, value) in results {
                    output = output.with_data(key, value);
                }

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

    fn check(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<ModuleOutput> {
        let check_context = ModuleContext {
            check_mode: true,
            ..context.clone()
        };
        self.execute(params, &check_context)
    }

    fn diff(&self, params: &ModuleParams, context: &ModuleContext) -> ModuleResult<Option<Diff>> {
        let apt_params = AptParams::from_params(params)?;

        // Get connection from context
        let conn = match context.connection.as_ref() {
            Some(c) => c,
            None => {
                // No connection available, return basic diff without checking remote state
                let mut before_lines = Vec::new();
                let mut after_lines = Vec::new();

                // Handle upgrade
                if apt_params.upgrade != UpgradeMode::No {
                    before_lines.push("System packages: (current versions)".to_string());
                    after_lines.push(format!(
                        "System packages: (after {:?} upgrade)",
                        apt_params.upgrade
                    ));
                }

                // Handle deb
                if let Some(ref deb) = apt_params.deb {
                    before_lines.push(format!("{}: (not installed)", deb));
                    after_lines.push(format!("{}: (will be installed from deb)", deb));
                }

                // Handle packages
                for package in &apt_params.packages {
                    match apt_params.state {
                        AptState::Present | AptState::Latest => {
                            before_lines.push(format!("{}: (unknown)", package));
                            after_lines.push(format!("{}: (will be installed/updated)", package));
                        }
                        AptState::Absent => {
                            before_lines.push(format!("{}: (unknown)", package));
                            after_lines.push(format!("{}: (will be removed)", package));
                        }
                        AptState::BuildDep => {
                            before_lines.push(format!("{}: build-deps (unknown)", package));
                            after_lines
                                .push(format!("{}: build-deps (will be installed)", package));
                        }
                        AptState::Fixed => {
                            before_lines.push("Dependencies: (possibly broken)".to_string());
                            after_lines.push("Dependencies: (will be fixed)".to_string());
                        }
                    }
                }

                if apt_params.autoremove {
                    before_lines.push("Unused packages: (present)".to_string());
                    after_lines.push("Unused packages: (removed)".to_string());
                }

                return Ok(Some(Diff::new(
                    before_lines.join("\n"),
                    after_lines.join("\n"),
                )));
            }
        };

        let exec_options = Self::build_exec_options(context);

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let mut before_lines = Vec::new();
                let mut after_lines = Vec::new();

                // Handle upgrade
                if apt_params.upgrade != UpgradeMode::No {
                    before_lines.push("System packages: (current versions)".to_string());
                    after_lines.push(format!(
                        "System packages: (after {:?} upgrade)",
                        apt_params.upgrade
                    ));
                }

                // Handle deb
                if let Some(ref deb) = apt_params.deb {
                    before_lines.push(format!("{}: (not installed)", deb));
                    after_lines.push(format!("{}: (will be installed from deb)", deb));
                }

                // Handle packages
                for package in &apt_params.packages {
                    let is_installed = Self::is_package_installed_remote(
                        conn.as_ref(),
                        package,
                        Some(exec_options.clone()),
                    )
                    .await?;

                    let installed_version = Self::get_installed_version_remote(
                        conn.as_ref(),
                        package,
                        Some(exec_options.clone()),
                    )
                    .await?
                    .unwrap_or_default();

                    let available_version = Self::get_available_version_remote(
                        conn.as_ref(),
                        package,
                        Some(exec_options.clone()),
                    )
                    .await?
                    .unwrap_or_else(|| "(unknown)".to_string());

                    match apt_params.state {
                        AptState::Present => {
                            if is_installed {
                                before_lines.push(format!("{}: {}", package, installed_version));
                                after_lines.push(format!(
                                    "{}: {} (no change)",
                                    package, installed_version
                                ));
                            } else {
                                before_lines.push(format!("{}: (not installed)", package));
                                after_lines.push(format!(
                                    "{}: {} (will install)",
                                    package, available_version
                                ));
                            }
                        }
                        AptState::Absent => {
                            if is_installed {
                                let removal_type =
                                    if apt_params.purge { "purge" } else { "remove" };
                                before_lines.push(format!("{}: {}", package, installed_version));
                                after_lines.push(format!("{}: (will {})", package, removal_type));
                            } else {
                                before_lines.push(format!("{}: (not installed)", package));
                                after_lines.push(format!("{}: (not installed)", package));
                            }
                        }
                        AptState::Latest => {
                            if is_installed {
                                if installed_version != available_version {
                                    before_lines
                                        .push(format!("{}: {}", package, installed_version));
                                    after_lines.push(format!(
                                        "{}: {} (will upgrade)",
                                        package, available_version
                                    ));
                                } else {
                                    before_lines
                                        .push(format!("{}: {}", package, installed_version));
                                    after_lines.push(format!(
                                        "{}: {} (already latest)",
                                        package, installed_version
                                    ));
                                }
                            } else {
                                before_lines.push(format!("{}: (not installed)", package));
                                after_lines.push(format!(
                                    "{}: {} (will install)",
                                    package, available_version
                                ));
                            }
                        }
                        AptState::BuildDep => {
                            before_lines.push(format!("{}: build-deps (unknown)", package));
                            after_lines.push(format!("{}: build-deps (will install)", package));
                        }
                        AptState::Fixed => {
                            before_lines.push("Dependencies: (possibly broken)".to_string());
                            after_lines.push("Dependencies: (will be fixed)".to_string());
                        }
                    }
                }

                if apt_params.autoremove {
                    before_lines.push("Unused packages: (present)".to_string());
                    after_lines.push("Unused packages: (will be removed)".to_string());
                }

                if apt_params.autoclean {
                    before_lines.push("Apt cache: (full)".to_string());
                    after_lines.push("Apt cache: (cleaned)".to_string());
                }

                Ok(Some(Diff::new(
                    before_lines.join("\n"),
                    after_lines.join("\n"),
                )))
            })
        });

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apt_state_from_str() {
        assert_eq!(AptState::from_str("present").unwrap(), AptState::Present);
        assert_eq!(AptState::from_str("installed").unwrap(), AptState::Present);
        assert_eq!(AptState::from_str("absent").unwrap(), AptState::Absent);
        assert_eq!(AptState::from_str("removed").unwrap(), AptState::Absent);
        assert_eq!(AptState::from_str("latest").unwrap(), AptState::Latest);
        assert_eq!(AptState::from_str("build-dep").unwrap(), AptState::BuildDep);
        assert_eq!(AptState::from_str("build_dep").unwrap(), AptState::BuildDep);
        assert_eq!(AptState::from_str("fixed").unwrap(), AptState::Fixed);
        assert!(AptState::from_str("invalid").is_err());
    }

    #[test]
    fn test_upgrade_mode_from_str() {
        assert_eq!(UpgradeMode::from_str("no").unwrap(), UpgradeMode::No);
        assert_eq!(UpgradeMode::from_str("false").unwrap(), UpgradeMode::No);
        assert_eq!(UpgradeMode::from_str("yes").unwrap(), UpgradeMode::Yes);
        assert_eq!(UpgradeMode::from_str("safe").unwrap(), UpgradeMode::Yes);
        assert_eq!(UpgradeMode::from_str("full").unwrap(), UpgradeMode::Dist);
        assert_eq!(UpgradeMode::from_str("dist").unwrap(), UpgradeMode::Dist);
        assert!(UpgradeMode::from_str("invalid").is_err());
    }

    #[test]
    fn test_apt_module_name() {
        let module = AptModule;
        assert_eq!(module.name(), "apt");
    }

    #[test]
    fn test_apt_module_classification() {
        let module = AptModule;
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }

    #[test]
    fn test_apt_module_parallelization() {
        let module = AptModule;
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::HostExclusive
        );
    }

    #[test]
    fn test_apt_module_required_params() {
        let module = AptModule;
        // No required params since name is optional when using upgrade/deb/update_cache
        assert!(module.required_params().is_empty());
    }

    #[test]
    fn test_apt_params_from_params_basic() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("nginx"));
        params.insert("state".to_string(), serde_json::json!("present"));

        let apt_params = AptParams::from_params(&params).unwrap();
        assert_eq!(apt_params.packages, vec!["nginx".to_string()]);
        assert_eq!(apt_params.state, AptState::Present);
        assert!(!apt_params.update_cache);
        assert!(!apt_params.force);
        assert!(!apt_params.purge);
        assert!(!apt_params.autoremove);
    }

    #[test]
    fn test_apt_params_from_params_with_options() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!(["nginx", "htop"]));
        params.insert("state".to_string(), serde_json::json!("latest"));
        params.insert("update_cache".to_string(), serde_json::json!(true));
        params.insert("cache_valid_time".to_string(), serde_json::json!(3600));
        params.insert("purge".to_string(), serde_json::json!(true));
        params.insert("autoremove".to_string(), serde_json::json!(true));
        params.insert("force".to_string(), serde_json::json!(true));
        params.insert("install_recommends".to_string(), serde_json::json!(false));
        params.insert(
            "default_release".to_string(),
            serde_json::json!("buster-backports"),
        );

        let apt_params = AptParams::from_params(&params).unwrap();
        assert_eq!(
            apt_params.packages,
            vec!["nginx".to_string(), "htop".to_string()]
        );
        assert_eq!(apt_params.state, AptState::Latest);
        assert!(apt_params.update_cache);
        assert_eq!(apt_params.cache_valid_time, 3600);
        assert!(apt_params.purge);
        assert!(apt_params.autoremove);
        assert!(apt_params.force);
        assert_eq!(apt_params.install_recommends, Some(false));
        assert_eq!(
            apt_params.default_release,
            Some("buster-backports".to_string())
        );
    }

    #[test]
    fn test_apt_params_package_aliases() {
        // Test 'package' alias
        let mut params: ModuleParams = HashMap::new();
        params.insert("package".to_string(), serde_json::json!("vim"));
        let apt_params = AptParams::from_params(&params).unwrap();
        assert_eq!(apt_params.packages, vec!["vim".to_string()]);

        // Test 'pkg' alias
        let mut params: ModuleParams = HashMap::new();
        params.insert("pkg".to_string(), serde_json::json!("emacs"));
        let apt_params = AptParams::from_params(&params).unwrap();
        assert_eq!(apt_params.packages, vec!["emacs".to_string()]);
    }

    #[test]
    fn test_apt_params_upgrade_mode() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("upgrade".to_string(), serde_json::json!("dist"));
        params.insert("update_cache".to_string(), serde_json::json!(true));

        let apt_params = AptParams::from_params(&params).unwrap();
        assert_eq!(apt_params.upgrade, UpgradeMode::Dist);
        assert!(apt_params.update_cache);
    }

    #[test]
    fn test_apt_params_deb_install() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("deb".to_string(), serde_json::json!("/tmp/mypackage.deb"));

        let apt_params = AptParams::from_params(&params).unwrap();
        assert_eq!(apt_params.deb, Some("/tmp/mypackage.deb".to_string()));
    }

    #[test]
    fn test_apt_params_build_apt_options() {
        let apt_params = AptParams {
            packages: vec![],
            state: AptState::Present,
            update_cache: false,
            cache_valid_time: 0,
            force: false,
            purge: false,
            autoremove: false,
            dpkg_options: "force-confdef,force-confold".to_string(),
            install_recommends: Some(false),
            default_release: Some("stable".to_string()),
            deb: None,
            upgrade: UpgradeMode::No,
            allow_downgrade: true,
            allow_unauthenticated: false,
            only_upgrade: false,
            force_apt_get: false,
            autoclean: false,
            fail_on_autoremove: false,
        };

        let opts = apt_params.build_apt_options();
        assert!(opts.contains(&"-y".to_string()));
        assert!(opts.contains(&"-o Dpkg::Options::=--force-confdef".to_string()));
        assert!(opts.contains(&"-o Dpkg::Options::=--force-confold".to_string()));
        assert!(opts.contains(&"--no-install-recommends".to_string()));
        assert!(opts.contains(&"-t".to_string()));
        assert!(opts.contains(&"stable".to_string()));
        assert!(opts.contains(&"--allow-downgrades".to_string()));
    }

    #[test]
    fn test_apt_params_build_apt_options_force() {
        let apt_params = AptParams {
            packages: vec![],
            state: AptState::Present,
            update_cache: false,
            cache_valid_time: 0,
            force: true,
            purge: false,
            autoremove: false,
            dpkg_options: String::new(),
            install_recommends: None,
            default_release: None,
            deb: None,
            upgrade: UpgradeMode::No,
            allow_downgrade: false,
            allow_unauthenticated: false,
            only_upgrade: false,
            force_apt_get: false,
            autoclean: false,
            fail_on_autoremove: false,
        };

        let opts = apt_params.build_apt_options();
        assert!(opts.contains(&"--allow-unauthenticated".to_string()));
        assert!(opts.contains(&"--allow-downgrades".to_string()));
        assert!(opts.contains(&"--allow-remove-essential".to_string()));
        assert!(opts.contains(&"--allow-change-held-packages".to_string()));
    }

    #[test]
    fn test_validate_params_missing_required() {
        let module = AptModule;
        let params: ModuleParams = HashMap::new();

        // Should fail - no name, upgrade, deb, or update_cache
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_with_name() {
        let module = AptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("nginx"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_with_upgrade() {
        let module = AptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("upgrade".to_string(), serde_json::json!("dist"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_with_deb() {
        let module = AptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("deb".to_string(), serde_json::json!("/tmp/pkg.deb"));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_update_cache_only() {
        let module = AptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("update_cache".to_string(), serde_json::json!(true));

        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_validate_params_invalid_package_name() {
        let module = AptModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("pkg; rm -rf /"));

        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_validate_params_package_with_version() {
        let module = AptModule;
        let mut params: ModuleParams = HashMap::new();
        // Version specifier should be allowed
        params.insert(
            "name".to_string(),
            serde_json::json!("nginx=1.18.0-0ubuntu1"),
        );

        // This should pass validation (the package name part is valid)
        // Note: In the actual implementation, the version part after = is handled by apt
        assert!(module.validate_params(&params).is_ok());
    }

    // Integration tests would require actual apt access
    // These are unit tests for the parsing/configuration logic
}
