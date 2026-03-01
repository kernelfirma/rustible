//! Pip module - Python package management
//!
//! This module manages Python packages using pip, supporting virtualenvs,
//! requirements files, proxy configuration, and different package states.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::connection::ExecuteOptions;
use crate::utils::shell_escape;
use std::collections::HashMap;

/// Desired state for a pip package
#[derive(Debug, Clone, PartialEq)]
pub enum PipState {
    Present,
    Absent,
    Latest,
    ForceReinstall,
}

impl PipState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "installed" => Ok(PipState::Present),
            "absent" | "removed" => Ok(PipState::Absent),
            "latest" => Ok(PipState::Latest),
            "forcereinstall" | "force-reinstall" | "force_reinstall" => {
                Ok(PipState::ForceReinstall)
            }
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, latest, forcereinstall",
                s
            ))),
        }
    }
}

impl std::str::FromStr for PipState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PipState::from_str(s)
    }
}

/// Configuration for pip operations
#[derive(Debug, Clone)]
struct PipConfig {
    pip_cmd: String,
    extra_args: Vec<String>,
    chdir: Option<String>,
    editable: bool,
    umask: Option<u32>,
    proxy: Option<String>,
    index_url: Option<String>,
    extra_index_url: Option<String>,
    no_index: bool,
    find_links: Option<String>,
}

impl PipConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let executable = params
            .get_string("executable")?
            .unwrap_or_else(|| "pip3".to_string());

        // If virtualenv is specified, use the pip from that virtualenv
        let pip_cmd = if let Some(venv) = params.get_string("virtualenv")? {
            format!("{}/bin/pip", venv)
        } else {
            executable
        };

        // Parse extra_args - check raw type to handle array vs string correctly
        let extra_args = match params.get("extra_args") {
            Some(serde_json::Value::Array(arr)) => {
                // Array: use elements directly
                arr.iter()
                    .map(|v| match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string().trim_matches('"').to_string(),
                    })
                    .collect()
            }
            Some(serde_json::Value::String(s)) => {
                // String: parse with shell_words for proper quote/space handling
                shell_words::split(s).map_err(|e| {
                    ModuleError::InvalidParameter(format!("Invalid extra_args: {}", e))
                })?
            }
            Some(_) => {
                return Err(ModuleError::InvalidParameter(
                    "extra_args must be a string or array".to_string(),
                ))
            }
            None => Vec::new(),
        };

        // Parse umask as octal string or integer
        let umask = if let Some(umask_val) = params.get("umask") {
            if let Some(s) = umask_val.as_str() {
                Some(
                    u32::from_str_radix(s.trim_start_matches("0o"), 8).map_err(|_| {
                        ModuleError::InvalidParameter(format!("Invalid umask: {}", s))
                    })?,
                )
            } else {
                umask_val.as_u64().map(|n| n as u32)
            }
        } else {
            None
        };

        Ok(Self {
            pip_cmd,
            extra_args,
            chdir: params.get_string("chdir")?,
            editable: params.get_bool("editable")?.unwrap_or(false),
            umask,
            proxy: params.get_string("proxy")?,
            index_url: params.get_string("index_url")?,
            extra_index_url: params.get_string("extra_index_url")?,
            no_index: params.get_bool("no_index")?.unwrap_or(false),
            find_links: params.get_string("find_links")?,
        })
    }

    /// Build the base pip command string with common arguments.
    ///
    /// Returns a shell command string like "/path/to/pip3 --proxy http://proxy --index-url ...".
    /// User-supplied values are shell-escaped to prevent injection.
    fn build_command_string(&self) -> String {
        let mut parts: Vec<String> = vec![shell_escape(&self.pip_cmd).into_owned()];

        // Add proxy if configured
        if let Some(ref proxy) = self.proxy {
            parts.push("--proxy".to_string());
            parts.push(shell_escape(proxy).into_owned());
        }

        // Add index URL options
        if let Some(ref index_url) = self.index_url {
            parts.push("--index-url".to_string());
            parts.push(shell_escape(index_url).into_owned());
        }

        if let Some(ref extra_index) = self.extra_index_url {
            parts.push("--extra-index-url".to_string());
            parts.push(shell_escape(extra_index).into_owned());
        }

        if self.no_index {
            parts.push("--no-index".to_string());
        }

        if let Some(ref find_links) = self.find_links {
            parts.push("--find-links".to_string());
            parts.push(shell_escape(find_links).into_owned());
        }

        parts.join(" ")
    }

    /// Build extra args as a shell-safe string fragment
    fn extra_args_string(&self) -> String {
        self.extra_args
            .iter()
            .map(|a| shell_escape(a).into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Module for pip package management
pub struct PipModule;

impl PipModule {
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

    /// Build the pip command path based on parameters
    /// Returns the path to the pip executable (respects virtualenv if set)
    pub fn build_pip_command(&self, params: &ModuleParams) -> ModuleResult<String> {
        let config = PipConfig::from_params(params)?;
        Ok(config.pip_cmd)
    }

    /// Check if a package is installed via remote connection
    async fn is_package_installed(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        config: &PipConfig,
        package: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<bool> {
        let pkg_name = Self::extract_package_name(package);

        let cmd = format!(
            "{} show {}",
            config.build_command_string(),
            shell_escape(&pkg_name)
        );
        match conn.execute(&cmd, options).await {
            Ok(result) => Ok(result.success),
            Err(_) => Ok(false),
        }
    }

    /// Extract package name from a package specification (removes version specifiers)
    fn extract_package_name(package: &str) -> String {
        // Handle various version specifier formats
        // e.g., "flask>=2.0", "django==4.2", "requests[security]>=2.0"
        let name = package
            .split(&['>', '<', '=', '!', '~', '@', '['][..])
            .next()
            .unwrap_or(package)
            .trim();
        name.to_string()
    }

    /// Get installed version of a package via remote connection
    async fn get_installed_version(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        config: &PipConfig,
        package: &str,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<Option<String>> {
        let pkg_name = Self::extract_package_name(package);

        let cmd = format!(
            "{} show {}",
            config.build_command_string(),
            shell_escape(&pkg_name)
        );
        match conn.execute(&cmd, options).await {
            Ok(result) if result.success => {
                for line in result.stdout.lines() {
                    if let Some(version) = line.strip_prefix("Version:") {
                        let version = version.trim().to_string();
                        if version.is_empty() {
                            return Ok(None);
                        }
                        return Ok(Some(version));
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Execute a pip command with the given configuration via remote connection
    async fn execute_pip_command(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        config: &PipConfig,
        args: &[&str],
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<(bool, String, String)> {
        let escaped_args: Vec<String> = args.iter().map(|a| shell_escape(a).into_owned()).collect();
        let extra = config.extra_args_string();

        let base_cmd = format!(
            "{} {}{}",
            config.build_command_string(),
            escaped_args.join(" "),
            if extra.is_empty() {
                String::new()
            } else {
                format!(" {}", extra)
            }
        );

        // Wrap with chdir and umask if needed
        let full_cmd = Self::wrap_command(&base_cmd, config);

        let result = conn.execute(&full_cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to execute pip command: {}", e))
        })?;

        Ok((result.success, result.stdout, result.stderr))
    }

    /// Wrap a command string with chdir and umask shell directives
    fn wrap_command(cmd: &str, config: &PipConfig) -> String {
        let mut parts = Vec::new();

        if let Some(ref chdir) = config.chdir {
            parts.push(format!("cd {} &&", shell_escape(chdir)));
        }

        if let Some(umask_val) = config.umask {
            parts.push(format!("umask {:04o} &&", umask_val));
        }

        parts.push(cmd.to_string());
        parts.join(" ")
    }

    /// Create a virtualenv if it doesn't exist, via remote connection
    async fn ensure_virtualenv(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        venv_path: &str,
        python: Option<&str>,
        site_packages: bool,
        virtualenv_command: Option<&str>,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<bool> {
        // Check if virtualenv exists by checking for the activate script on the remote host
        let check_cmd = format!("test -f {}/bin/activate", shell_escape(venv_path));
        if let Ok(result) = conn.execute(&check_cmd, options.clone()).await {
            if result.success {
                return Ok(false);
            }
        }

        // Determine the command to use for creating virtualenv
        let venv_cmd = virtualenv_command.unwrap_or("python3 -m venv");

        let mut parts: Vec<String> = venv_cmd.split_whitespace().map(|s| s.to_string()).collect();

        // Add system site-packages option if requested
        if site_packages {
            parts.push("--system-site-packages".to_string());
        }

        // Add python interpreter if specified (for virtualenv command, not venv)
        if let Some(py) = python {
            if venv_cmd.contains("virtualenv") {
                parts.push("--python".to_string());
                parts.push(shell_escape(py).into_owned());
            }
        }

        parts.push(shell_escape(venv_path).into_owned());

        let cmd = parts.join(" ");

        let result = conn.execute(&cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create virtualenv: {}", e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to create virtualenv: {}",
                result.stderr
            )));
        }

        Ok(true)
    }

    /// Handle requirements file installation via remote connection
    async fn handle_requirements(
        conn: &(dyn crate::connection::Connection + Send + Sync),
        config: &PipConfig,
        requirements: &str,
        state: &PipState,
        venv_created: bool,
        context: &ModuleContext,
        options: Option<ExecuteOptions>,
    ) -> ModuleResult<ModuleOutput> {
        if *state == PipState::Absent {
            return Err(ModuleError::InvalidParameter(
                "state=absent is not supported with requirements parameter".to_string(),
            ));
        }

        if context.check_mode {
            let mut msg = String::new();
            if venv_created {
                msg.push_str("Would create virtualenv. ");
            }
            msg.push_str(&format!(
                "Would install packages from requirements file: {}",
                requirements
            ));
            return Ok(ModuleOutput::changed(msg));
        }

        let mut args = vec!["install"];

        if *state == PipState::Latest {
            args.push("--upgrade");
        }

        if *state == PipState::ForceReinstall {
            args.push("--force-reinstall");
        }

        args.push("-r");
        args.push(requirements);

        let (success, stdout, stderr) =
            Self::execute_pip_command(conn, config, &args, options).await?;

        if !success {
            return Err(ModuleError::ExecutionFailed(format!(
                "Failed to install from requirements: {}",
                if stderr.is_empty() { stdout } else { stderr }
            )));
        }

        // Check if anything was actually installed by looking for "already satisfied" in output
        let changed =
            !stdout.contains("Requirement already satisfied") || *state == PipState::ForceReinstall;

        if changed || venv_created {
            Ok(ModuleOutput::changed(format!(
                "Installed packages from requirements file: {}",
                requirements
            ))
            .with_command_output(Some(stdout), Some(stderr), Some(0)))
        } else {
            Ok(
                ModuleOutput::ok("All requirements already satisfied".to_string())
                    .with_command_output(Some(stdout), Some(stderr), Some(0)),
            )
        }
    }

    /// Build package specification with version if provided
    fn build_package_spec(name: &str, version: Option<&str>) -> String {
        if let Some(ver) = version {
            // If version already contains a specifier, use as-is
            if ver.starts_with(&['>', '<', '=', '!', '~'][..]) {
                format!("{}{}", name, ver)
            } else {
                // Default to exact version match
                format!("{}=={}", name, ver)
            }
        } else {
            name.to_string()
        }
    }
}

impl Module for PipModule {
    fn name(&self) -> &'static str {
        "pip"
    }

    fn description(&self) -> &'static str {
        "Manage Python packages with pip"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Pip can generally run in parallel, but virtualenv operations might conflict
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        // Either 'name' or 'requirements' must be provided
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Must have either name or requirements
        if params.get("name").is_none() && params.get("requirements").is_none() {
            return Err(ModuleError::MissingParameter(
                "Either 'name' or 'requirements' must be provided".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Build configuration from parameters
        let config = PipConfig::from_params(params)?;

        // Get state
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = PipState::from_str(&state_str)?;

        // Get version specification
        let version = params.get_string("version")?;

        // Get connection from context
        let conn = context.connection.as_ref().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available in context. Pip module requires a remote connection."
                    .to_string(),
            )
        })?;

        // Build execution options with become/sudo
        let exec_options = Self::build_exec_options(context);

        // Parse virtualenv params before entering async block
        let venv = params.get_string("virtualenv")?;
        let venv_python = params.get_string("virtualenv_python")?;
        let site_packages = params
            .get_bool("virtualenv_site_packages")?
            .unwrap_or(false);
        let venv_command = params.get_string("virtualenv_command")?;
        let requirements = params.get_string("requirements")?;
        let check_mode = context.check_mode;

        // Handle individual packages
        let packages: Vec<String> = if let Some(names) = params.get_vec_string("name")? {
            names
        } else if params.get("name").is_some() {
            vec![params.get_string_required("name")?]
        } else {
            Vec::new()
        };

        // Build package specs with version if provided
        let package_specs: Vec<String> = packages
            .iter()
            .map(|p| Self::build_package_spec(p, version.as_deref()))
            .collect();

        // Use tokio runtime to execute async operations
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Handle virtualenv creation if needed
                let mut venv_created = false;
                if let Some(ref venv_path) = venv {
                    if !check_mode {
                        venv_created = Self::ensure_virtualenv(
                            conn.as_ref(),
                            venv_path,
                            venv_python.as_deref(),
                            site_packages,
                            venv_command.as_deref(),
                            Some(exec_options.clone()),
                        )
                        .await?;
                    }
                }

                // Handle requirements file
                if let Some(ref req_file) = requirements {
                    return Self::handle_requirements(
                        conn.as_ref(),
                        &config,
                        req_file,
                        &state,
                        venv_created,
                        context,
                        Some(exec_options.clone()),
                    )
                    .await;
                }

                let mut to_install: Vec<String> = Vec::new();
                let mut to_remove: Vec<String> = Vec::new();
                let mut already_ok: Vec<String> = Vec::new();

                for (package, spec) in packages.iter().zip(package_specs.iter()) {
                    let is_installed = Self::is_package_installed(
                        conn.as_ref(),
                        &config,
                        package,
                        Some(exec_options.clone()),
                    )
                    .await?;

                    match state {
                        PipState::Present => {
                            if is_installed && version.is_none() {
                                already_ok.push(package.clone());
                            } else if is_installed && version.is_some() {
                                // Check if the installed version matches
                                let installed_ver = Self::get_installed_version(
                                    conn.as_ref(),
                                    &config,
                                    package,
                                    Some(exec_options.clone()),
                                )
                                .await?;
                                if let (Some(inst_ver), Some(req_ver)) = (installed_ver, &version) {
                                    // Simple exact match check - for complex version specs, always upgrade
                                    if inst_ver == *req_ver
                                        || req_ver.starts_with(&['>', '<', '!', '~'][..])
                                    {
                                        already_ok.push(package.clone());
                                    } else if inst_ver != *req_ver {
                                        to_install.push(spec.clone());
                                    }
                                } else {
                                    to_install.push(spec.clone());
                                }
                            } else {
                                to_install.push(spec.clone());
                            }
                        }
                        PipState::Absent => {
                            if is_installed {
                                to_remove.push(package.clone());
                            } else {
                                already_ok.push(package.clone());
                            }
                        }
                        PipState::Latest | PipState::ForceReinstall => {
                            // For 'latest' or 'forcereinstall', we always try to install/upgrade
                            to_install.push(spec.clone());
                        }
                    }
                }

                // Check mode - return what would happen
                if check_mode {
                    if to_install.is_empty() && to_remove.is_empty() && !venv_created {
                        return Ok(ModuleOutput::ok(format!(
                            "All packages already in desired state: {}",
                            already_ok.join(", ")
                        )));
                    }

                    let mut msg = String::new();
                    if venv_created {
                        msg.push_str("Would create virtualenv. ");
                    }
                    if !to_install.is_empty() {
                        msg.push_str(&format!("Would install: {}. ", to_install.join(", ")));
                    }
                    if !to_remove.is_empty() {
                        msg.push_str(&format!("Would remove: {}. ", to_remove.join(", ")));
                    }

                    return Ok(ModuleOutput::changed(msg.trim().to_string()));
                }

                // Perform the actual operations
                let mut changed = venv_created;
                let mut results: HashMap<String, String> = HashMap::new();

                if !to_install.is_empty() {
                    let mut args: Vec<&str> = vec!["install"];

                    // Add state-specific flags
                    match state {
                        PipState::Latest => {
                            args.push("--upgrade");
                        }
                        PipState::ForceReinstall => {
                            args.push("--force-reinstall");
                        }
                        _ => {}
                    }

                    // Add editable flag if requested
                    if config.editable {
                        args.push("-e");
                    }

                    // Convert to refs for the command
                    let pkg_refs: Vec<&str> = to_install.iter().map(|s| s.as_str()).collect();
                    args.extend(pkg_refs);

                    let (success, stdout, stderr) = Self::execute_pip_command(
                        conn.as_ref(),
                        &config,
                        &args,
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
                    for pkg in &to_install {
                        let pkg_name = Self::extract_package_name(pkg);
                        results.insert(pkg_name, "installed".to_string());
                    }
                }

                if !to_remove.is_empty() {
                    let mut args: Vec<&str> = vec!["uninstall", "-y"];
                    let pkg_refs: Vec<&str> = to_remove.iter().map(|s| s.as_str()).collect();
                    args.extend(pkg_refs);

                    let (success, stdout, stderr) = Self::execute_pip_command(
                        conn.as_ref(),
                        &config,
                        &args,
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
                    for pkg in &to_remove {
                        results.insert(pkg.clone(), "removed".to_string());
                    }
                }

                for pkg in &already_ok {
                    results.insert(pkg.clone(), "ok".to_string());
                }

                if changed {
                    let mut msg = String::new();
                    if venv_created {
                        msg.push_str("Virtualenv created. ");
                    }
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
    fn test_pip_state_from_str() {
        assert_eq!(PipState::from_str("present").unwrap(), PipState::Present);
        assert_eq!(PipState::from_str("installed").unwrap(), PipState::Present);
        assert_eq!(PipState::from_str("absent").unwrap(), PipState::Absent);
        assert_eq!(PipState::from_str("removed").unwrap(), PipState::Absent);
        assert_eq!(PipState::from_str("latest").unwrap(), PipState::Latest);
        assert_eq!(
            PipState::from_str("forcereinstall").unwrap(),
            PipState::ForceReinstall
        );
        assert_eq!(
            PipState::from_str("force-reinstall").unwrap(),
            PipState::ForceReinstall
        );
        assert!(PipState::from_str("invalid").is_err());
    }

    #[test]
    fn test_pip_config_from_params() {
        let mut params: ModuleParams = HashMap::new();

        // Default configuration
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(config.pip_cmd, "pip3");
        assert!(config.extra_args.is_empty());
        assert!(config.chdir.is_none());
        assert!(!config.editable);
        assert!(config.proxy.is_none());

        // Custom executable
        params.insert("executable".to_string(), serde_json::json!("pip"));
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(config.pip_cmd, "pip");

        // Virtualenv overrides executable
        params.insert("virtualenv".to_string(), serde_json::json!("/path/to/venv"));
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(config.pip_cmd, "/path/to/venv/bin/pip");
    }

    #[test]
    fn test_pip_config_extra_args() {
        let mut params: ModuleParams = HashMap::new();

        // Extra args as string
        params.insert(
            "extra_args".to_string(),
            serde_json::json!("--trusted-host pypi.example.com --no-cache-dir"),
        );
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(
            config.extra_args,
            vec!["--trusted-host", "pypi.example.com", "--no-cache-dir"]
        );

        // Extra args as array
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "extra_args".to_string(),
            serde_json::json!(["--trusted-host", "pypi.example.com"]),
        );
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(
            config.extra_args,
            vec!["--trusted-host", "pypi.example.com"]
        );
    }

    #[test]
    fn test_pip_config_proxy() {
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "proxy".to_string(),
            serde_json::json!("http://proxy.example.com:8080"),
        );
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(
            config.proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
    }

    #[test]
    fn test_pip_config_index_urls() {
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "index_url".to_string(),
            serde_json::json!("https://pypi.example.com/simple"),
        );
        params.insert(
            "extra_index_url".to_string(),
            serde_json::json!("https://pypi.org/simple"),
        );
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(
            config.index_url,
            Some("https://pypi.example.com/simple".to_string())
        );
        assert_eq!(
            config.extra_index_url,
            Some("https://pypi.org/simple".to_string())
        );
    }

    #[test]
    fn test_pip_config_umask() {
        // Umask as string
        let mut params: ModuleParams = HashMap::new();
        params.insert("umask".to_string(), serde_json::json!("0022"));
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(config.umask, Some(0o022));

        // Umask with 0o prefix
        let mut params: ModuleParams = HashMap::new();
        params.insert("umask".to_string(), serde_json::json!("0o077"));
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(config.umask, Some(0o077));

        // Umask as integer
        let mut params: ModuleParams = HashMap::new();
        params.insert("umask".to_string(), serde_json::json!(18)); // 0o022 in decimal
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(config.umask, Some(18));
    }

    #[test]
    fn test_extract_package_name() {
        assert_eq!(PipModule::extract_package_name("flask"), "flask");
        assert_eq!(PipModule::extract_package_name("flask>=2.0"), "flask");
        assert_eq!(PipModule::extract_package_name("django==4.2"), "django");
        assert_eq!(
            PipModule::extract_package_name("requests[security]>=2.0"),
            "requests"
        );
        assert_eq!(PipModule::extract_package_name("package~=1.0"), "package");
        assert_eq!(PipModule::extract_package_name("pkg!=1.0"), "pkg");
        assert_eq!(PipModule::extract_package_name("pkg<2.0,>=1.0"), "pkg");
    }

    #[test]
    fn test_build_package_spec() {
        // No version
        assert_eq!(PipModule::build_package_spec("flask", None), "flask");

        // Simple version
        assert_eq!(
            PipModule::build_package_spec("flask", Some("2.0")),
            "flask==2.0"
        );

        // Version with specifier
        assert_eq!(
            PipModule::build_package_spec("flask", Some(">=2.0")),
            "flask>=2.0"
        );
        assert_eq!(
            PipModule::build_package_spec("django", Some("<4.0,>=3.2")),
            "django<4.0,>=3.2"
        );
        assert_eq!(
            PipModule::build_package_spec("requests", Some("~=2.28")),
            "requests~=2.28"
        );
    }

    #[test]
    fn test_validate_params() {
        let module = PipModule;

        // Missing both name and requirements
        let params: ModuleParams = HashMap::new();
        assert!(module.validate_params(&params).is_err());

        // Has name
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("requests"));
        assert!(module.validate_params(&params).is_ok());

        // Has requirements
        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "requirements".to_string(),
            serde_json::json!("requirements.txt"),
        );
        assert!(module.validate_params(&params).is_ok());
    }

    #[test]
    fn test_pip_config_editable() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("editable".to_string(), serde_json::json!(true));
        let config = PipConfig::from_params(&params).unwrap();
        assert!(config.editable);
    }

    #[test]
    fn test_pip_config_chdir() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("chdir".to_string(), serde_json::json!("/opt/myapp"));
        let config = PipConfig::from_params(&params).unwrap();
        assert_eq!(config.chdir, Some("/opt/myapp".to_string()));
    }

    #[test]
    fn test_pip_config_no_index() {
        let mut params: ModuleParams = HashMap::new();
        params.insert("no_index".to_string(), serde_json::json!(true));
        params.insert(
            "find_links".to_string(),
            serde_json::json!("/path/to/packages"),
        );
        let config = PipConfig::from_params(&params).unwrap();
        assert!(config.no_index);
        assert_eq!(config.find_links, Some("/path/to/packages".to_string()));
    }
}
