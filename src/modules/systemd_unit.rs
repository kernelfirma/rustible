//! Systemd Unit Module - Manage systemd unit files
//!
//! This module provides comprehensive management of systemd unit files including:
//! - Creating/updating/removing unit files (service, socket, timer, path, mount, etc.)
//! - Unit file templating with variable substitution
//! - Automatic daemon-reload when unit files change
//! - Unit state management (enable/disable, start/stop)
//!
//! ## Supported Unit Types
//!
//! - **service**: Long-running daemons or one-shot tasks
//! - **socket**: Socket-based activation units
//! - **timer**: Timer-based activation (cron replacement)
//! - **path**: Path-based activation (file/directory monitoring)
//! - **mount**: Filesystem mount points
//! - **automount**: Automount points
//! - **swap**: Swap space configuration
//! - **slice**: Resource management groups
//! - **scope**: Externally created process groups
//! - **target**: Synchronization points
//!
//! ## Parameters
//!
//! - `name`: Unit name (required, e.g., "myapp.service")
//! - `state`: Desired state (present, absent)
//! - `content`: Unit file content (mutually exclusive with template/src)
//! - `template`: Template content for unit file (Tera/Jinja2 syntax)
//! - `src`: Path to template file on control node
//! - `unit_type`: Override unit type detection from name
//! - `enabled`: Whether unit should start on boot
//! - `running`: Desired running state (started, stopped, restarted)
//! - `daemon_reload`: Force daemon-reload even if no changes (default: auto)
//! - `daemon_reexec`: Re-execute systemd manager
//! - `force`: Overwrite existing unit files
//! - `mode`: File permissions (default: 0644)
//! - `owner`: File owner (default: root)
//! - `group`: File group (default: root)
//! - `unit_path`: Custom path for unit file (default: /etc/systemd/system)
//! - `vars`: Additional variables for template rendering
//!
//! ## Unit File Templates
//!
//! Templates support Tera/Jinja2 syntax for variable substitution:
//!
//! ```ini
//! [Unit]
//! Description={{ description }}
//! After={{ after | default(value="network.target") }}
//!
//! [Service]
//! Type={{ service_type | default(value="simple") }}
//! ExecStart={{ exec_start }}
//! User={{ user }}
//! Group={{ group }}
//! {% if environment is defined %}
//! {% for key, value in environment %}
//! Environment="{{ key }}={{ value }}"
//! {% endfor %}
//! {% endif %}
//!
//! [Install]
//! WantedBy={{ wanted_by | default(value="multi-user.target") }}
//! ```

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use crate::connection::{Connection, ExecuteOptions, TransferOptions};
use crate::template::TEMPLATE_ENGINE;
use once_cell::sync::Lazy;
use regex::Regex;
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// Regex for validating unit names
static UNIT_NAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^[a-zA-Z0-9_@.-]+\.(service|socket|timer|path|mount|automount|swap|slice|scope|target)$",
    )
    .expect("Invalid unit name regex")
});

/// Regex for validating template instance names (e.g., myapp@instance.service)
static INSTANCE_NAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^([a-zA-Z0-9_.-]+)@([a-zA-Z0-9_.-]*)\.([a-z]+)$")
        .expect("Invalid instance name regex")
});

/// Supported systemd unit types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitType {
    Service,
    Socket,
    Timer,
    Path,
    Mount,
    Automount,
    Swap,
    Slice,
    Scope,
    Target,
}

impl UnitType {
    /// Parse unit type from string
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "service" => Ok(UnitType::Service),
            "socket" => Ok(UnitType::Socket),
            "timer" => Ok(UnitType::Timer),
            "path" => Ok(UnitType::Path),
            "mount" => Ok(UnitType::Mount),
            "automount" => Ok(UnitType::Automount),
            "swap" => Ok(UnitType::Swap),
            "slice" => Ok(UnitType::Slice),
            "scope" => Ok(UnitType::Scope),
            "target" => Ok(UnitType::Target),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid unit type '{}'. Valid types: service, socket, timer, path, mount, automount, swap, slice, scope, target",
                s
            ))),
        }
    }

    /// Get the file extension for this unit type
    fn extension(&self) -> &'static str {
        match self {
            UnitType::Service => "service",
            UnitType::Socket => "socket",
            UnitType::Timer => "timer",
            UnitType::Path => "path",
            UnitType::Mount => "mount",
            UnitType::Automount => "automount",
            UnitType::Swap => "swap",
            UnitType::Slice => "slice",
            UnitType::Scope => "scope",
            UnitType::Target => "target",
        }
    }

    /// Detect unit type from filename
    fn from_filename(name: &str) -> Option<Self> {
        if let Some(ext) = name.rsplit('.').next() {
            Self::from_str(ext).ok()
        } else {
            None
        }
    }
}

impl std::str::FromStr for UnitType {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UnitType::from_str(s)
    }
}

impl std::fmt::Display for UnitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.extension())
    }
}

/// Desired state for a unit file
#[derive(Debug, Clone, PartialEq)]
pub enum UnitState {
    /// Unit file should exist
    Present,
    /// Unit file should not exist
    Absent,
}

impl UnitState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "present" => Ok(UnitState::Present),
            "absent" => Ok(UnitState::Absent),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid state '{}'. Valid states: present, absent",
                s
            ))),
        }
    }
}

impl std::str::FromStr for UnitState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UnitState::from_str(s)
    }
}

/// Desired running state for a unit
#[derive(Debug, Clone, PartialEq)]
pub enum RunningState {
    Started,
    Stopped,
    Restarted,
    Reloaded,
}

impl RunningState {
    pub fn from_str(s: &str) -> ModuleResult<Self> {
        match s.to_lowercase().as_str() {
            "started" | "running" => Ok(RunningState::Started),
            "stopped" => Ok(RunningState::Stopped),
            "restarted" => Ok(RunningState::Restarted),
            "reloaded" => Ok(RunningState::Reloaded),
            _ => Err(ModuleError::InvalidParameter(format!(
                "Invalid running state '{}'. Valid states: started, stopped, restarted, reloaded",
                s
            ))),
        }
    }
}

impl std::str::FromStr for RunningState {
    type Err = ModuleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        RunningState::from_str(s)
    }
}

/// Module configuration parsed from parameters
#[derive(Debug, Clone)]
struct SystemdUnitConfig {
    /// Unit name (e.g., "myapp.service")
    name: String,
    /// Detected or specified unit type
    unit_type: UnitType,
    /// Whether this is a template unit (e.g., myapp@.service)
    is_template: bool,
    /// Instance name if instantiated from template (e.g., "instance" in myapp@instance.service)
    instance: Option<String>,
    /// Desired state (present, absent)
    state: UnitState,
    /// Unit file content (direct or rendered)
    content: Option<String>,
    /// Template content for rendering
    template: Option<String>,
    /// Source file path for template
    src: Option<String>,
    /// Whether to enable the unit
    enabled: Option<bool>,
    /// Desired running state
    running: Option<RunningState>,
    /// Force daemon-reload
    daemon_reload: Option<bool>,
    /// Re-execute systemd manager
    daemon_reexec: bool,
    /// Force overwrite
    force: bool,
    /// File mode
    mode: u32,
    /// File owner
    owner: Option<String>,
    /// File group
    group: Option<String>,
    /// Unit file path
    unit_path: String,
    /// Extra variables for template rendering
    vars: Option<serde_json::Value>,
}

impl SystemdUnitConfig {
    fn from_params(params: &ModuleParams) -> ModuleResult<Self> {
        let name = params.get_string_required("name")?;

        // Validate unit name
        if !UNIT_NAME_REGEX.is_match(&name)
            && !INSTANCE_NAME_REGEX.is_match(&name)
            && !name.contains("@.")
        {
            // Allow template files like myapp@.service
            if !name.ends_with(".service")
                && !name.ends_with(".socket")
                && !name.ends_with(".timer")
                && !name.ends_with(".path")
                && !name.ends_with(".mount")
                && !name.ends_with(".automount")
                && !name.ends_with(".swap")
                && !name.ends_with(".slice")
                && !name.ends_with(".scope")
                && !name.ends_with(".target")
            {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid unit name '{}'. Must be a valid systemd unit name (e.g., myapp.service, myapp@.timer)",
                    name
                )));
            }
        }

        // Detect unit type from name or explicit parameter
        let unit_type = if let Some(type_str) = params.get_string("unit_type")? {
            UnitType::from_str(&type_str)?
        } else {
            UnitType::from_filename(&name).ok_or_else(|| {
                ModuleError::InvalidParameter(format!(
                    "Could not detect unit type from name '{}'. Please specify unit_type explicitly.",
                    name
                ))
            })?
        };

        // Check if this is a template unit
        let is_template = name.contains("@.");

        // Check for instance name
        let instance = INSTANCE_NAME_REGEX
            .captures(&name)
            .map(|caps| caps.get(2).map_or("", |m| m.as_str()).to_string());

        // Parse state
        let state_str = params
            .get_string("state")?
            .unwrap_or_else(|| "present".to_string());
        let state = UnitState::from_str(&state_str)?;

        // Get content sources (mutually exclusive)
        let content = params.get_string("content")?;
        let template = params.get_string("template")?;
        let src = params.get_string("src")?;

        // Validate content sources
        let content_sources = [content.is_some(), template.is_some(), src.is_some()];
        let source_count = content_sources.iter().filter(|&&x| x).count();
        if state == UnitState::Present && source_count == 0 {
            return Err(ModuleError::MissingParameter(
                "One of 'content', 'template', or 'src' is required when state=present".to_string(),
            ));
        }
        if source_count > 1 {
            return Err(ModuleError::InvalidParameter(
                "Only one of 'content', 'template', or 'src' can be specified".to_string(),
            ));
        }

        // Parse running state
        let running = if let Some(running_str) = params.get_string("running")? {
            Some(RunningState::from_str(&running_str)?)
        } else {
            None
        };

        // Parse mode (default 0644)
        let mode = params.get_u32("mode")?.unwrap_or(0o644);

        // Parse unit path (default /etc/systemd/system)
        let unit_path = params
            .get_string("unit_path")?
            .unwrap_or_else(|| "/etc/systemd/system".to_string());

        // Security validation for unit_path
        let path = std::path::Path::new(&unit_path);
        if !path.is_absolute() {
            return Err(ModuleError::InvalidParameter(format!(
                "Invalid unit_path '{}': path must be absolute",
                unit_path
            )));
        }

        for component in path.components() {
            if matches!(component, std::path::Component::ParentDir) {
                return Err(ModuleError::InvalidParameter(format!(
                    "Invalid unit_path '{}': Path traversal detected",
                    unit_path
                )));
            }
        }

        Ok(Self {
            name,
            unit_type,
            is_template,
            instance,
            state,
            content,
            template,
            src,
            enabled: params.get_bool("enabled")?,
            running,
            daemon_reload: params.get_bool("daemon_reload")?,
            daemon_reexec: params.get_bool_or("daemon_reexec", false),
            force: params.get_bool_or("force", false),
            mode,
            owner: params.get_string("owner")?,
            group: params.get_string("group")?,
            unit_path,
            vars: params.get("vars").cloned(),
        })
    }

    /// Get the full path to the unit file
    fn unit_file_path(&self) -> String {
        format!("{}/{}", self.unit_path, self.name)
    }

    /// Get the unit name for systemctl commands (without path)
    fn systemctl_name(&self) -> &str {
        &self.name
    }
}

/// Module for systemd unit file management
pub struct SystemdUnitModule;

impl SystemdUnitModule {
    /// Build execute options with privilege escalation if needed
    fn build_execute_options(context: &ModuleContext) -> Option<ExecuteOptions> {
        if context.r#become {
            Some(ExecuteOptions {
                escalate: true,
                escalate_user: context.become_user.clone(),
                escalate_method: context.become_method.clone(),
                escalate_password: context.become_password.clone(),
                ..Default::default()
            })
        } else {
            None
        }
    }

    /// Build context for template rendering
    fn build_context(
        context: &ModuleContext,
        extra_vars: Option<&serde_json::Value>,
        config: &SystemdUnitConfig,
    ) -> serde_json::Value {
        let mut ctx_map = serde_json::Map::new();

        // Add variables from module context
        for (key, value) in &context.vars {
            ctx_map.insert(key.clone(), value.clone());
        }

        // Add facts
        ctx_map.insert(
            "ansible_facts".to_string(),
            serde_json::json!(&context.facts),
        );
        for (key, value) in &context.facts {
            ctx_map.insert(key.clone(), value.clone());
        }

        // Add unit-specific variables
        ctx_map.insert("unit_name".to_string(), serde_json::json!(config.name));
        ctx_map.insert(
            "unit_type".to_string(),
            serde_json::json!(config.unit_type.to_string()),
        );
        if let Some(ref instance) = config.instance {
            ctx_map.insert("instance".to_string(), serde_json::json!(instance));
        }

        // Add extra variables if provided
        if let Some(serde_json::Value::Object(vars)) = extra_vars {
            for (key, value) in vars {
                ctx_map.insert(key.clone(), value.clone());
            }
        }

        serde_json::Value::Object(ctx_map)
    }

    /// Render template content with context variables
    fn render_template(
        template_content: &str,
        context: &ModuleContext,
        extra_vars: Option<&serde_json::Value>,
        config: &SystemdUnitConfig,
    ) -> ModuleResult<String> {
        let ctx = Self::build_context(context, extra_vars, config);

        TEMPLATE_ENGINE
            .render_with_json(template_content, &ctx)
            .map_err(|e| ModuleError::TemplateError(format!("Failed to render template: {}", e)))
    }

    /// Get the content to write to the unit file
    fn get_unit_content(
        config: &SystemdUnitConfig,
        context: &ModuleContext,
    ) -> ModuleResult<String> {
        if let Some(ref content) = config.content {
            // Direct content - use as-is
            Ok(content.clone())
        } else if let Some(ref template) = config.template {
            // Inline template - render it
            Self::render_template(template, context, config.vars.as_ref(), config)
        } else if let Some(ref src) = config.src {
            // Source file - read and render
            let src_path = Path::new(src);
            if !src_path.exists() {
                return Err(ModuleError::ExecutionFailed(format!(
                    "Template source '{}' does not exist",
                    src
                )));
            }
            let template_content = fs::read_to_string(src_path)?;
            Self::render_template(&template_content, context, config.vars.as_ref(), config)
        } else {
            Err(ModuleError::MissingParameter(
                "No content source specified".to_string(),
            ))
        }
    }

    /// Execute the module with async connection
    async fn execute_async(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
    ) -> ModuleResult<ModuleOutput> {
        let config = SystemdUnitConfig::from_params(params)?;
        let unit_file_path = config.unit_file_path();
        let unit_path = Path::new(&unit_file_path);

        match config.state {
            UnitState::Present => {
                self.ensure_present(&config, context, connection, unit_path)
                    .await
            }
            UnitState::Absent => {
                self.ensure_absent(&config, context, connection, unit_path)
                    .await
            }
        }
    }

    /// Ensure unit file is present with correct content
    async fn ensure_present(
        &self,
        config: &SystemdUnitConfig,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
        unit_path: &Path,
    ) -> ModuleResult<ModuleOutput> {
        let desired_content = Self::get_unit_content(config, context)?;
        let unit_file_path = config.unit_file_path();

        // Check if unit file exists and get current content
        let current_content = if connection.path_exists(unit_path).await.unwrap_or(false) {
            connection
                .download_content(unit_path)
                .await
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        } else {
            None
        };

        let content_changed = match &current_content {
            Some(current) => current != &desired_content,
            None => true,
        };

        let mut changed = false;
        let mut messages = Vec::new();
        let mut needs_daemon_reload = false;

        // Handle daemon-reexec first (if requested)
        if config.daemon_reexec {
            if context.check_mode {
                messages.push("Would re-execute systemd daemon".to_string());
            } else {
                self.systemd_daemon_reexec(connection.as_ref(), context)
                    .await?;
                messages.push("Re-executed systemd daemon".to_string());
                changed = true;
            }
        }

        // Write unit file if content changed
        if content_changed {
            if context.check_mode {
                if current_content.is_some() {
                    messages.push(format!("Would update unit file '{}'", unit_file_path));
                } else {
                    messages.push(format!("Would create unit file '{}'", unit_file_path));
                }
                changed = true;
                needs_daemon_reload = true;
            } else {
                // Ensure unit directory exists
                let unit_dir = Path::new(&config.unit_path);
                if !connection.path_exists(unit_dir).await.unwrap_or(false) {
                    let options = Self::build_execute_options(context);
                    let mkdir_cmd = format!("mkdir -p '{}'", config.unit_path);
                    connection.execute(&mkdir_cmd, options).await.map_err(|e| {
                        ModuleError::ExecutionFailed(format!(
                            "Failed to create unit directory: {}",
                            e
                        ))
                    })?;
                }

                // Upload unit file
                let transfer_opts = TransferOptions {
                    mode: Some(config.mode),
                    create_dirs: true,
                    backup: false,
                    owner: config.owner.clone(),
                    group: config.group.clone(),
                };

                connection
                    .upload_content(desired_content.as_bytes(), unit_path, Some(transfer_opts))
                    .await
                    .map_err(|e| {
                        ModuleError::ExecutionFailed(format!("Failed to write unit file: {}", e))
                    })?;

                if current_content.is_some() {
                    messages.push(format!("Updated unit file '{}'", unit_file_path));
                } else {
                    messages.push(format!("Created unit file '{}'", unit_file_path));
                }
                changed = true;
                needs_daemon_reload = true;
            }
        }

        // Handle daemon-reload
        let should_reload = config.daemon_reload.unwrap_or(needs_daemon_reload);
        if should_reload {
            if context.check_mode {
                messages.push("Would reload systemd daemon".to_string());
            } else {
                self.systemd_daemon_reload(connection.as_ref(), context)
                    .await?;
                messages.push("Reloaded systemd daemon".to_string());
                changed = true;
            }
        }

        // Handle enabled state
        if let Some(should_enable) = config.enabled {
            let is_enabled = self
                .is_enabled(connection.as_ref(), config.systemctl_name(), context)
                .await
                .unwrap_or(false);

            if should_enable != is_enabled {
                if context.check_mode {
                    let action = if should_enable { "enable" } else { "disable" };
                    messages.push(format!("Would {} unit '{}'", action, config.name));
                    changed = true;
                } else {
                    let action = if should_enable { "enable" } else { "disable" };
                    self.systemctl_action(
                        connection.as_ref(),
                        config.systemctl_name(),
                        action,
                        context,
                    )
                    .await?;
                    messages.push(format!("{}d unit '{}'", action, config.name));
                    changed = true;
                }
            }
        }

        // Handle running state
        if let Some(ref running) = config.running {
            let is_active = self
                .is_active(connection.as_ref(), config.systemctl_name(), context)
                .await
                .unwrap_or(false);

            match running {
                RunningState::Started => {
                    if !is_active {
                        if context.check_mode {
                            messages.push(format!("Would start unit '{}'", config.name));
                            changed = true;
                        } else {
                            self.systemctl_action(
                                connection.as_ref(),
                                config.systemctl_name(),
                                "start",
                                context,
                            )
                            .await?;
                            messages.push(format!("Started unit '{}'", config.name));
                            changed = true;
                        }
                    }
                }
                RunningState::Stopped => {
                    if is_active {
                        if context.check_mode {
                            messages.push(format!("Would stop unit '{}'", config.name));
                            changed = true;
                        } else {
                            self.systemctl_action(
                                connection.as_ref(),
                                config.systemctl_name(),
                                "stop",
                                context,
                            )
                            .await?;
                            messages.push(format!("Stopped unit '{}'", config.name));
                            changed = true;
                        }
                    }
                }
                RunningState::Restarted => {
                    if context.check_mode {
                        messages.push(format!("Would restart unit '{}'", config.name));
                        changed = true;
                    } else {
                        self.systemctl_action(
                            connection.as_ref(),
                            config.systemctl_name(),
                            "restart",
                            context,
                        )
                        .await?;
                        messages.push(format!("Restarted unit '{}'", config.name));
                        changed = true;
                    }
                }
                RunningState::Reloaded => {
                    if context.check_mode {
                        messages.push(format!("Would reload unit '{}'", config.name));
                        changed = true;
                    } else {
                        // Try reload, fall back to restart
                        if self
                            .systemctl_action(
                                connection.as_ref(),
                                config.systemctl_name(),
                                "reload",
                                context,
                            )
                            .await
                            .is_err()
                        {
                            self.systemctl_action(
                                connection.as_ref(),
                                config.systemctl_name(),
                                "restart",
                                context,
                            )
                            .await?;
                            messages.push(format!(
                                "Restarted unit '{}' (reload not supported)",
                                config.name
                            ));
                        } else {
                            messages.push(format!("Reloaded unit '{}'", config.name));
                        }
                        changed = true;
                    }
                }
            }
        }

        let msg = if messages.is_empty() {
            format!("Unit '{}' is already in desired state", config.name)
        } else {
            messages.join(". ")
        };

        let output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        // Add output data
        let output = output
            .with_data("unit_name", serde_json::json!(config.name))
            .with_data("unit_type", serde_json::json!(config.unit_type.to_string()))
            .with_data("unit_path", serde_json::json!(unit_file_path))
            .with_data("content_changed", serde_json::json!(content_changed));

        // Add diff if in diff mode
        if context.diff_mode && content_changed {
            let before = current_content.unwrap_or_default();
            let diff = Diff::new(before, desired_content);
            return Ok(output.with_diff(diff));
        }

        Ok(output)
    }

    /// Ensure unit file is absent
    async fn ensure_absent(
        &self,
        config: &SystemdUnitConfig,
        context: &ModuleContext,
        connection: Arc<dyn Connection + Send + Sync>,
        unit_path: &Path,
    ) -> ModuleResult<ModuleOutput> {
        let unit_file_path = config.unit_file_path();

        // Check if unit file exists
        let exists = connection.path_exists(unit_path).await.unwrap_or(false);

        if !exists {
            return Ok(
                ModuleOutput::ok(format!("Unit file '{}' already absent", unit_file_path))
                    .with_data("unit_name", serde_json::json!(config.name))
                    .with_data("unit_path", serde_json::json!(unit_file_path)),
            );
        }

        let mut messages = Vec::new();
        #[allow(unused_assignments)]
        let mut changed = false;

        // Stop the unit if it's running
        let is_active = self
            .is_active(connection.as_ref(), config.systemctl_name(), context)
            .await
            .unwrap_or(false);

        if is_active {
            if context.check_mode {
                messages.push(format!("Would stop unit '{}'", config.name));
            } else {
                self.systemctl_action(
                    connection.as_ref(),
                    config.systemctl_name(),
                    "stop",
                    context,
                )
                .await?;
                messages.push(format!("Stopped unit '{}'", config.name));
            }
            // Note: changed will be set to true below when unit file is removed
        }

        // Disable the unit if enabled
        let is_enabled = self
            .is_enabled(connection.as_ref(), config.systemctl_name(), context)
            .await
            .unwrap_or(false);

        if is_enabled {
            if context.check_mode {
                messages.push(format!("Would disable unit '{}'", config.name));
            } else {
                self.systemctl_action(
                    connection.as_ref(),
                    config.systemctl_name(),
                    "disable",
                    context,
                )
                .await?;
                messages.push(format!("Disabled unit '{}'", config.name));
            }
            // Note: changed will be set to true below when unit file is removed
        }

        // Remove unit file
        if context.check_mode {
            messages.push(format!("Would remove unit file '{}'", unit_file_path));
            changed = true;
        } else {
            let options = Self::build_execute_options(context);
            let rm_cmd = format!("rm -f '{}'", unit_file_path);
            connection.execute(&rm_cmd, options).await.map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to remove unit file: {}", e))
            })?;
            messages.push(format!("Removed unit file '{}'", unit_file_path));
            changed = true;
        }

        // Reload daemon after removing unit
        if context.check_mode {
            messages.push("Would reload systemd daemon".to_string());
        } else {
            self.systemd_daemon_reload(connection.as_ref(), context)
                .await?;
            messages.push("Reloaded systemd daemon".to_string());
        }

        let msg = messages.join(". ");
        let output = if changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        Ok(output
            .with_data("unit_name", serde_json::json!(config.name))
            .with_data("unit_path", serde_json::json!(unit_file_path)))
    }

    /// Check if unit is active
    async fn is_active(
        &self,
        connection: &dyn Connection,
        unit: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let options = Self::build_execute_options(context);
        let cmd = format!("systemctl is-active '{}' >/dev/null 2>&1", unit);
        let result = connection.execute(&cmd, options).await;
        Ok(result.map(|r| r.success).unwrap_or(false))
    }

    /// Check if unit is enabled
    async fn is_enabled(
        &self,
        connection: &dyn Connection,
        unit: &str,
        context: &ModuleContext,
    ) -> ModuleResult<bool> {
        let options = Self::build_execute_options(context);
        let cmd = format!("systemctl is-enabled '{}' >/dev/null 2>&1", unit);
        let result = connection.execute(&cmd, options).await;
        Ok(result.map(|r| r.success).unwrap_or(false))
    }

    /// Execute systemctl action
    async fn systemctl_action(
        &self,
        connection: &dyn Connection,
        unit: &str,
        action: &str,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let options = Self::build_execute_options(context);
        let cmd = format!("systemctl {} '{}'", action, unit);
        let result = connection.execute(&cmd, options).await.map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to {} unit: {}", action, e))
        })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "systemctl {} '{}' failed: {}",
                action, unit, result.stderr
            )));
        }

        Ok(())
    }

    /// Reload systemd daemon
    async fn systemd_daemon_reload(
        &self,
        connection: &dyn Connection,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let options = Self::build_execute_options(context);
        let result = connection
            .execute("systemctl daemon-reload", options)
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to reload daemon: {}", e)))?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "systemctl daemon-reload failed: {}",
                result.stderr
            )));
        }

        Ok(())
    }

    /// Re-execute systemd manager
    async fn systemd_daemon_reexec(
        &self,
        connection: &dyn Connection,
        context: &ModuleContext,
    ) -> ModuleResult<()> {
        let options = Self::build_execute_options(context);
        let result = connection
            .execute("systemctl daemon-reexec", options)
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to re-execute daemon: {}", e))
            })?;

        if !result.success {
            return Err(ModuleError::ExecutionFailed(format!(
                "systemctl daemon-reexec failed: {}",
                result.stderr
            )));
        }

        Ok(())
    }
}

impl Module for SystemdUnitModule {
    fn name(&self) -> &'static str {
        "systemd_unit"
    }

    fn description(&self) -> &'static str {
        "Manage systemd unit files (service, socket, timer, path, etc.)"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::RemoteCommand
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Unit file operations are safe to parallelize across hosts
        ParallelizationHint::FullyParallel
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
        let connection = context.connection.clone().ok_or_else(|| {
            ModuleError::ExecutionFailed(
                "No connection available for systemd_unit module execution".to_string(),
            )
        })?;

        // Use tokio runtime to execute async code
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| ModuleError::ExecutionFailed("No tokio runtime available".to_string()))?;

        let params = params.clone();
        let context = context.clone();
        let module = self;

        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(module.execute_async(&params, &context, connection)))
                .join()
                .unwrap()
        })
    }
}

/// Helper module for creating common unit file templates
pub mod templates {
    /// Generate a basic service unit file
    pub fn service(
        description: &str,
        exec_start: &str,
        user: Option<&str>,
        wanted_by: Option<&str>,
    ) -> String {
        let mut content = String::new();
        content.push_str("[Unit]\n");
        content.push_str(&format!("Description={}\n", description));
        content.push_str("After=network.target\n");
        content.push_str("\n[Service]\n");
        content.push_str("Type=simple\n");
        content.push_str(&format!("ExecStart={}\n", exec_start));
        if let Some(u) = user {
            content.push_str(&format!("User={}\n", u));
        }
        content.push_str("Restart=on-failure\n");
        content.push_str("\n[Install]\n");
        content.push_str(&format!(
            "WantedBy={}\n",
            wanted_by.unwrap_or("multi-user.target")
        ));
        content
    }

    /// Generate a timer unit file
    pub fn timer(
        description: &str,
        on_calendar: Option<&str>,
        on_boot_sec: Option<&str>,
        on_unit_active_sec: Option<&str>,
        unit: &str,
    ) -> String {
        let mut content = String::new();
        content.push_str("[Unit]\n");
        content.push_str(&format!("Description={}\n", description));
        content.push_str("\n[Timer]\n");
        if let Some(cal) = on_calendar {
            content.push_str(&format!("OnCalendar={}\n", cal));
        }
        if let Some(boot) = on_boot_sec {
            content.push_str(&format!("OnBootSec={}\n", boot));
        }
        if let Some(active) = on_unit_active_sec {
            content.push_str(&format!("OnUnitActiveSec={}\n", active));
        }
        content.push_str(&format!("Unit={}\n", unit));
        content.push_str("Persistent=true\n");
        content.push_str("\n[Install]\n");
        content.push_str("WantedBy=timers.target\n");
        content
    }

    /// Generate a socket unit file
    pub fn socket(
        description: &str,
        listen_stream: Option<&str>,
        listen_datagram: Option<&str>,
        accept: bool,
    ) -> String {
        let mut content = String::new();
        content.push_str("[Unit]\n");
        content.push_str(&format!("Description={}\n", description));
        content.push_str("\n[Socket]\n");
        if let Some(stream) = listen_stream {
            content.push_str(&format!("ListenStream={}\n", stream));
        }
        if let Some(dgram) = listen_datagram {
            content.push_str(&format!("ListenDatagram={}\n", dgram));
        }
        content.push_str(&format!("Accept={}\n", if accept { "yes" } else { "no" }));
        content.push_str("\n[Install]\n");
        content.push_str("WantedBy=sockets.target\n");
        content
    }

    /// Generate a path unit file
    pub fn path(
        description: &str,
        path_exists: Option<&str>,
        path_exists_glob: Option<&str>,
        path_changed: Option<&str>,
        path_modified: Option<&str>,
        directory_not_empty: Option<&str>,
        unit: &str,
    ) -> String {
        let mut content = String::new();
        content.push_str("[Unit]\n");
        content.push_str(&format!("Description={}\n", description));
        content.push_str("\n[Path]\n");
        if let Some(p) = path_exists {
            content.push_str(&format!("PathExists={}\n", p));
        }
        if let Some(p) = path_exists_glob {
            content.push_str(&format!("PathExistsGlob={}\n", p));
        }
        if let Some(p) = path_changed {
            content.push_str(&format!("PathChanged={}\n", p));
        }
        if let Some(p) = path_modified {
            content.push_str(&format!("PathModified={}\n", p));
        }
        if let Some(p) = directory_not_empty {
            content.push_str(&format!("DirectoryNotEmpty={}\n", p));
        }
        content.push_str(&format!("Unit={}\n", unit));
        content.push_str("\n[Install]\n");
        content.push_str("WantedBy=paths.target\n");
        content
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_unit_type_from_str() {
        assert_eq!(UnitType::from_str("service").unwrap(), UnitType::Service);
        assert_eq!(UnitType::from_str("socket").unwrap(), UnitType::Socket);
        assert_eq!(UnitType::from_str("timer").unwrap(), UnitType::Timer);
        assert_eq!(UnitType::from_str("path").unwrap(), UnitType::Path);
        assert_eq!(UnitType::from_str("mount").unwrap(), UnitType::Mount);
        assert!(UnitType::from_str("invalid").is_err());
    }

    #[test]
    fn test_unit_type_from_filename() {
        assert_eq!(
            UnitType::from_filename("myapp.service"),
            Some(UnitType::Service)
        );
        assert_eq!(
            UnitType::from_filename("myapp.socket"),
            Some(UnitType::Socket)
        );
        assert_eq!(
            UnitType::from_filename("myapp.timer"),
            Some(UnitType::Timer)
        );
        assert_eq!(
            UnitType::from_filename("myapp@.service"),
            Some(UnitType::Service)
        );
        assert_eq!(
            UnitType::from_filename("myapp@instance.service"),
            Some(UnitType::Service)
        );
        assert_eq!(UnitType::from_filename("invalid"), None);
    }

    #[test]
    fn test_unit_state_from_str() {
        assert_eq!(UnitState::from_str("present").unwrap(), UnitState::Present);
        assert_eq!(UnitState::from_str("absent").unwrap(), UnitState::Absent);
        assert!(UnitState::from_str("invalid").is_err());
    }

    #[test]
    fn test_running_state_from_str() {
        assert_eq!(
            RunningState::from_str("started").unwrap(),
            RunningState::Started
        );
        assert_eq!(
            RunningState::from_str("running").unwrap(),
            RunningState::Started
        );
        assert_eq!(
            RunningState::from_str("stopped").unwrap(),
            RunningState::Stopped
        );
        assert_eq!(
            RunningState::from_str("restarted").unwrap(),
            RunningState::Restarted
        );
        assert_eq!(
            RunningState::from_str("reloaded").unwrap(),
            RunningState::Reloaded
        );
        assert!(RunningState::from_str("invalid").is_err());
    }

    #[test]
    fn test_config_from_params_basic() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert(
            "content".to_string(),
            serde_json::json!("[Unit]\nDescription=Test"),
        );

        let config = SystemdUnitConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "myapp.service");
        assert_eq!(config.unit_type, UnitType::Service);
        assert_eq!(config.state, UnitState::Present);
        assert!(!config.is_template);
        assert!(config.instance.is_none());
    }

    #[test]
    fn test_config_from_params_template_unit() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp@.service"));
        params.insert(
            "content".to_string(),
            serde_json::json!("[Unit]\nDescription=Test"),
        );

        let config = SystemdUnitConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "myapp@.service");
        assert!(config.is_template);
    }

    #[test]
    fn test_config_from_params_instance_unit() {
        let mut params = ModuleParams::new();
        params.insert(
            "name".to_string(),
            serde_json::json!("myapp@instance1.service"),
        );
        params.insert(
            "content".to_string(),
            serde_json::json!("[Unit]\nDescription=Test"),
        );

        let config = SystemdUnitConfig::from_params(&params).unwrap();
        assert_eq!(config.name, "myapp@instance1.service");
        assert!(!config.is_template);
        assert_eq!(config.instance, Some("instance1".to_string()));
    }

    #[test]
    fn test_config_missing_content_source() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("state".to_string(), serde_json::json!("present"));

        let result = SystemdUnitConfig::from_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_multiple_content_sources() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("content".to_string(), serde_json::json!("[Unit]"));
        params.insert("template".to_string(), serde_json::json!("[Unit]"));

        let result = SystemdUnitConfig::from_params(&params);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_absent_no_content_required() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("state".to_string(), serde_json::json!("absent"));

        let config = SystemdUnitConfig::from_params(&params).unwrap();
        assert_eq!(config.state, UnitState::Absent);
    }

    #[test]
    fn test_unit_file_path() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("content".to_string(), serde_json::json!("[Unit]"));

        let config = SystemdUnitConfig::from_params(&params).unwrap();
        assert_eq!(config.unit_file_path(), "/etc/systemd/system/myapp.service");
    }

    #[test]
    fn test_unit_file_path_custom() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("content".to_string(), serde_json::json!("[Unit]"));
        params.insert(
            "unit_path".to_string(),
            serde_json::json!("/usr/lib/systemd/system"),
        );

        let config = SystemdUnitConfig::from_params(&params).unwrap();
        assert_eq!(
            config.unit_file_path(),
            "/usr/lib/systemd/system/myapp.service"
        );
    }

    #[test]
    fn test_template_service() {
        let content = templates::service(
            "My Application",
            "/usr/bin/myapp",
            Some("myuser"),
            Some("multi-user.target"),
        );
        assert!(content.contains("Description=My Application"));
        assert!(content.contains("ExecStart=/usr/bin/myapp"));
        assert!(content.contains("User=myuser"));
        assert!(content.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn test_template_timer() {
        let content = templates::timer(
            "Run backup daily",
            Some("*-*-* 02:00:00"),
            None,
            None,
            "backup.service",
        );
        assert!(content.contains("Description=Run backup daily"));
        assert!(content.contains("OnCalendar=*-*-* 02:00:00"));
        assert!(content.contains("Unit=backup.service"));
        assert!(content.contains("Persistent=true"));
    }

    #[test]
    fn test_template_socket() {
        let content = templates::socket("My Application Socket", Some("0.0.0.0:8080"), None, false);
        assert!(content.contains("Description=My Application Socket"));
        assert!(content.contains("ListenStream=0.0.0.0:8080"));
        assert!(content.contains("Accept=no"));
    }

    #[test]
    fn test_template_path() {
        let content = templates::path(
            "Watch for config changes",
            None,
            None,
            Some("/etc/myapp/config.yaml"),
            None,
            None,
            "myapp-reload.service",
        );
        assert!(content.contains("Description=Watch for config changes"));
        assert!(content.contains("PathChanged=/etc/myapp/config.yaml"));
        assert!(content.contains("Unit=myapp-reload.service"));
    }

    #[test]
    fn test_module_metadata() {
        let module = SystemdUnitModule;
        assert_eq!(module.name(), "systemd_unit");
        assert_eq!(module.classification(), ModuleClassification::RemoteCommand);
        assert_eq!(module.required_params(), &["name"]);
    }

    #[test]
    fn test_unit_name_validation() {
        // Valid names
        assert!(UNIT_NAME_REGEX.is_match("myapp.service"));
        assert!(UNIT_NAME_REGEX.is_match("my-app.service"));
        assert!(UNIT_NAME_REGEX.is_match("my_app.service"));
        assert!(UNIT_NAME_REGEX.is_match("myapp123.timer"));
        assert!(UNIT_NAME_REGEX.is_match("my.app.socket"));

        // Invalid names
        assert!(!UNIT_NAME_REGEX.is_match("myapp")); // No extension
        assert!(!UNIT_NAME_REGEX.is_match("myapp.invalid")); // Invalid extension
        assert!(!UNIT_NAME_REGEX.is_match("")); // Empty
    }

    #[test]
    fn test_instance_name_regex() {
        // Valid instance names
        assert!(INSTANCE_NAME_REGEX.is_match("myapp@instance.service"));
        assert!(INSTANCE_NAME_REGEX.is_match("myapp@.service")); // Empty instance (template)
        assert!(INSTANCE_NAME_REGEX.is_match("my-app@test123.timer"));

        // Check captures
        let caps = INSTANCE_NAME_REGEX
            .captures("myapp@instance.service")
            .unwrap();
        assert_eq!(caps.get(1).unwrap().as_str(), "myapp");
        assert_eq!(caps.get(2).unwrap().as_str(), "instance");
        assert_eq!(caps.get(3).unwrap().as_str(), "service");
    }

    #[test]
    fn test_render_template_basic() {
        let template = "[Unit]\nDescription={{ description }}\n";
        let mut vars = HashMap::new();
        vars.insert("description".to_string(), serde_json::json!("Test Service"));

        let context = ModuleContext::default().with_vars(vars);
        let config = SystemdUnitConfig {
            name: "test.service".to_string(),
            unit_type: UnitType::Service,
            is_template: false,
            instance: None,
            state: UnitState::Present,
            content: None,
            template: Some(template.to_string()),
            src: None,
            enabled: None,
            running: None,
            daemon_reload: None,
            daemon_reexec: false,
            force: false,
            mode: 0o644,
            owner: None,
            group: None,
            unit_path: "/etc/systemd/system".to_string(),
            vars: None,
        };

        let result = SystemdUnitModule::render_template(template, &context, None, &config).unwrap();
        // MiniJinja might not preserve the trailing newline exactly as Tera did,
        // or the test expectation was relying on Tera behavior.
        // Let's trim both sides to be safe, or just check content.
        assert_eq!(result.trim(), "[Unit]\nDescription=Test Service");
    }

    #[test]
    fn test_render_template_with_default_filter() {
        let template = "[Service]\nType={{ service_type | default(value=\"simple\") }}\n";
        let context = ModuleContext::default();
        let config = SystemdUnitConfig {
            name: "test.service".to_string(),
            unit_type: UnitType::Service,
            is_template: false,
            instance: None,
            state: UnitState::Present,
            content: None,
            template: Some(template.to_string()),
            src: None,
            enabled: None,
            running: None,
            daemon_reload: None,
            daemon_reexec: false,
            force: false,
            mode: 0o644,
            owner: None,
            group: None,
            unit_path: "/etc/systemd/system".to_string(),
            vars: None,
        };

        let result = SystemdUnitModule::render_template(template, &context, None, &config).unwrap();
        // Adjust expectation for potential newline differences
        assert_eq!(result.trim(), "[Service]\nType=simple");
    }

    #[test]
    fn test_config_path_traversal() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("content".to_string(), serde_json::json!("[Unit]"));
        params.insert(
            "unit_path".to_string(),
            serde_json::json!("/etc/systemd/system/../.."),
        );

        let result = SystemdUnitConfig::from_params(&params);
        assert!(result.is_err(), "Should detect path traversal");
        let err = result.err().unwrap();
        match err {
            ModuleError::InvalidParameter(msg) => assert!(msg.contains("Path traversal detected")),
            _ => panic!("Unexpected error type: {:?}", err),
        }
    }

    #[test]
    fn test_config_relative_path() {
        let mut params = ModuleParams::new();
        params.insert("name".to_string(), serde_json::json!("myapp.service"));
        params.insert("content".to_string(), serde_json::json!("[Unit]"));
        params.insert(
            "unit_path".to_string(),
            serde_json::json!("etc/systemd/system"),
        );

        let result = SystemdUnitConfig::from_params(&params);
        assert!(result.is_err(), "Should require absolute path");
        let err = result.err().unwrap();
        match err {
            ModuleError::InvalidParameter(msg) => assert!(msg.contains("must be absolute")),
            _ => panic!("Unexpected error type: {:?}", err),
        }
    }
}
