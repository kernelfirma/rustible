//! OS image pipeline module
//!
//! Manages the lifecycle of OS images for bare-metal provisioning:
//! build, version, promote, and rollback. Images progress through
//! status stages: Building -> Ready -> Active -> Deprecated.
//!
//! # Parameters
//!
//! - `action` (required): "build", "promote", "rollback", "list", "status"
//! - `name` (required): Image name
//! - `version` (optional): Image version (for promote/rollback)
//! - `build_script` (optional): Path to image build script (for build)
//! - `image_dir` (optional): Directory for image storage (default: "/var/lib/rustible/images")

use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Handle;

use serde::{Deserialize, Serialize};

use crate::connection::{Connection, ExecuteOptions};
use crate::modules::{
    Module, ModuleContext, ModuleError, ModuleOutput, ModuleParams, ModuleResult, ParamExt,
    ParallelizationHint,
};

fn get_exec_options(context: &ModuleContext) -> ExecuteOptions {
    let mut options = ExecuteOptions::new();
    if context.r#become {
        options = options.with_escalation(context.become_user.clone());
        if let Some(ref method) = context.become_method {
            options.escalate_method = Some(method.clone());
        }
        if let Some(ref password) = context.become_password {
            options.escalate_password = Some(password.clone());
        }
    }
    options
}

fn run_cmd(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<(bool, String, String)> {
    let options = get_exec_options(context);
    let result = Handle::current()
        .block_on(async { connection.execute(cmd, Some(options)).await })
        .map_err(|e| ModuleError::ExecutionFailed(format!("Connection error: {}", e)))?;
    Ok((result.success, result.stdout, result.stderr))
}

fn run_cmd_ok(
    connection: &Arc<dyn Connection + Send + Sync>,
    cmd: &str,
    context: &ModuleContext,
) -> ModuleResult<String> {
    let (success, stdout, stderr) = run_cmd(connection, cmd, context)?;
    if !success {
        return Err(ModuleError::ExecutionFailed(format!(
            "Command failed: {}",
            stderr.trim()
        )));
    }
    Ok(stdout)
}

/// Status of an OS image in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageStatus {
    /// Image is currently being built.
    Building,
    /// Image build completed, ready for testing.
    Ready,
    /// Image is the currently active/deployed image.
    Active,
    /// Image has been superseded and is no longer deployed.
    Deprecated,
}

impl ImageStatus {
    /// Parse a string into an ImageStatus.
    pub fn from_str(s: &str) -> Option<ImageStatus> {
        match s.to_lowercase().as_str() {
            "building" => Some(ImageStatus::Building),
            "ready" => Some(ImageStatus::Ready),
            "active" => Some(ImageStatus::Active),
            "deprecated" => Some(ImageStatus::Deprecated),
            _ => None,
        }
    }

    /// Check if a transition from self to target is valid.
    pub fn can_transition_to(&self, target: ImageStatus) -> bool {
        matches!(
            (self, target),
            (ImageStatus::Building, ImageStatus::Ready)
                | (ImageStatus::Ready, ImageStatus::Active)
                | (ImageStatus::Active, ImageStatus::Deprecated)
                | (ImageStatus::Ready, ImageStatus::Deprecated)
                // Rollback: deprecated can become active again
                | (ImageStatus::Deprecated, ImageStatus::Active)
        )
    }
}

/// Represents a versioned OS image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsImage {
    pub name: String,
    pub version: String,
    pub build_id: String,
    pub status: ImageStatus,
    pub created_at: String,
}

impl OsImage {
    /// Create a new OS image record.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        build_id: impl Into<String>,
        status: ImageStatus,
        created_at: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            build_id: build_id.into(),
            status,
            created_at: created_at.into(),
        }
    }

    /// Serialize the image metadata to JSON for storage.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserialize image metadata from JSON.
    pub fn from_json_str(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Default directory for image metadata storage.
const DEFAULT_IMAGE_DIR: &str = "/var/lib/rustible/images";

pub struct ImagePipelineModule;

impl Module for ImagePipelineModule {
    fn name(&self) -> &'static str {
        "hpc_image_pipeline"
    }

    fn description(&self) -> &'static str {
        "OS image lifecycle pipeline: build, version, promote, and rollback"
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        ParallelizationHint::HostExclusive
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let connection = context
            .connection
            .as_ref()
            .ok_or_else(|| ModuleError::ExecutionFailed("No connection available".to_string()))?;

        let action = params.get_string_required("action")?;
        let image_dir = params
            .get_string("image_dir")?
            .unwrap_or_else(|| DEFAULT_IMAGE_DIR.to_string());

        match action.as_str() {
            "build" => self.action_build(connection, params, context, &image_dir),
            "promote" => self.action_promote(connection, params, context, &image_dir),
            "rollback" => self.action_rollback(connection, params, context, &image_dir),
            "list" => self.action_list(connection, params, context, &image_dir),
            "status" => self.action_status(connection, params, context, &image_dir),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid action '{}'. Must be 'build', 'promote', 'rollback', 'list', or 'status'",
                action
            ))),
        }
    }

    fn required_params(&self) -> &[&'static str] {
        &["action", "name"]
    }

    fn optional_params(&self) -> HashMap<&'static str, serde_json::Value> {
        let mut m = HashMap::new();
        m.insert("version", serde_json::json!(null));
        m.insert("build_script", serde_json::json!(null));
        m.insert("image_dir", serde_json::json!(DEFAULT_IMAGE_DIR));
        m
    }
}

impl ImagePipelineModule {
    /// Generate a build ID from the current timestamp.
    fn generate_build_id(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let output = run_cmd_ok(connection, "date -u +%Y%m%d%H%M%S", context)?;
        Ok(output.trim().to_string())
    }

    /// Get the current UTC timestamp as ISO 8601 string.
    fn get_timestamp(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        let output = run_cmd_ok(connection, "date -u +%Y-%m-%dT%H:%M:%SZ", context)?;
        Ok(output.trim().to_string())
    }

    /// Read an image metadata file if it exists.
    fn read_image(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        path: &str,
    ) -> ModuleResult<Option<OsImage>> {
        let (ok, content, _) = run_cmd(
            connection,
            &format!("cat '{}' 2>/dev/null", path),
            context,
        )?;
        if !ok || content.trim().is_empty() {
            return Ok(None);
        }
        let image = OsImage::from_json_str(content.trim()).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to parse image metadata: {}", e))
        })?;
        Ok(Some(image))
    }

    /// Write image metadata to a file.
    fn write_image(
        connection: &Arc<dyn Connection + Send + Sync>,
        context: &ModuleContext,
        path: &str,
        image: &OsImage,
    ) -> ModuleResult<()> {
        let json = image.to_json_string().map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to serialize image: {}", e))
        })?;
        run_cmd_ok(
            connection,
            &format!(
                "printf '%s\\n' '{}' > '{}'",
                json.replace('\'', "'\\''"),
                path,
            ),
            context,
        )?;
        Ok(())
    }

    fn action_build(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
        image_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("name")?;
        let build_script = params.get_string("build_script")?;

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would build image '{}'",
                name
            ))
            .with_data("name", serde_json::json!(name)));
        }

        let build_id = Self::generate_build_id(connection, context)?;
        let timestamp = Self::get_timestamp(connection, context)?;
        let version = format!("{}-{}", name, build_id);

        // Ensure image directory exists
        let version_dir = format!("{}/{}", image_dir, name);
        run_cmd_ok(
            connection,
            &format!("mkdir -p '{}'", version_dir),
            context,
        )?;

        // Create initial image record as Building
        let mut image = OsImage::new(
            name.clone(),
            version.clone(),
            build_id.clone(),
            ImageStatus::Building,
            timestamp,
        );

        let metadata_path = format!("{}/{}.json", version_dir, build_id);
        Self::write_image(connection, context, &metadata_path, &image)?;

        // Run build script if provided
        if let Some(ref script) = build_script {
            let (ok, stdout, stderr) = run_cmd(connection, script, context)?;
            if !ok {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Build script failed for image '{}': {}",
                    name,
                    stderr.trim()
                )));
            }
            // Log build output
            let log_path = format!("{}/{}.log", version_dir, build_id);
            let _ = run_cmd(
                connection,
                &format!(
                    "printf '%s\\n' '{}' > '{}'",
                    stdout.replace('\'', "'\\''"),
                    log_path
                ),
                context,
            );
        }

        // Transition to Ready
        image.status = ImageStatus::Ready;
        Self::write_image(connection, context, &metadata_path, &image)?;

        Ok(ModuleOutput::changed(format!(
            "Built image '{}' version '{}' (status: ready)",
            name, version
        ))
        .with_data("image", serde_json::json!(image))
        .with_data("path", serde_json::json!(metadata_path)))
    }

    fn action_promote(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
        image_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("name")?;
        let version = params.get_string_required("version")?;

        // Extract build_id from version (format: name-buildid)
        let build_id = version
            .strip_prefix(&format!("{}-", name))
            .unwrap_or(&version);

        let version_dir = format!("{}/{}", image_dir, name);
        let metadata_path = format!("{}/{}.json", version_dir, build_id);

        let image = Self::read_image(connection, context, &metadata_path)?.ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "Image '{}' version '{}' not found",
                name, version
            ))
        })?;

        // Validate transition
        if !image.status.can_transition_to(ImageStatus::Active) {
            return Err(ModuleError::ExecutionFailed(format!(
                "Cannot promote image from {:?} to Active. Image must be in Ready state.",
                image.status
            )));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would promote image '{}' version '{}' to active",
                name, version
            ))
            .with_data("image", serde_json::json!(image)));
        }

        // Deprecate any currently active image
        let (ok, listing, _) = run_cmd(
            connection,
            &format!(
                "ls -1 '{}'/*.json 2>/dev/null",
                version_dir
            ),
            context,
        )?;

        if ok {
            for file in listing.lines() {
                let file = file.trim();
                if file.is_empty() || file == metadata_path {
                    continue;
                }
                if let Ok(Some(mut other)) = Self::read_image(connection, context, file) {
                    if other.status == ImageStatus::Active {
                        other.status = ImageStatus::Deprecated;
                        let _ = Self::write_image(connection, context, file, &other);
                    }
                }
            }
        }

        // Promote this image
        let mut promoted = image;
        promoted.status = ImageStatus::Active;
        Self::write_image(connection, context, &metadata_path, &promoted)?;

        // Update the "active" symlink
        let active_link = format!("{}/active.json", version_dir);
        let _ = run_cmd(
            connection,
            &format!("ln -sf '{}.json' '{}'", build_id, active_link),
            context,
        );

        Ok(ModuleOutput::changed(format!(
            "Promoted image '{}' version '{}' to active",
            name, version
        ))
        .with_data("image", serde_json::json!(promoted))
        .with_data("path", serde_json::json!(metadata_path)))
    }

    fn action_rollback(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
        image_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("name")?;
        let version = params.get_string_required("version")?;

        let build_id = version
            .strip_prefix(&format!("{}-", name))
            .unwrap_or(&version);

        let version_dir = format!("{}/{}", image_dir, name);
        let metadata_path = format!("{}/{}.json", version_dir, build_id);

        let image = Self::read_image(connection, context, &metadata_path)?.ok_or_else(|| {
            ModuleError::ExecutionFailed(format!(
                "Image '{}' version '{}' not found for rollback",
                name, version
            ))
        })?;

        if image.status == ImageStatus::Active {
            return Ok(ModuleOutput::ok(format!(
                "Image '{}' version '{}' is already active",
                name, version
            ))
            .with_data("image", serde_json::json!(image)));
        }

        // Only deprecated images can be rolled back to active
        if !image.status.can_transition_to(ImageStatus::Active) {
            return Err(ModuleError::ExecutionFailed(format!(
                "Cannot rollback image from {:?} to Active. Image must be Deprecated.",
                image.status
            )));
        }

        if context.check_mode {
            return Ok(ModuleOutput::changed(format!(
                "Would rollback to image '{}' version '{}'",
                name, version
            ))
            .with_data("image", serde_json::json!(image)));
        }

        // Deprecate current active image
        let (ok, listing, _) = run_cmd(
            connection,
            &format!("ls -1 '{}'/*.json 2>/dev/null", version_dir),
            context,
        )?;

        if ok {
            for file in listing.lines() {
                let file = file.trim();
                if file.is_empty() || file == metadata_path {
                    continue;
                }
                if let Ok(Some(mut other)) = Self::read_image(connection, context, file) {
                    if other.status == ImageStatus::Active {
                        other.status = ImageStatus::Deprecated;
                        let _ = Self::write_image(connection, context, file, &other);
                    }
                }
            }
        }

        // Activate the rollback target
        let mut rolled_back = image;
        rolled_back.status = ImageStatus::Active;
        Self::write_image(connection, context, &metadata_path, &rolled_back)?;

        // Update active symlink
        let active_link = format!("{}/active.json", version_dir);
        let _ = run_cmd(
            connection,
            &format!("ln -sf '{}.json' '{}'", build_id, active_link),
            context,
        );

        Ok(ModuleOutput::changed(format!(
            "Rolled back to image '{}' version '{}'",
            name, version
        ))
        .with_data("image", serde_json::json!(rolled_back))
        .with_data("path", serde_json::json!(metadata_path)))
    }

    fn action_list(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
        image_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("name")?;
        let version_dir = format!("{}/{}", image_dir, name);

        if context.check_mode {
            return Ok(ModuleOutput::ok(format!(
                "Would list versions of image '{}'",
                name
            )));
        }

        let (ok, listing, _) = run_cmd(
            connection,
            &format!(
                "ls -1 '{}'/*.json 2>/dev/null | grep -v active.json || true",
                version_dir
            ),
            context,
        )?;

        let mut images: Vec<serde_json::Value> = Vec::new();
        if ok {
            for file in listing.lines() {
                let file = file.trim();
                if file.is_empty() {
                    continue;
                }
                if let Ok(Some(img)) = Self::read_image(connection, context, file) {
                    images.push(serde_json::json!(img));
                }
            }
        }

        Ok(ModuleOutput::ok(format!(
            "Found {} versions of image '{}'",
            images.len(),
            name
        ))
        .with_data("images", serde_json::json!(images))
        .with_data("name", serde_json::json!(name)))
    }

    fn action_status(
        &self,
        connection: &Arc<dyn Connection + Send + Sync>,
        params: &ModuleParams,
        context: &ModuleContext,
        image_dir: &str,
    ) -> ModuleResult<ModuleOutput> {
        let name = params.get_string_required("name")?;
        let version = params.get_string("version")?;

        if context.check_mode {
            return Ok(ModuleOutput::ok(format!(
                "Would check status of image '{}'",
                name
            )));
        }

        let version_dir = format!("{}/{}", image_dir, name);

        // If version specified, get that specific image
        if let Some(ver) = version {
            let build_id = ver
                .strip_prefix(&format!("{}-", name))
                .unwrap_or(&ver);
            let metadata_path = format!("{}/{}.json", version_dir, build_id);

            let image =
                Self::read_image(connection, context, &metadata_path)?.ok_or_else(|| {
                    ModuleError::ExecutionFailed(format!(
                        "Image '{}' version '{}' not found",
                        name, ver
                    ))
                })?;

            return Ok(ModuleOutput::ok(format!(
                "Image '{}' version '{}' status: {:?}",
                name, ver, image.status
            ))
            .with_data("image", serde_json::json!(image)));
        }

        // Otherwise find the active image
        let active_path = format!("{}/active.json", version_dir);
        let image = Self::read_image(connection, context, &active_path)?;

        match image {
            Some(img) => Ok(ModuleOutput::ok(format!(
                "Active image '{}': version '{}' (build {})",
                name, img.version, img.build_id
            ))
            .with_data("image", serde_json::json!(img))),
            None => Ok(ModuleOutput::ok(format!(
                "No active image found for '{}'",
                name
            ))
            .with_data("name", serde_json::json!(name))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_status_from_str() {
        assert_eq!(
            ImageStatus::from_str("building"),
            Some(ImageStatus::Building)
        );
        assert_eq!(ImageStatus::from_str("READY"), Some(ImageStatus::Ready));
        assert_eq!(ImageStatus::from_str("Active"), Some(ImageStatus::Active));
        assert_eq!(
            ImageStatus::from_str("deprecated"),
            Some(ImageStatus::Deprecated)
        );
        assert_eq!(ImageStatus::from_str("invalid"), None);
        assert_eq!(ImageStatus::from_str(""), None);
    }

    #[test]
    fn test_image_status_transitions() {
        // Valid transitions: Building -> Ready
        assert!(ImageStatus::Building.can_transition_to(ImageStatus::Ready));
        // Valid transitions: Ready -> Active
        assert!(ImageStatus::Ready.can_transition_to(ImageStatus::Active));
        // Valid transitions: Active -> Deprecated
        assert!(ImageStatus::Active.can_transition_to(ImageStatus::Deprecated));
        // Valid transitions: Ready -> Deprecated (skip active)
        assert!(ImageStatus::Ready.can_transition_to(ImageStatus::Deprecated));
        // Valid transitions: Deprecated -> Active (rollback)
        assert!(ImageStatus::Deprecated.can_transition_to(ImageStatus::Active));

        // Invalid transitions
        assert!(!ImageStatus::Building.can_transition_to(ImageStatus::Active));
        assert!(!ImageStatus::Building.can_transition_to(ImageStatus::Deprecated));
        assert!(!ImageStatus::Active.can_transition_to(ImageStatus::Ready));
        assert!(!ImageStatus::Deprecated.can_transition_to(ImageStatus::Ready));
        assert!(!ImageStatus::Active.can_transition_to(ImageStatus::Building));
    }

    #[test]
    fn test_os_image_new() {
        let image = OsImage::new(
            "rocky9-hpc",
            "rocky9-hpc-20250101120000",
            "20250101120000",
            ImageStatus::Building,
            "2025-01-01T12:00:00Z",
        );
        assert_eq!(image.name, "rocky9-hpc");
        assert_eq!(image.version, "rocky9-hpc-20250101120000");
        assert_eq!(image.build_id, "20250101120000");
        assert_eq!(image.status, ImageStatus::Building);
        assert_eq!(image.created_at, "2025-01-01T12:00:00Z");
    }

    #[test]
    fn test_os_image_json_roundtrip() {
        let image = OsImage::new(
            "ubuntu2204-compute",
            "ubuntu2204-compute-20250615",
            "20250615143000",
            ImageStatus::Ready,
            "2025-06-15T14:30:00Z",
        );

        let json = image.to_json_string().unwrap();
        let parsed = OsImage::from_json_str(&json).unwrap();

        assert_eq!(parsed.name, "ubuntu2204-compute");
        assert_eq!(parsed.version, image.version);
        assert_eq!(parsed.build_id, image.build_id);
        assert_eq!(parsed.status, ImageStatus::Ready);
        assert_eq!(parsed.created_at, image.created_at);
    }

    #[test]
    fn test_os_image_from_invalid_json() {
        let result = OsImage::from_json_str("not valid json");
        assert!(result.is_err());
    }

    #[test]
    fn test_os_image_serde_status_values() {
        // Verify all status values serialize correctly
        for (status, expected) in [
            (ImageStatus::Building, "\"building\""),
            (ImageStatus::Ready, "\"ready\""),
            (ImageStatus::Active, "\"active\""),
            (ImageStatus::Deprecated, "\"deprecated\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn test_os_image_lifecycle() {
        // Simulate a complete lifecycle: build -> promote -> deprecate
        let mut image = OsImage::new(
            "rocky9",
            "rocky9-001",
            "001",
            ImageStatus::Building,
            "2025-01-01T00:00:00Z",
        );
        assert_eq!(image.status, ImageStatus::Building);

        // Building -> Ready
        assert!(image.status.can_transition_to(ImageStatus::Ready));
        image.status = ImageStatus::Ready;
        assert_eq!(image.status, ImageStatus::Ready);

        // Ready -> Active (promote)
        assert!(image.status.can_transition_to(ImageStatus::Active));
        image.status = ImageStatus::Active;
        assert_eq!(image.status, ImageStatus::Active);

        // Active -> Deprecated (new image promoted)
        assert!(image.status.can_transition_to(ImageStatus::Deprecated));
        image.status = ImageStatus::Deprecated;
        assert_eq!(image.status, ImageStatus::Deprecated);

        // Deprecated -> Active (rollback)
        assert!(image.status.can_transition_to(ImageStatus::Active));
        image.status = ImageStatus::Active;
        assert_eq!(image.status, ImageStatus::Active);
    }

    #[test]
    fn test_module_name_and_description() {
        let module = ImagePipelineModule;
        assert_eq!(module.name(), "hpc_image_pipeline");
        assert!(!module.description().is_empty());
    }

    #[test]
    fn test_module_required_params() {
        let module = ImagePipelineModule;
        let required = module.required_params();
        assert!(required.contains(&"action"));
        assert!(required.contains(&"name"));
    }

    #[test]
    fn test_module_optional_params() {
        let module = ImagePipelineModule;
        let optional = module.optional_params();
        assert!(optional.contains_key("version"));
        assert!(optional.contains_key("build_script"));
        assert!(optional.contains_key("image_dir"));
    }

    #[test]
    fn test_os_image_full_serde() {
        let image = OsImage::new(
            "centos-stream9",
            "centos-stream9-20250701",
            "20250701090000",
            ImageStatus::Active,
            "2025-07-01T09:00:00Z",
        );
        let value = serde_json::to_value(&image).unwrap();
        assert_eq!(value["name"], "centos-stream9");
        assert_eq!(value["status"], "active");
        assert_eq!(value["build_id"], "20250701090000");
        assert_eq!(value["created_at"], "2025-07-01T09:00:00Z");
    }

    #[test]
    fn test_image_status_equality() {
        assert_eq!(ImageStatus::Building, ImageStatus::Building);
        assert_ne!(ImageStatus::Building, ImageStatus::Ready);
        assert_ne!(ImageStatus::Active, ImageStatus::Deprecated);
    }

    #[test]
    fn test_image_status_self_transition_invalid() {
        // No self-transitions should be valid
        assert!(!ImageStatus::Building.can_transition_to(ImageStatus::Building));
        assert!(!ImageStatus::Ready.can_transition_to(ImageStatus::Ready));
        assert!(!ImageStatus::Active.can_transition_to(ImageStatus::Active));
        assert!(!ImageStatus::Deprecated.can_transition_to(ImageStatus::Deprecated));
    }
}
