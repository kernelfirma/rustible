//! Docker Compose module - Docker Compose project management
//!
//! This module manages Docker Compose projects using the bollard crate.
//! It supports deploying, starting, stopping, and removing Compose stacks.
//!
//! Note: This module executes docker-compose/docker compose CLI commands
//! as bollard doesn't have native Compose API support.
//!
//! ## Parameters
//!
//! - `project_src`: Path to docker-compose.yml directory (required unless definition is provided)
//! - `project_name`: Compose project name (default: directory name)
//! - `state`: Desired state (present, absent, restarted)
//! - `files`: List of compose files to use (default: docker-compose.yml)
//! - `services`: List of specific services to operate on (default: all)
//! - `definition`: Inline docker-compose definition (YAML)
//! - `build`: Build images before starting
//! - `pull`: Pull images before starting (always, missing, never)
//! - `recreate`: Recreate containers (always, never, smart)
//! - `remove_orphans`: Remove containers not defined in compose file
//! - `remove_images`: Remove images when stopping (all, local)
//! - `remove_volumes`: Remove volumes when stopping
//! - `timeout`: Timeout in seconds for container operations
//! - `scale`: Service scaling (service: count)
//! - `env_file`: Path to environment file
//! - `profiles`: Compose profiles to enable

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Desired state for a Compose project
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComposeState {
    /// Project should be running
    Present,
    /// Project should not exist
    Absent,
    /// Project should be restarted
    Restarted,
    /// Project should be stopped (but not removed)
    Stopped,
}

impl ComposeState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" | "started" | "up" => Ok(ComposeState::Present),
            "absent" | "removed" | "down" => Ok(ComposeState::Absent),
            "restarted" => Ok(ComposeState::Restarted),
            "stopped" => Ok(ComposeState::Stopped),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, restarted, stopped",
                s
            ))),
        }
    }
}

/// Pull policy for Compose
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ComposePullPolicy {
    /// Always pull images
    Always,
    /// Pull if image is missing (default)
    #[default]
    Missing,
    /// Never pull images
    Never,
}

impl ComposePullPolicy {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "always" => Ok(ComposePullPolicy::Always),
            "missing" | "if_not_present" => Ok(ComposePullPolicy::Missing),
            "never" => Ok(ComposePullPolicy::Never),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid pull policy '{}'. Valid values: always, missing, never",
                s
            ))),
        }
    }

    fn as_arg(&self) -> Option<&'static str> {
        match self {
            ComposePullPolicy::Always => Some("--pull=always"),
            ComposePullPolicy::Missing => None, // default behavior
            ComposePullPolicy::Never => Some("--pull=never"),
        }
    }
}

/// Recreate policy for Compose
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RecreatePolicy {
    /// Always recreate containers
    Always,
    /// Never recreate containers
    Never,
    /// Recreate only if config changed (default)
    #[default]
    Smart,
}

impl RecreatePolicy {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "always" => Ok(RecreatePolicy::Always),
            "never" => Ok(RecreatePolicy::Never),
            "smart" | "auto" => Ok(RecreatePolicy::Smart),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid recreate policy '{}'. Valid values: always, never, smart",
                s
            ))),
        }
    }
}

/// Remove images option
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveImages {
    /// Remove all images
    All,
    /// Remove only local images
    Local,
}

impl RemoveImages {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "all" => Ok(RemoveImages::All),
            "local" => Ok(RemoveImages::Local),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid remove_images value '{}'. Valid values: all, local",
                s
            ))),
        }
    }

    fn as_arg(&self) -> &'static str {
        match self {
            RemoveImages::All => "all",
            RemoveImages::Local => "local",
        }
    }
}

/// Compose configuration parsed from parameters
#[derive(Debug, Clone)]
pub struct ComposeConfig {
    /// Project source directory
    pub project_src: Option<String>,
    /// Project name
    pub project_name: Option<String>,
    /// Desired state
    pub state: ComposeState,
    /// Compose files to use
    pub files: Vec<String>,
    /// Specific services to operate on
    pub services: Vec<String>,
    /// Inline compose definition
    pub definition: Option<String>,
    /// Build images
    pub build: bool,
    /// Pull policy
    pub pull: ComposePullPolicy,
    /// Recreate policy
    pub recreate: RecreatePolicy,
    /// Remove orphan containers
    pub remove_orphans: bool,
    /// Remove images when down
    pub remove_images: Option<RemoveImages>,
    /// Remove volumes when down
    pub remove_volumes: bool,
    /// Operation timeout
    pub timeout: Option<i64>,
    /// Service scaling
    pub scale: HashMap<String, u32>,
    /// Environment file
    pub env_file: Option<String>,
    /// Compose profiles
    pub profiles: Vec<String>,
    /// Detach after up
    pub detach: bool,
    /// Wait for services to be healthy
    pub wait: bool,
}

impl ComposeConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let state = if let Some(s) = params.get_string("state")? {
            ComposeState::from_str(&s)?
        } else {
            ComposeState::Present
        };

        let pull = if let Some(p) = params.get_string("pull")? {
            ComposePullPolicy::from_str(&p)?
        } else {
            ComposePullPolicy::default()
        };

        let recreate = if let Some(r) = params.get_string("recreate")? {
            RecreatePolicy::from_str(&r)?
        } else {
            RecreatePolicy::default()
        };

        let remove_images = if let Some(r) = params.get_string("remove_images")? {
            Some(RemoveImages::from_str(&r)?)
        } else {
            None
        };

        // Parse scale
        let mut scale = HashMap::new();
        if let Some(serde_json::Value::Object(obj)) = params.get("scale") {
            for (service, count) in obj {
                if let Some(n) = count.as_u64() {
                    scale.insert(service.clone(), n as u32);
                }
            }
        }

        // Parse files
        let files = params
            .get_vec_string("files")?
            .unwrap_or_else(|| vec!["docker-compose.yml".to_string()]);

        // Parse services
        let services = params.get_vec_string("services")?.unwrap_or_default();

        // Parse profiles
        let profiles = params.get_vec_string("profiles")?.unwrap_or_default();

        Ok(Self {
            project_src: params.get_string("project_src")?,
            project_name: params.get_string("project_name")?,
            state,
            files,
            services,
            definition: params.get_string("definition")?,
            build: params.get_bool_or("build", false),
            pull,
            recreate,
            remove_orphans: params.get_bool_or("remove_orphans", true),
            remove_images,
            remove_volumes: params.get_bool_or("remove_volumes", false),
            timeout: params.get_i64("timeout")?,
            scale,
            env_file: params.get_string("env_file")?,
            profiles,
            detach: params.get_bool_or("detach", true),
            wait: params.get_bool_or("wait", false),
        })
    }
}

/// Docker Compose module
pub struct DockerComposeModule;

impl DockerComposeModule {
    /// Detect docker compose command (docker-compose or docker compose)
    async fn detect_compose_command() -> ModuleResult<Vec<String>> {
        // Try "docker compose" first (Docker Compose V2)
        let output = Command::new("docker")
            .args(["compose", "version"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        if let Ok(output) = output {
            if output.status.success() {
                return Ok(vec!["docker".to_string(), "compose".to_string()]);
            }
        }

        // Try "docker-compose" (Docker Compose V1)
        let output = Command::new("docker-compose")
            .arg("version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await;

        if let Ok(output) = output {
            if output.status.success() {
                return Ok(vec!["docker-compose".to_string()]);
            }
        }

        Err(ModuleError::ExecutionFailed(
            "Docker Compose not found. Install docker-compose or Docker Compose plugin."
                .to_string(),
        ))
    }

    /// Build compose command with common options
    fn build_base_command(cmd: &[String], config: &ComposeConfig, work_dir: &Path) -> Command {
        let mut command = if cmd.len() == 1 {
            Command::new(&cmd[0])
        } else {
            let mut c = Command::new(&cmd[0]);
            c.arg(&cmd[1]);
            c
        };

        command.current_dir(work_dir);

        // Add project name if specified
        if let Some(ref name) = config.project_name {
            command.args(["--project-name", name]);
        }

        // Add compose files
        for file in &config.files {
            command.args(["--file", file]);
        }

        // Add env file
        if let Some(ref env_file) = config.env_file {
            command.args(["--env-file", env_file]);
        }

        // Add profiles
        for profile in &config.profiles {
            command.args(["--profile", profile]);
        }

        command
    }

    /// Execute docker compose up
    async fn compose_up(
        compose_cmd: &[String],
        config: &ComposeConfig,
        work_dir: &Path,
    ) -> ModuleResult<(bool, String)> {
        let mut cmd = Self::build_base_command(compose_cmd, config, work_dir);
        cmd.arg("up");

        // Add options
        if config.detach {
            cmd.arg("-d");
        }

        if config.build {
            cmd.arg("--build");
        }

        if let Some(arg) = config.pull.as_arg() {
            cmd.arg(arg);
        }

        match config.recreate {
            RecreatePolicy::Always => {
                cmd.arg("--force-recreate");
            }
            RecreatePolicy::Never => {
                cmd.arg("--no-recreate");
            }
            RecreatePolicy::Smart => {
                // Default behavior
            }
        }

        if config.remove_orphans {
            cmd.arg("--remove-orphans");
        }

        if config.wait {
            cmd.arg("--wait");
        }

        // Add scale
        for (service, count) in &config.scale {
            cmd.args(["--scale", &format!("{}={}", service, count)]);
        }

        // Add timeout
        if let Some(timeout) = config.timeout {
            cmd.args(["--timeout", &timeout.to_string()]);
        }

        // Add specific services
        for service in &config.services {
            cmd.arg(service);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to run docker compose up: {}", e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "docker compose up failed: {}",
                if stderr.is_empty() { &stdout } else { &stderr }
            )));
        }

        // Check if anything was created/recreated
        let changed = stdout.contains("Created")
            || stdout.contains("Recreated")
            || stdout.contains("Starting")
            || stderr.contains("Created")
            || stderr.contains("Recreated");

        Ok((changed, format!("{}{}", stdout, stderr)))
    }

    /// Execute docker compose down
    async fn compose_down(
        compose_cmd: &[String],
        config: &ComposeConfig,
        work_dir: &Path,
    ) -> ModuleResult<(bool, String)> {
        let mut cmd = Self::build_base_command(compose_cmd, config, work_dir);
        cmd.arg("down");

        // Add options
        if let Some(ref remove_images) = config.remove_images {
            cmd.args(["--rmi", remove_images.as_arg()]);
        }

        if config.remove_volumes {
            cmd.arg("--volumes");
        }

        if config.remove_orphans {
            cmd.arg("--remove-orphans");
        }

        if let Some(timeout) = config.timeout {
            cmd.args(["--timeout", &timeout.to_string()]);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to run docker compose down: {}", e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "docker compose down failed: {}",
                if stderr.is_empty() { &stdout } else { &stderr }
            )));
        }

        let changed = stdout.contains("Removed")
            || stdout.contains("Stopping")
            || stderr.contains("Removed")
            || stderr.contains("Stopping");

        Ok((changed, format!("{}{}", stdout, stderr)))
    }

    /// Execute docker compose stop
    async fn compose_stop(
        compose_cmd: &[String],
        config: &ComposeConfig,
        work_dir: &Path,
    ) -> ModuleResult<(bool, String)> {
        let mut cmd = Self::build_base_command(compose_cmd, config, work_dir);
        cmd.arg("stop");

        if let Some(timeout) = config.timeout {
            cmd.args(["--timeout", &timeout.to_string()]);
        }

        // Add specific services
        for service in &config.services {
            cmd.arg(service);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to run docker compose stop: {}", e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "docker compose stop failed: {}",
                if stderr.is_empty() { &stdout } else { &stderr }
            )));
        }

        let changed = stdout.contains("Stopped") || stderr.contains("Stopped");

        Ok((changed, format!("{}{}", stdout, stderr)))
    }

    /// Execute docker compose restart
    async fn compose_restart(
        compose_cmd: &[String],
        config: &ComposeConfig,
        work_dir: &Path,
    ) -> ModuleResult<(bool, String)> {
        let mut cmd = Self::build_base_command(compose_cmd, config, work_dir);
        cmd.arg("restart");

        if let Some(timeout) = config.timeout {
            cmd.args(["--timeout", &timeout.to_string()]);
        }

        // Add specific services
        for service in &config.services {
            cmd.arg(service);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to run docker compose restart: {}", e))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            return Err(ModuleError::ExecutionFailed(format!(
                "docker compose restart failed: {}",
                if stderr.is_empty() { &stdout } else { &stderr }
            )));
        }

        Ok((true, format!("{}{}", stdout, stderr)))
    }

    /// Get compose project info
    async fn get_project_info(
        compose_cmd: &[String],
        config: &ComposeConfig,
        work_dir: &Path,
    ) -> ModuleResult<serde_json::Value> {
        let mut cmd = Self::build_base_command(compose_cmd, config, work_dir);
        cmd.args(["ps", "--format", "json"]);

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().await;

        match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse JSON output - each line is a JSON object
                let services: Vec<serde_json::Value> = stdout
                    .lines()
                    .filter_map(|line| serde_json::from_str(line).ok())
                    .collect();

                Ok(serde_json::json!({
                    "services": services,
                    "running": !services.is_empty(),
                }))
            }
            _ => Ok(serde_json::json!({
                "running": false,
            })),
        }
    }

    /// Create temporary compose file from definition
    async fn create_temp_compose_file(definition: &str) -> ModuleResult<tempfile::TempDir> {
        let temp_dir = tempfile::TempDir::new().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to create temp directory: {}", e))
        })?;

        let compose_path = temp_dir.path().join("docker-compose.yml");
        std::fs::write(&compose_path, definition).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to write compose file: {}", e))
        })?;

        Ok(temp_dir)
    }

    /// Execute the module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ComposeConfig::from_params(params)?;

        // Detect docker compose command
        let compose_cmd = Self::detect_compose_command().await?;

        // Determine work directory
        // Note: temp_dir must stay in scope to keep the temp directory alive
        #[allow(unused_assignments)]
        let mut temp_dir: Option<tempfile::TempDir> = None;
        let work_dir = if let Some(ref definition) = config.definition {
            temp_dir = Some(Self::create_temp_compose_file(definition).await?);
            temp_dir.as_ref().unwrap().path().to_path_buf()
        } else if let Some(ref project_src) = config.project_src {
            // temp_dir stays None - no temp directory needed
            Path::new(project_src).to_path_buf()
        } else {
            return Err(ModuleError::MissingParameter(
                "Either 'project_src' or 'definition' must be provided".to_string(),
            ));
        };

        if !work_dir.exists() {
            return Err(ModuleError::ExecutionFailed(format!(
                "Project directory does not exist: {}",
                work_dir.display()
            )));
        }

        #[allow(unused_assignments)]
        let mut changed = false;
        let mut messages = Vec::new();

        match config.state {
            ComposeState::Present => {
                if context.check_mode {
                    messages.push("Would run docker compose up".to_string());
                    changed = true;
                } else {
                    let (did_change, _output) =
                        Self::compose_up(&compose_cmd, &config, &work_dir).await?;
                    if did_change {
                        messages.push("Started/updated compose project".to_string());
                    } else {
                        messages.push("Compose project is already running".to_string());
                    }
                    changed = did_change;
                }
            }

            ComposeState::Absent => {
                if context.check_mode {
                    messages.push("Would run docker compose down".to_string());
                    changed = true;
                } else {
                    let (did_change, _) =
                        Self::compose_down(&compose_cmd, &config, &work_dir).await?;
                    if did_change {
                        messages.push("Stopped and removed compose project".to_string());
                    } else {
                        messages.push("Compose project is not running".to_string());
                    }
                    changed = did_change;
                }
            }

            ComposeState::Stopped => {
                if context.check_mode {
                    messages.push("Would run docker compose stop".to_string());
                    changed = true;
                } else {
                    let (did_change, _) =
                        Self::compose_stop(&compose_cmd, &config, &work_dir).await?;
                    if did_change {
                        messages.push("Stopped compose project".to_string());
                    } else {
                        messages.push("Compose project is already stopped".to_string());
                    }
                    changed = did_change;
                }
            }

            ComposeState::Restarted => {
                if context.check_mode {
                    messages.push("Would run docker compose restart".to_string());
                    changed = true;
                } else {
                    let (did_change, _) =
                        Self::compose_restart(&compose_cmd, &config, &work_dir).await?;
                    if did_change {
                        messages.push("Restarted compose project".to_string());
                    }
                    changed = did_change;
                }
            }
        }

        // Get project info
        let project_info = Self::get_project_info(&compose_cmd, &config, &work_dir).await?;

        let msg = if messages.is_empty() {
            "Compose project is in desired state".to_string()
        } else {
            messages.join(". ")
        };

        if changed {
            Ok(ModuleOutput::changed(msg).with_data("project", project_info))
        } else {
            Ok(ModuleOutput::ok(msg).with_data("project", project_info))
        }
    }
}

impl Module for DockerComposeModule {
    fn name(&self) -> &'static str {
        "docker_compose"
    }

    fn description(&self) -> &'static str {
        "Manage Docker Compose projects"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Compose operations should be exclusive per project
        ParallelizationHint::HostExclusive
    }

    fn required_params(&self) -> &[&'static str] {
        &[] // Either project_src or definition is required
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        if params.get("project_src").is_none() && params.get("definition").is_none() {
            return Err(ModuleError::MissingParameter(
                "Either 'project_src' or 'definition' must be provided".to_string(),
            ));
        }
        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        std::thread::scope(|s| {
            s.spawn(|| rt.block_on(self.execute_async(&params, &context)))
                .join()
                .unwrap()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compose_state_from_str() {
        assert_eq!(
            ComposeState::from_str("present").unwrap(),
            ComposeState::Present
        );
        assert_eq!(
            ComposeState::from_str("started").unwrap(),
            ComposeState::Present
        );
        assert_eq!(ComposeState::from_str("up").unwrap(), ComposeState::Present);
        assert_eq!(
            ComposeState::from_str("absent").unwrap(),
            ComposeState::Absent
        );
        assert_eq!(
            ComposeState::from_str("down").unwrap(),
            ComposeState::Absent
        );
        assert_eq!(
            ComposeState::from_str("stopped").unwrap(),
            ComposeState::Stopped
        );
        assert_eq!(
            ComposeState::from_str("restarted").unwrap(),
            ComposeState::Restarted
        );
        assert!(ComposeState::from_str("invalid").is_err());
    }

    #[test]
    fn test_pull_policy_from_str() {
        assert_eq!(
            ComposePullPolicy::from_str("always").unwrap(),
            ComposePullPolicy::Always
        );
        assert_eq!(
            ComposePullPolicy::from_str("missing").unwrap(),
            ComposePullPolicy::Missing
        );
        assert_eq!(
            ComposePullPolicy::from_str("never").unwrap(),
            ComposePullPolicy::Never
        );
        assert!(ComposePullPolicy::from_str("invalid").is_err());
    }

    #[test]
    fn test_recreate_policy_from_str() {
        assert_eq!(
            RecreatePolicy::from_str("always").unwrap(),
            RecreatePolicy::Always
        );
        assert_eq!(
            RecreatePolicy::from_str("never").unwrap(),
            RecreatePolicy::Never
        );
        assert_eq!(
            RecreatePolicy::from_str("smart").unwrap(),
            RecreatePolicy::Smart
        );
        assert!(RecreatePolicy::from_str("invalid").is_err());
    }

    #[test]
    fn test_remove_images_from_str() {
        assert_eq!(RemoveImages::from_str("all").unwrap(), RemoveImages::All);
        assert_eq!(
            RemoveImages::from_str("local").unwrap(),
            RemoveImages::Local
        );
        assert!(RemoveImages::from_str("invalid").is_err());
    }

    #[test]
    fn test_module_metadata() {
        let module = DockerComposeModule;
        assert_eq!(module.name(), "docker_compose");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
    }
}
