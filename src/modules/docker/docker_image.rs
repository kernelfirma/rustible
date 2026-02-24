//! Docker Image module - Image management
//!
//! This module manages Docker images using the bollard crate.
//! It supports pulling, building, tagging, pushing, and removing images.
//!
//! ## Parameters
//!
//! - `name`: Image name (required)
//! - `tag`: Image tag (default: latest)
//! - `state`: Desired state (present, absent, build)
//! - `source`: Source for image (pull, build, load)
//! - `build`: Build configuration (for source=build)
//!   - `path`: Path to Dockerfile directory
//!   - `dockerfile`: Dockerfile name (default: Dockerfile)
//!   - `args`: Build arguments
//!   - `nocache`: Disable cache
//!   - `pull`: Always pull base image
//!   - `target`: Build target stage
//! - `push`: Push image to registry
//! - `force`: Force removal of image
//! - `archive_path`: Path to tar archive for load/save
//! - `repository`: Registry repository for push

#[cfg(feature = "docker")]
use bollard::image::{BuildImageOptions, CreateImageOptions, RemoveImageOptions, TagImageOptions};
#[cfg(feature = "docker")]
use bollard::Docker;
#[cfg(feature = "docker")]
use futures::StreamExt;

use crate::modules::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Desired state for an image
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageState {
    /// Image should exist
    Present,
    /// Image should not exist
    Absent,
    /// Image should be built
    Build,
}

impl ImageState {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(ImageState::Present),
            "absent" => Ok(ImageState::Absent),
            "build" => Ok(ImageState::Build),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent, build",
                s
            ))),
        }
    }
}

/// Image source
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ImageSource {
    /// Pull from registry (default)
    #[default]
    Pull,
    /// Build from Dockerfile
    Build,
    /// Load from tar archive
    Load,
    /// Use local image (no pull)
    Local,
}

impl ImageSource {
    fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "pull" => Ok(ImageSource::Pull),
            "build" => Ok(ImageSource::Build),
            "load" => Ok(ImageSource::Load),
            "local" => Ok(ImageSource::Local),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid source '{}'. Valid values: pull, build, load, local",
                s
            ))),
        }
    }
}

/// Build configuration
#[derive(Debug, Clone, Default)]
pub struct BuildConfig {
    /// Path to build context
    pub path: Option<String>,
    /// Dockerfile name
    pub dockerfile: String,
    /// Build arguments
    pub args: HashMap<String, String>,
    /// Disable build cache
    pub nocache: bool,
    /// Always pull base images
    pub pull: bool,
    /// Target build stage
    pub target: Option<String>,
    /// Remove intermediate containers
    pub rm: bool,
    /// Force remove intermediate containers
    pub forcerm: bool,
    /// Labels to apply to image
    pub labels: HashMap<String, String>,
}

/// Image configuration parsed from parameters
#[derive(Debug, Clone)]
pub struct ImageConfig {
    /// Image name
    pub name: String,
    /// Image tag
    pub tag: String,
    /// Desired state
    pub state: ImageState,
    /// Image source
    pub source: ImageSource,
    /// Build configuration
    pub build: BuildConfig,
    /// Push to registry
    pub push: bool,
    /// Force removal
    pub force: bool,
    /// Archive path for load/save
    pub archive_path: Option<String>,
    /// Registry repository for push
    pub repository: Option<String>,
}

impl ImageConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;
        let tag = params
            .get_string("tag")?
            .unwrap_or_else(|| "latest".to_string());

        let state = if let Some(s) = params.get_string("state")? {
            ImageState::from_str(&s)?
        } else {
            ImageState::Present
        };

        let source = if let Some(s) = params.get_string("source")? {
            ImageSource::from_str(&s)?
        } else {
            ImageSource::default()
        };

        // Parse build configuration
        let build = if let Some(serde_json::Value::Object(obj)) = params.get("build") {
            let mut args = HashMap::new();
            if let Some(serde_json::Value::Object(build_args)) = obj.get("args") {
                for (k, v) in build_args {
                    if let serde_json::Value::String(val) = v {
                        args.insert(k.clone(), val.clone());
                    }
                }
            }

            let mut labels = HashMap::new();
            if let Some(serde_json::Value::Object(build_labels)) = obj.get("labels") {
                for (k, v) in build_labels {
                    if let serde_json::Value::String(val) = v {
                        labels.insert(k.clone(), val.clone());
                    }
                }
            }

            BuildConfig {
                path: obj.get("path").and_then(|v| v.as_str()).map(String::from),
                dockerfile: obj
                    .get("dockerfile")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Dockerfile")
                    .to_string(),
                args,
                nocache: obj
                    .get("nocache")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                pull: obj.get("pull").and_then(|v| v.as_bool()).unwrap_or(false),
                target: obj.get("target").and_then(|v| v.as_str()).map(String::from),
                rm: obj.get("rm").and_then(|v| v.as_bool()).unwrap_or(true),
                forcerm: obj
                    .get("forcerm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                labels,
            }
        } else {
            BuildConfig {
                path: params.get_string("build_path")?,
                dockerfile: params
                    .get_string("dockerfile")?
                    .unwrap_or_else(|| "Dockerfile".to_string()),
                ..Default::default()
            }
        };

        Ok(Self {
            name,
            tag,
            state,
            source,
            build,
            push: params.get_bool_or("push", false),
            force: params.get_bool_or("force", false),
            archive_path: params.get_string("archive_path")?,
            repository: params.get_string("repository")?,
        })
    }

    /// Get full image reference (name:tag)
    fn full_reference(&self) -> String {
        format!("{}:{}", self.name, self.tag)
    }
}

/// Docker Image module
pub struct DockerImageModule;

#[cfg(feature = "docker")]
impl DockerImageModule {
    /// Connect to Docker daemon
    async fn connect_docker() -> ModuleResult<Docker> {
        Docker::connect_with_local_defaults().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to connect to Docker: {}", e))
        })
    }

    /// Check if image exists locally
    async fn image_exists(docker: &Docker, name: &str, tag: &str) -> ModuleResult<bool> {
        let reference = format!("{}:{}", name, tag);
        match docker.inspect_image(&reference).await {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(ModuleError::ExecutionFailed(format!(
                "Failed to inspect image: {}",
                e
            ))),
        }
    }

    /// Pull image from registry
    async fn pull_image(docker: &Docker, name: &str, tag: &str) -> ModuleResult<()> {
        let options = CreateImageOptions {
            from_image: name,
            tag,
            ..Default::default()
        };

        let mut stream = docker.create_image(Some(options), None, None);
        while let Some(result) = stream.next().await {
            result.map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to pull image: {}", e))
            })?;
        }
        Ok(())
    }

    /// Build image from Dockerfile
    async fn build_image(docker: &Docker, config: &ImageConfig) -> ModuleResult<()> {
        use tar::Builder;

        let build_path = config.build.path.as_ref().ok_or_else(|| {
            ModuleError::MissingParameter("build.path is required for building images".to_string())
        })?;

        // Create tar archive of build context
        let mut tar_data = Vec::new();
        {
            let mut tar = Builder::new(&mut tar_data);
            tar.append_dir_all(".", build_path).map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to create build context: {}", e))
            })?;
            tar.finish().map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to finalize tar: {}", e))
            })?;
        }

        let options = BuildImageOptions {
            t: config.full_reference(),
            dockerfile: config.build.dockerfile.clone(),
            nocache: config.build.nocache,
            pull: config.build.pull,
            rm: config.build.rm,
            forcerm: config.build.forcerm,
            buildargs: config.build.args.clone(),
            labels: config.build.labels.clone(),
            // Note: bollard 0.16 does not expose a `target` field on BuildImageOptions
            ..Default::default()
        };

        let mut stream = docker.build_image(options, None, Some(tar_data.into()));
        while let Some(result) = stream.next().await {
            let info =
                result.map_err(|e| ModuleError::ExecutionFailed(format!("Build failed: {}", e)))?;
            // Log build output if needed
            if let Some(error) = info.error {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Build error: {}",
                    error
                )));
            }
        }

        Ok(())
    }

    /// Remove image
    async fn remove_image(docker: &Docker, name: &str, tag: &str, force: bool) -> ModuleResult<()> {
        let reference = format!("{}:{}", name, tag);
        let options = RemoveImageOptions {
            force,
            noprune: false,
        };

        docker
            .remove_image(&reference, Some(options), None)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to remove image: {}", e)))?;

        Ok(())
    }

    /// Tag image
    async fn tag_image(docker: &Docker, source: &str, repo: &str, tag: &str) -> ModuleResult<()> {
        let options = TagImageOptions { repo, tag };

        docker
            .tag_image(source, Some(options))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to tag image: {}", e)))
    }

    /// Push image to registry
    async fn push_image(docker: &Docker, name: &str, tag: &str) -> ModuleResult<()> {
        let options = bollard::image::PushImageOptions { tag };

        let mut stream = docker.push_image(name, Some(options), None);
        while let Some(result) = stream.next().await {
            let info =
                result.map_err(|e| ModuleError::ExecutionFailed(format!("Push failed: {}", e)))?;
            if let Some(error) = info.error {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Push error: {}",
                    error
                )));
            }
        }

        Ok(())
    }

    /// Get image info
    async fn get_image_info(
        docker: &Docker,
        name: &str,
        tag: &str,
    ) -> ModuleResult<serde_json::Value> {
        let reference = format!("{}:{}", name, tag);
        match docker.inspect_image(&reference).await {
            Ok(info) => Ok(serde_json::json!({
                "id": info.id,
                "created": info.created,
                "size": info.size,
                "virtual_size": info.virtual_size,
                "architecture": info.architecture,
                "os": info.os,
                "tags": info.repo_tags,
            })),
            Err(_) => Ok(serde_json::json!({
                "exists": false,
            })),
        }
    }

    /// Execute the module asynchronously
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let config = ImageConfig::from_params(params)?;
        let docker = Self::connect_docker().await?;

        let exists = Self::image_exists(&docker, &config.name, &config.tag).await?;
        let mut changed = false;
        let mut messages = Vec::new();

        match config.state {
            ImageState::Absent => {
                if exists {
                    if context.check_mode {
                        messages.push(format!("Would remove image '{}'", config.full_reference()));
                        changed = true;
                    } else {
                        Self::remove_image(&docker, &config.name, &config.tag, config.force)
                            .await?;
                        messages.push(format!("Removed image '{}'", config.full_reference()));
                        changed = true;
                    }
                } else {
                    messages.push(format!(
                        "Image '{}' does not exist",
                        config.full_reference()
                    ));
                }
            }

            ImageState::Present => {
                match config.source {
                    ImageSource::Pull => {
                        if !exists {
                            if context.check_mode {
                                messages.push(format!(
                                    "Would pull image '{}'",
                                    config.full_reference()
                                ));
                                changed = true;
                            } else {
                                Self::pull_image(&docker, &config.name, &config.tag).await?;
                                messages
                                    .push(format!("Pulled image '{}'", config.full_reference()));
                                changed = true;
                            }
                        } else {
                            messages.push(format!(
                                "Image '{}' already exists",
                                config.full_reference()
                            ));
                        }
                    }
                    ImageSource::Build => {
                        // For build, we always rebuild if source is 'build'
                        if context.check_mode {
                            messages
                                .push(format!("Would build image '{}'", config.full_reference()));
                            changed = true;
                        } else {
                            Self::build_image(&docker, &config).await?;
                            messages.push(format!("Built image '{}'", config.full_reference()));
                            changed = true;
                        }
                    }
                    ImageSource::Load => {
                        if !exists {
                            let archive_path = config.archive_path.as_ref().ok_or_else(|| {
                                ModuleError::MissingParameter(
                                    "archive_path is required for source=load".to_string(),
                                )
                            })?;
                            if context.check_mode {
                                messages.push(format!("Would load image from '{}'", archive_path));
                                changed = true;
                            } else {
                                // Load image from tar archive
                                let file = std::fs::File::open(archive_path).map_err(|e| {
                                    ModuleError::ExecutionFailed(format!(
                                        "Failed to open archive: {}",
                                        e
                                    ))
                                })?;
                                let bytes = {
                                    use std::io::Read;
                                    let mut buf = Vec::new();
                                    let mut file = file;
                                    file.read_to_end(&mut buf).map_err(|e| {
                                        ModuleError::ExecutionFailed(format!(
                                            "Failed to read archive: {}",
                                            e
                                        ))
                                    })?;
                                    bytes::Bytes::from(buf)
                                };
                                let mut stream = docker.import_image(
                                    bollard::image::ImportImageOptions { quiet: true },
                                    bytes,
                                    None,
                                );
                                while let Some(result) = stream.next().await {
                                    result.map_err(|e| {
                                        ModuleError::ExecutionFailed(format!(
                                            "Failed to load image: {}",
                                            e
                                        ))
                                    })?;
                                }
                                messages.push(format!("Loaded image from '{}'", archive_path));
                                changed = true;
                            }
                        } else {
                            messages.push(format!(
                                "Image '{}' already exists",
                                config.full_reference()
                            ));
                        }
                    }
                    ImageSource::Local => {
                        if !exists {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Image '{}' does not exist locally",
                                config.full_reference()
                            )));
                        }
                        messages.push(format!(
                            "Image '{}' exists locally",
                            config.full_reference()
                        ));
                    }
                }
            }

            ImageState::Build => {
                if context.check_mode {
                    messages.push(format!("Would build image '{}'", config.full_reference()));
                    changed = true;
                } else {
                    Self::build_image(&docker, &config).await?;
                    messages.push(format!("Built image '{}'", config.full_reference()));
                    changed = true;
                }
            }
        }

        // Handle push if requested
        if config.push && config.state != ImageState::Absent {
            if context.check_mode {
                messages.push(format!("Would push image '{}'", config.full_reference()));
                changed = true;
            } else {
                Self::push_image(&docker, &config.name, &config.tag).await?;
                messages.push(format!("Pushed image '{}'", config.full_reference()));
                changed = true;
            }
        }

        // Get image info for output
        let image_info = Self::get_image_info(&docker, &config.name, &config.tag).await?;

        let msg = if messages.is_empty() {
            format!("Image '{}' is in desired state", config.full_reference())
        } else {
            messages.join(". ")
        };

        if changed {
            Ok(ModuleOutput::changed(msg).with_data("image", image_info))
        } else {
            Ok(ModuleOutput::ok(msg).with_data("image", image_info))
        }
    }
}

#[cfg(not(feature = "docker"))]
impl DockerImageModule {
    fn run_cmd(cmd: &str, context: &ModuleContext) -> ModuleResult<(bool, String, String)> {
        if let Some(conn) = context.connection.as_ref() {
            let rt = tokio::runtime::Handle::try_current()
                .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".into()))?;
            let result = tokio::task::block_in_place(|| rt.block_on(conn.execute(cmd, None)))
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to execute command: {}", e))
                })?;
            Ok((result.success, result.stdout, result.stderr))
        } else {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .output()
                .map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to run command: {}", e))
                })?;
            Ok((
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).to_string(),
                String::from_utf8_lossy(&output.stderr).to_string(),
            ))
        }
    }

    fn execute_cli(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        use crate::utils::shell_escape;

        let config = ImageConfig::from_params(params)?;
        let full_ref = config.full_reference();
        let escaped_ref = shell_escape(&full_ref);

        // Check if image exists
        let check_cmd = format!("docker image inspect {} 2>/dev/null", escaped_ref);
        let (exists, _, _) = Self::run_cmd(&check_cmd, context)?;

        let mut changed = false;
        let mut messages = Vec::new();

        match config.state {
            ImageState::Absent => {
                if exists {
                    if context.check_mode {
                        messages.push(format!("Would remove image '{}'", full_ref));
                        changed = true;
                    } else {
                        let mut rm_cmd = format!("docker rmi {}", escaped_ref);
                        if config.force {
                            rm_cmd = format!("docker rmi --force {}", escaped_ref);
                        }
                        let (ok, _, stderr) = Self::run_cmd(&rm_cmd, context)?;
                        if !ok {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to remove image '{}': {}",
                                full_ref,
                                stderr.trim()
                            )));
                        }
                        messages.push(format!("Removed image '{}'", full_ref));
                        changed = true;
                    }
                } else {
                    messages.push(format!("Image '{}' does not exist", full_ref));
                }
            }

            ImageState::Present => match config.source {
                ImageSource::Pull => {
                    if !exists {
                        if context.check_mode {
                            messages.push(format!("Would pull image '{}'", full_ref));
                            changed = true;
                        } else {
                            let pull_cmd = format!("docker pull {}", escaped_ref);
                            let (ok, _, stderr) = Self::run_cmd(&pull_cmd, context)?;
                            if !ok {
                                return Err(ModuleError::ExecutionFailed(format!(
                                    "Failed to pull image '{}': {}",
                                    full_ref,
                                    stderr.trim()
                                )));
                            }
                            messages.push(format!("Pulled image '{}'", full_ref));
                            changed = true;
                        }
                    } else {
                        messages.push(format!("Image '{}' already exists", full_ref));
                    }
                }
                ImageSource::Build => {
                    if context.check_mode {
                        messages.push(format!("Would build image '{}'", full_ref));
                        changed = true;
                    } else {
                        let build_path = config.build.path.as_ref().ok_or_else(|| {
                            ModuleError::MissingParameter(
                                "build.path is required for building images".to_string(),
                            )
                        })?;
                        let mut build_cmd = format!(
                            "docker build -t {} {}",
                            escaped_ref,
                            shell_escape(build_path)
                        );
                        if config.build.dockerfile != "Dockerfile" {
                            build_cmd.push_str(&format!(
                                " --file {}",
                                shell_escape(&config.build.dockerfile)
                            ));
                        }
                        if config.build.nocache {
                            build_cmd.push_str(" --no-cache");
                        }
                        for (k, v) in &config.build.args {
                            build_cmd.push_str(&format!(
                                " --build-arg {}={}",
                                shell_escape(k),
                                shell_escape(v)
                            ));
                        }
                        let (ok, _, stderr) = Self::run_cmd(&build_cmd, context)?;
                        if !ok {
                            return Err(ModuleError::ExecutionFailed(format!(
                                "Failed to build image '{}': {}",
                                full_ref,
                                stderr.trim()
                            )));
                        }
                        messages.push(format!("Built image '{}'", full_ref));
                        changed = true;
                    }
                }
                ImageSource::Load => {
                    if !exists {
                        let archive_path = config.archive_path.as_ref().ok_or_else(|| {
                            ModuleError::MissingParameter(
                                "archive_path is required for source=load".to_string(),
                            )
                        })?;
                        if context.check_mode {
                            messages.push(format!("Would load image from '{}'", archive_path));
                            changed = true;
                        } else {
                            let load_cmd = format!("docker load -i {}", shell_escape(archive_path));
                            let (ok, _, stderr) = Self::run_cmd(&load_cmd, context)?;
                            if !ok {
                                return Err(ModuleError::ExecutionFailed(format!(
                                    "Failed to load image from '{}': {}",
                                    archive_path,
                                    stderr.trim()
                                )));
                            }
                            messages.push(format!("Loaded image from '{}'", archive_path));
                            changed = true;
                        }
                    } else {
                        messages.push(format!("Image '{}' already exists", full_ref));
                    }
                }
                ImageSource::Local => {
                    if !exists {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Image '{}' does not exist locally",
                            full_ref
                        )));
                    }
                    messages.push(format!("Image '{}' exists locally", full_ref));
                }
            },

            ImageState::Build => {
                if context.check_mode {
                    messages.push(format!("Would build image '{}'", full_ref));
                    changed = true;
                } else {
                    let build_path = config.build.path.as_ref().ok_or_else(|| {
                        ModuleError::MissingParameter(
                            "build.path is required for building images".to_string(),
                        )
                    })?;
                    let mut build_cmd = format!(
                        "docker build -t {} {}",
                        escaped_ref,
                        shell_escape(build_path)
                    );
                    if config.build.dockerfile != "Dockerfile" {
                        build_cmd.push_str(&format!(
                            " --file {}",
                            shell_escape(&config.build.dockerfile)
                        ));
                    }
                    if config.build.nocache {
                        build_cmd.push_str(" --no-cache");
                    }
                    for (k, v) in &config.build.args {
                        build_cmd.push_str(&format!(
                            " --build-arg {}={}",
                            shell_escape(k),
                            shell_escape(v)
                        ));
                    }
                    let (ok, _, stderr) = Self::run_cmd(&build_cmd, context)?;
                    if !ok {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to build image '{}': {}",
                            full_ref,
                            stderr.trim()
                        )));
                    }
                    messages.push(format!("Built image '{}'", full_ref));
                    changed = true;
                }
            }
        }

        // Handle push if requested
        if config.push && config.state != ImageState::Absent {
            if context.check_mode {
                messages.push(format!("Would push image '{}'", full_ref));
                changed = true;
            } else {
                // Tag for repository if specified
                if let Some(ref repo) = config.repository {
                    let tag_target = format!("{}:{}", repo, config.tag);
                    let tag_cmd =
                        format!("docker tag {} {}", escaped_ref, shell_escape(&tag_target));
                    let (ok, _, stderr) = Self::run_cmd(&tag_cmd, context)?;
                    if !ok {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to tag image: {}",
                            stderr.trim()
                        )));
                    }
                    let push_cmd = format!("docker push {}", shell_escape(&tag_target));
                    let (ok, _, stderr) = Self::run_cmd(&push_cmd, context)?;
                    if !ok {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to push image '{}': {}",
                            tag_target,
                            stderr.trim()
                        )));
                    }
                    messages.push(format!("Pushed image '{}'", tag_target));
                } else {
                    let push_cmd = format!("docker push {}", escaped_ref);
                    let (ok, _, stderr) = Self::run_cmd(&push_cmd, context)?;
                    if !ok {
                        return Err(ModuleError::ExecutionFailed(format!(
                            "Failed to push image '{}': {}",
                            full_ref,
                            stderr.trim()
                        )));
                    }
                    messages.push(format!("Pushed image '{}'", full_ref));
                }
                changed = true;
            }
        }

        // Get image info for output
        let image_info = if !context.check_mode {
            let info_cmd = format!(
                "docker image inspect --format '{{{{json .}}}}' {}",
                escaped_ref
            );
            if let Ok((true, stdout, _)) = Self::run_cmd(&info_cmd, context) {
                serde_json::from_str(stdout.trim())
                    .unwrap_or_else(|_| serde_json::json!({ "name": full_ref }))
            } else {
                serde_json::json!({ "name": full_ref, "exists": false })
            }
        } else {
            serde_json::json!({ "name": full_ref })
        };

        let msg = if messages.is_empty() {
            format!("Image '{}' is in desired state", full_ref)
        } else {
            messages.join(". ")
        };

        if changed {
            Ok(ModuleOutput::changed(msg).with_data("image", image_info))
        } else {
            Ok(ModuleOutput::ok(msg).with_data("image", image_info))
        }
    }
}

impl Module for DockerImageModule {
    fn name(&self) -> &'static str {
        "docker_image"
    }

    fn description(&self) -> &'static str {
        "Manage Docker images"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Image operations can be rate-limited by registry
        ParallelizationHint::RateLimited {
            requests_per_second: 5,
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["name"]
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        #[cfg(feature = "docker")]
        {
            let rt = tokio::runtime::Handle::try_current().map_err(|_| {
                ModuleError::ExecutionFailed("No tokio runtime available".to_string())
            })?;

            let params = params.clone();
            let context = context.clone();
            std::thread::scope(|s| {
                s.spawn(|| rt.block_on(self.execute_async(&params, &context)))
                    .join()
                    .map_err(|_| ModuleError::ExecutionFailed("Thread panicked".into()))?
            })
        }

        #[cfg(not(feature = "docker"))]
        {
            self.execute_cli(params, context)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_state_from_str() {
        assert_eq!(
            ImageState::from_str("present").unwrap(),
            ImageState::Present
        );
        assert_eq!(ImageState::from_str("absent").unwrap(), ImageState::Absent);
        assert_eq!(ImageState::from_str("build").unwrap(), ImageState::Build);
        assert!(ImageState::from_str("invalid").is_err());
    }

    #[test]
    fn test_image_source_from_str() {
        assert_eq!(ImageSource::from_str("pull").unwrap(), ImageSource::Pull);
        assert_eq!(ImageSource::from_str("build").unwrap(), ImageSource::Build);
        assert_eq!(ImageSource::from_str("load").unwrap(), ImageSource::Load);
        assert_eq!(ImageSource::from_str("local").unwrap(), ImageSource::Local);
        assert!(ImageSource::from_str("invalid").is_err());
    }

    #[test]
    fn test_module_metadata() {
        let module = DockerImageModule;
        assert_eq!(module.name(), "docker_image");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_image_config_full_reference() {
        let config = ImageConfig {
            name: "nginx".to_string(),
            tag: "latest".to_string(),
            state: ImageState::Present,
            source: ImageSource::Pull,
            build: BuildConfig::default(),
            push: false,
            force: false,
            archive_path: None,
            repository: None,
        };
        assert_eq!(config.full_reference(), "nginx:latest");
    }
}
