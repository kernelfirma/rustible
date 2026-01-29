//! Change detection model for plan mode
//!
//! This module provides change prediction for core modules during plan mode.
//! It analyzes task arguments and current state to predict whether execution
//! would result in a change, without actually making modifications.
//!
//! ## Supported Modules
//!
//! - File operations: `file`, `copy`, `template`
//! - Package management: `apt`, `yum`, `dnf`, `package`
//! - Service management: `service`, `systemd`
//! - User/group management: `user`, `group`
//! - Line operations: `lineinfile`, `blockinfile`
//!
//! ## Usage
//!
//! ```ignore
//! use rustible::cli::change_detection::{ChangeDetectorRegistry, ChangePrediction};
//!
//! let registry = ChangeDetectorRegistry::new();
//! let prediction = registry.predict_change("file", &args, &connection).await;
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rustible::connection::Connection;

/// Predicted change result for a module execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PredictedChange {
    /// Resource will be created
    WillCreate,
    /// Resource will be modified
    WillModify,
    /// Resource will be deleted
    WillDelete,
    /// No change will occur
    NoChange,
    /// Cannot determine without execution
    Unknown,
    /// Check mode not supported for this module
    NotSupported,
}

impl PredictedChange {
    /// Check if this prediction indicates a change will occur
    pub fn will_change(&self) -> bool {
        matches!(
            self,
            PredictedChange::WillCreate | PredictedChange::WillModify | PredictedChange::WillDelete
        )
    }

    /// Get a human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            PredictedChange::WillCreate => "will create resource",
            PredictedChange::WillModify => "will modify resource",
            PredictedChange::WillDelete => "will delete resource",
            PredictedChange::NoChange => "no change required",
            PredictedChange::Unknown => "change status unknown",
            PredictedChange::NotSupported => "check mode not supported",
        }
    }
}

/// Detailed prediction result with reasoning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePrediction {
    /// The predicted change type
    pub change: PredictedChange,
    /// The resource being affected
    pub resource: Option<String>,
    /// Reason for the prediction
    pub reason: String,
    /// Current state of the resource (if determinable)
    pub current_state: Option<JsonValue>,
    /// Desired state based on arguments
    pub desired_state: Option<JsonValue>,
    /// Confidence level (0.0 to 1.0)
    pub confidence: f64,
}

impl ChangePrediction {
    /// Create a new prediction
    pub fn new(change: PredictedChange, reason: impl Into<String>) -> Self {
        Self {
            change,
            resource: None,
            reason: reason.into(),
            current_state: None,
            desired_state: None,
            confidence: 1.0,
        }
    }

    /// Set the resource being affected
    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }

    /// Set the current and desired states
    pub fn with_states(mut self, current: JsonValue, desired: JsonValue) -> Self {
        self.current_state = Some(current);
        self.desired_state = Some(desired);
        self
    }

    /// Set confidence level
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Create an unknown prediction
    pub fn unknown(reason: impl Into<String>) -> Self {
        Self::new(PredictedChange::Unknown, reason)
    }

    /// Create a not supported prediction
    pub fn not_supported(module: &str) -> Self {
        Self::new(
            PredictedChange::NotSupported,
            format!("Module '{}' does not support change detection", module),
        )
    }
}

/// Trait for module-specific change detection
#[async_trait]
pub trait ChangeDetector: Send + Sync {
    /// Predict whether the module execution will result in a change
    async fn predict(
        &self,
        args: &JsonValue,
        connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction;

    /// Get the module name this detector handles
    fn module_name(&self) -> &'static str;
}

/// Registry of change detectors
pub struct ChangeDetectorRegistry {
    detectors: HashMap<String, Box<dyn ChangeDetector>>,
}

impl Default for ChangeDetectorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeDetectorRegistry {
    /// Create a new registry with all built-in detectors
    pub fn new() -> Self {
        let mut registry = Self {
            detectors: HashMap::new(),
        };

        // Register all built-in detectors
        registry.register(Box::new(FileChangeDetector));
        registry.register(Box::new(CopyChangeDetector));
        registry.register(Box::new(TemplateChangeDetector));
        registry.register(Box::new(PackageChangeDetector::new("apt")));
        registry.register(Box::new(PackageChangeDetector::new("yum")));
        registry.register(Box::new(PackageChangeDetector::new("dnf")));
        registry.register(Box::new(PackageChangeDetector::new("package")));
        registry.register(Box::new(ServiceChangeDetector::new("service")));
        registry.register(Box::new(ServiceChangeDetector::new("systemd")));
        registry.register(Box::new(UserChangeDetector));
        registry.register(Box::new(GroupChangeDetector));
        registry.register(Box::new(LineinfileChangeDetector));
        registry.register(Box::new(BlockinfileChangeDetector));
        registry.register(Box::new(DebugChangeDetector));
        registry.register(Box::new(SetFactChangeDetector));

        // Register FQCN variants
        let fqcn_modules = [
            "file",
            "copy",
            "template",
            "apt",
            "yum",
            "dnf",
            "package",
            "service",
            "systemd",
            "user",
            "group",
            "lineinfile",
            "blockinfile",
            "debug",
            "set_fact",
        ];

        for module in fqcn_modules {
            let builtin_key = format!("ansible.builtin.{}", module);
            let legacy_key = format!("ansible.legacy.{}", module);

            // Clone detectors for FQCN variants
            if let Some(detector) = registry.detectors.get(module).map(|d| d.module_name()) {
                match detector {
                    "file" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(FileChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(FileChangeDetector));
                    }
                    "copy" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(CopyChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(CopyChangeDetector));
                    }
                    "template" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(TemplateChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(TemplateChangeDetector));
                    }
                    "apt" | "yum" | "dnf" | "package" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(PackageChangeDetector::new(detector)));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(PackageChangeDetector::new(detector)));
                    }
                    "service" | "systemd" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(ServiceChangeDetector::new(detector)));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(ServiceChangeDetector::new(detector)));
                    }
                    "user" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(UserChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(UserChangeDetector));
                    }
                    "group" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(GroupChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(GroupChangeDetector));
                    }
                    "lineinfile" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(LineinfileChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(LineinfileChangeDetector));
                    }
                    "blockinfile" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(BlockinfileChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(BlockinfileChangeDetector));
                    }
                    "debug" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(DebugChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(DebugChangeDetector));
                    }
                    "set_fact" => {
                        registry
                            .detectors
                            .insert(builtin_key, Box::new(SetFactChangeDetector));
                        registry
                            .detectors
                            .insert(legacy_key, Box::new(SetFactChangeDetector));
                    }
                    _ => {}
                }
            }
        }

        registry
    }

    /// Register a change detector
    pub fn register(&mut self, detector: Box<dyn ChangeDetector>) {
        self.detectors
            .insert(detector.module_name().to_string(), detector);
    }

    /// Get a detector for a module
    pub fn get(&self, module: &str) -> Option<&dyn ChangeDetector> {
        self.detectors.get(module).map(|d| d.as_ref())
    }

    /// Check if a module has a registered detector
    pub fn has_detector(&self, module: &str) -> bool {
        self.detectors.contains_key(module)
    }

    /// Predict change for a module
    pub async fn predict_change(
        &self,
        module: &str,
        args: &JsonValue,
        connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        if let Some(detector) = self.get(module) {
            detector.predict(args, connection).await
        } else {
            ChangePrediction::not_supported(module)
        }
    }
}

// ============================================================================
// File Module Change Detector
// ============================================================================

struct FileChangeDetector;

#[async_trait]
impl ChangeDetector for FileChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let path = args
            .get("path")
            .or_else(|| args.get("dest"))
            .and_then(|v| v.as_str());

        let Some(path_str) = path else {
            return ChangePrediction::unknown("No path specified");
        };

        let state = args.get("state").and_then(|v| v.as_str()).unwrap_or("file");

        let path = Path::new(path_str);
        let exists = path.exists();

        let prediction = match state {
            "absent" => {
                if exists {
                    ChangePrediction::new(
                        PredictedChange::WillDelete,
                        "Path exists and will be removed",
                    )
                } else {
                    ChangePrediction::new(PredictedChange::NoChange, "Path does not exist")
                }
            }
            "directory" => {
                if exists {
                    if path.is_dir() {
                        // Check for attribute changes (mode, owner, group)
                        if has_attribute_changes(args, path) {
                            ChangePrediction::new(
                                PredictedChange::WillModify,
                                "Directory exists but attributes differ",
                            )
                        } else {
                            ChangePrediction::new(
                                PredictedChange::NoChange,
                                "Directory already exists with correct attributes",
                            )
                        }
                    } else {
                        ChangePrediction::new(
                            PredictedChange::WillModify,
                            "Path exists but is not a directory",
                        )
                    }
                } else {
                    ChangePrediction::new(PredictedChange::WillCreate, "Directory will be created")
                }
            }
            "file" => {
                if exists {
                    if path.is_file() {
                        if has_attribute_changes(args, path) {
                            ChangePrediction::new(
                                PredictedChange::WillModify,
                                "File exists but attributes differ",
                            )
                        } else {
                            ChangePrediction::new(
                                PredictedChange::NoChange,
                                "File already exists with correct attributes",
                            )
                        }
                    } else {
                        ChangePrediction::new(
                            PredictedChange::WillModify,
                            "Path exists but is not a regular file",
                        )
                    }
                } else {
                    // file state requires the file to already exist
                    ChangePrediction::unknown(
                        "File does not exist (file state requires existing file)",
                    )
                }
            }
            "link" | "hard" => {
                let src = args.get("src").and_then(|v| v.as_str());
                if src.is_none() {
                    return ChangePrediction::unknown("Link source not specified");
                }
                if exists {
                    if path.is_symlink() || state == "hard" {
                        // Would need to check link target
                        ChangePrediction::new(
                            PredictedChange::Unknown,
                            "Link exists, target check required",
                        )
                        .with_confidence(0.5)
                    } else {
                        ChangePrediction::new(
                            PredictedChange::WillModify,
                            "Path exists but is not a link",
                        )
                    }
                } else {
                    ChangePrediction::new(PredictedChange::WillCreate, "Link will be created")
                }
            }
            "touch" => ChangePrediction::new(
                PredictedChange::WillModify,
                "Touch always updates timestamps",
            ),
            _ => ChangePrediction::unknown(format!("Unknown state: {}", state)),
        };

        prediction.with_resource(path_str)
    }

    fn module_name(&self) -> &'static str {
        "file"
    }
}

/// Check if file attributes need to change
fn has_attribute_changes(args: &JsonValue, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };

    // Check mode
    if let Some(mode) = args.get("mode").and_then(parse_mode) {
        let current_mode = meta.mode() & 0o7777;
        if current_mode != mode {
            return true;
        }
    }

    // Check owner (would need username resolution for full check)
    if args.get("owner").is_some() {
        // Simplified - assume change might be needed
        return true;
    }

    // Check group (would need group name resolution for full check)
    if args.get("group").is_some() {
        return true;
    }

    false
}

/// Parse mode from string or integer
fn parse_mode(value: &JsonValue) -> Option<u32> {
    match value {
        JsonValue::Number(n) => n.as_u64().map(|n| n as u32),
        JsonValue::String(s) => {
            // Handle octal strings like "0755"
            if s.starts_with('0') {
                u32::from_str_radix(s.trim_start_matches('0'), 8).ok()
            } else {
                s.parse().ok()
            }
        }
        _ => None,
    }
}

// ============================================================================
// Copy Module Change Detector
// ============================================================================

struct CopyChangeDetector;

#[async_trait]
impl ChangeDetector for CopyChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let dest = args.get("dest").and_then(|v| v.as_str());

        let Some(dest_str) = dest else {
            return ChangePrediction::unknown("No destination specified");
        };

        let path = Path::new(dest_str);

        if !path.exists() {
            return ChangePrediction::new(
                PredictedChange::WillCreate,
                "Destination does not exist",
            )
            .with_resource(dest_str);
        }

        // Check if force=no
        let force = args.get("force").and_then(|v| v.as_bool()).unwrap_or(true);

        if !force {
            return ChangePrediction::new(
                PredictedChange::NoChange,
                "Destination exists and force=no",
            )
            .with_resource(dest_str);
        }

        // If content is specified, we'd need to compare
        if args.get("content").is_some() {
            return ChangePrediction::new(
                PredictedChange::Unknown,
                "Content comparison required (would need to read existing file)",
            )
            .with_resource(dest_str)
            .with_confidence(0.5);
        }

        // If src is specified, we'd need to compare files
        if args.get("src").is_some() {
            return ChangePrediction::new(
                PredictedChange::Unknown,
                "Source/destination comparison required",
            )
            .with_resource(dest_str)
            .with_confidence(0.5);
        }

        ChangePrediction::unknown("Insufficient information for prediction").with_resource(dest_str)
    }

    fn module_name(&self) -> &'static str {
        "copy"
    }
}

// ============================================================================
// Template Module Change Detector
// ============================================================================

struct TemplateChangeDetector;

#[async_trait]
impl ChangeDetector for TemplateChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let dest = args.get("dest").and_then(|v| v.as_str());

        let Some(dest_str) = dest else {
            return ChangePrediction::unknown("No destination specified");
        };

        let path = Path::new(dest_str);

        if !path.exists() {
            return ChangePrediction::new(
                PredictedChange::WillCreate,
                "Destination does not exist",
            )
            .with_resource(dest_str);
        }

        // Templates always require rendering and comparison
        ChangePrediction::new(
            PredictedChange::Unknown,
            "Template rendering and comparison required",
        )
        .with_resource(dest_str)
        .with_confidence(0.5)
    }

    fn module_name(&self) -> &'static str {
        "template"
    }
}

// ============================================================================
// Package Module Change Detector
// ============================================================================

struct PackageChangeDetector {
    module_name: &'static str,
}

impl PackageChangeDetector {
    fn new(name: &'static str) -> Self {
        Self { module_name: name }
    }
}

#[async_trait]
impl ChangeDetector for PackageChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("pkg").and_then(|v| v.as_str()));

        let Some(pkg_name) = name else {
            return ChangePrediction::unknown("No package name specified");
        };

        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present");

        // Package detection requires querying the package manager
        // In plan mode without connection, we can only make assumptions
        let prediction = match state {
            "absent" | "removed" => ChangePrediction::new(
                PredictedChange::Unknown,
                "Package removal requires checking if installed",
            )
            .with_confidence(0.3),
            "present" | "installed" => ChangePrediction::new(
                PredictedChange::Unknown,
                "Package installation requires checking if already installed",
            )
            .with_confidence(0.3),
            "latest" => ChangePrediction::new(
                PredictedChange::Unknown,
                "Latest state requires version comparison",
            )
            .with_confidence(0.2),
            _ => ChangePrediction::unknown(format!("Unknown package state: {}", state)),
        };

        prediction.with_resource(pkg_name)
    }

    fn module_name(&self) -> &'static str {
        self.module_name
    }
}

// ============================================================================
// Service Module Change Detector
// ============================================================================

struct ServiceChangeDetector {
    module_name: &'static str,
}

impl ServiceChangeDetector {
    fn new(name: &'static str) -> Self {
        Self { module_name: name }
    }
}

#[async_trait]
impl ChangeDetector for ServiceChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let name = args.get("name").and_then(|v| v.as_str());

        let Some(svc_name) = name else {
            return ChangePrediction::unknown("No service name specified");
        };

        let state = args.get("state").and_then(|v| v.as_str());
        let enabled = args.get("enabled").and_then(|v| v.as_bool());

        // Service state requires querying systemctl/service
        let mut reasons = Vec::new();

        if let Some(s) = state {
            reasons.push(format!("state={}", s));
        }
        if let Some(e) = enabled {
            reasons.push(format!("enabled={}", e));
        }

        if reasons.is_empty() {
            return ChangePrediction::new(
                PredictedChange::NoChange,
                "No state or enabled specified",
            )
            .with_resource(svc_name);
        }

        ChangePrediction::new(
            PredictedChange::Unknown,
            format!("Service state check required ({})", reasons.join(", ")),
        )
        .with_resource(svc_name)
        .with_confidence(0.3)
    }

    fn module_name(&self) -> &'static str {
        self.module_name
    }
}

// ============================================================================
// User Module Change Detector
// ============================================================================

struct UserChangeDetector;

#[async_trait]
impl ChangeDetector for UserChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let name = args.get("name").and_then(|v| v.as_str());

        let Some(user_name) = name else {
            return ChangePrediction::unknown("No username specified");
        };

        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present");

        // Check if user exists locally (simplified)
        let user_exists = std::process::Command::new("id")
            .arg(user_name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let prediction = match state {
            "absent" => {
                if user_exists {
                    ChangePrediction::new(
                        PredictedChange::WillDelete,
                        "User exists and will be removed",
                    )
                } else {
                    ChangePrediction::new(PredictedChange::NoChange, "User does not exist")
                }
            }
            "present" => {
                if user_exists {
                    // Would need to check all attributes
                    ChangePrediction::new(
                        PredictedChange::Unknown,
                        "User exists, attribute check required",
                    )
                    .with_confidence(0.5)
                } else {
                    ChangePrediction::new(PredictedChange::WillCreate, "User will be created")
                }
            }
            _ => ChangePrediction::unknown(format!("Unknown user state: {}", state)),
        };

        prediction.with_resource(user_name)
    }

    fn module_name(&self) -> &'static str {
        "user"
    }
}

// ============================================================================
// Group Module Change Detector
// ============================================================================

struct GroupChangeDetector;

#[async_trait]
impl ChangeDetector for GroupChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let name = args.get("name").and_then(|v| v.as_str());

        let Some(group_name) = name else {
            return ChangePrediction::unknown("No group name specified");
        };

        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present");

        // Check if group exists locally (simplified)
        let group_exists = std::process::Command::new("getent")
            .args(["group", group_name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let prediction = match state {
            "absent" => {
                if group_exists {
                    ChangePrediction::new(
                        PredictedChange::WillDelete,
                        "Group exists and will be removed",
                    )
                } else {
                    ChangePrediction::new(PredictedChange::NoChange, "Group does not exist")
                }
            }
            "present" => {
                if group_exists {
                    ChangePrediction::new(
                        PredictedChange::Unknown,
                        "Group exists, attribute check required",
                    )
                    .with_confidence(0.5)
                } else {
                    ChangePrediction::new(PredictedChange::WillCreate, "Group will be created")
                }
            }
            _ => ChangePrediction::unknown(format!("Unknown group state: {}", state)),
        };

        prediction.with_resource(group_name)
    }

    fn module_name(&self) -> &'static str {
        "group"
    }
}

// ============================================================================
// Lineinfile Module Change Detector
// ============================================================================

struct LineinfileChangeDetector;

#[async_trait]
impl ChangeDetector for LineinfileChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let path = args
            .get("path")
            .or_else(|| args.get("dest"))
            .and_then(|v| v.as_str());

        let Some(path_str) = path else {
            return ChangePrediction::unknown("No path specified");
        };

        let file_path = Path::new(path_str);

        if !file_path.exists() {
            let create = args
                .get("create")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            return if create {
                ChangePrediction::new(
                    PredictedChange::WillCreate,
                    "File does not exist, will be created",
                )
                .with_resource(path_str)
            } else {
                ChangePrediction::unknown("File does not exist and create=false")
                    .with_resource(path_str)
            };
        }

        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present");

        // Would need to read file and check for line
        ChangePrediction::new(
            PredictedChange::Unknown,
            format!("Line {} requires file content check", state),
        )
        .with_resource(path_str)
        .with_confidence(0.5)
    }

    fn module_name(&self) -> &'static str {
        "lineinfile"
    }
}

// ============================================================================
// Blockinfile Module Change Detector
// ============================================================================

struct BlockinfileChangeDetector;

#[async_trait]
impl ChangeDetector for BlockinfileChangeDetector {
    async fn predict(
        &self,
        args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        let path = args
            .get("path")
            .or_else(|| args.get("dest"))
            .and_then(|v| v.as_str());

        let Some(path_str) = path else {
            return ChangePrediction::unknown("No path specified");
        };

        let file_path = Path::new(path_str);

        if !file_path.exists() {
            let create = args
                .get("create")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            return if create {
                ChangePrediction::new(
                    PredictedChange::WillCreate,
                    "File does not exist, will be created",
                )
                .with_resource(path_str)
            } else {
                ChangePrediction::unknown("File does not exist and create=false")
                    .with_resource(path_str)
            };
        }

        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present");

        // Would need to read file and check for block markers
        ChangePrediction::new(
            PredictedChange::Unknown,
            format!("Block {} requires file content check", state),
        )
        .with_resource(path_str)
        .with_confidence(0.5)
    }

    fn module_name(&self) -> &'static str {
        "blockinfile"
    }
}

// ============================================================================
// Debug Module Change Detector (always no-change)
// ============================================================================

struct DebugChangeDetector;

#[async_trait]
impl ChangeDetector for DebugChangeDetector {
    async fn predict(
        &self,
        _args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        ChangePrediction::new(
            PredictedChange::NoChange,
            "Debug module does not make changes",
        )
    }

    fn module_name(&self) -> &'static str {
        "debug"
    }
}

// ============================================================================
// SetFact Module Change Detector (always no-change to system)
// ============================================================================

struct SetFactChangeDetector;

#[async_trait]
impl ChangeDetector for SetFactChangeDetector {
    async fn predict(
        &self,
        _args: &JsonValue,
        _connection: Option<&Arc<dyn Connection + Send + Sync>>,
    ) -> ChangePrediction {
        ChangePrediction::new(
            PredictedChange::NoChange,
            "set_fact only modifies runtime variables",
        )
    }

    fn module_name(&self) -> &'static str {
        "set_fact"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_file_detector_absent() {
        let detector = FileChangeDetector;
        let args = json!({
            "path": "/nonexistent/path/for/testing",
            "state": "absent"
        });

        let prediction = detector.predict(&args, None).await;
        assert_eq!(prediction.change, PredictedChange::NoChange);
    }

    #[tokio::test]
    async fn test_file_detector_directory_create() {
        let detector = FileChangeDetector;
        let args = json!({
            "path": "/nonexistent/directory/for/testing",
            "state": "directory"
        });

        let prediction = detector.predict(&args, None).await;
        assert_eq!(prediction.change, PredictedChange::WillCreate);
    }

    #[tokio::test]
    async fn test_copy_detector_missing_dest() {
        let detector = CopyChangeDetector;
        let args = json!({
            "src": "/some/source"
        });

        let prediction = detector.predict(&args, None).await;
        assert_eq!(prediction.change, PredictedChange::Unknown);
    }

    #[tokio::test]
    async fn test_debug_detector() {
        let detector = DebugChangeDetector;
        let args = json!({
            "msg": "Hello world"
        });

        let prediction = detector.predict(&args, None).await;
        assert_eq!(prediction.change, PredictedChange::NoChange);
    }

    #[tokio::test]
    async fn test_set_fact_detector() {
        let detector = SetFactChangeDetector;
        let args = json!({
            "my_var": "value"
        });

        let prediction = detector.predict(&args, None).await;
        assert_eq!(prediction.change, PredictedChange::NoChange);
    }

    #[tokio::test]
    async fn test_registry_lookup() {
        let registry = ChangeDetectorRegistry::new();

        assert!(registry.has_detector("file"));
        assert!(registry.has_detector("copy"));
        assert!(registry.has_detector("ansible.builtin.debug"));
        assert!(!registry.has_detector("nonexistent_module"));
    }

    #[tokio::test]
    async fn test_registry_predict() {
        let registry = ChangeDetectorRegistry::new();

        let prediction = registry
            .predict_change("debug", &json!({"msg": "test"}), None)
            .await;
        assert_eq!(prediction.change, PredictedChange::NoChange);

        let prediction = registry
            .predict_change("unknown_module", &json!({}), None)
            .await;
        assert_eq!(prediction.change, PredictedChange::NotSupported);
    }

    #[test]
    fn test_predicted_change_will_change() {
        assert!(PredictedChange::WillCreate.will_change());
        assert!(PredictedChange::WillModify.will_change());
        assert!(PredictedChange::WillDelete.will_change());
        assert!(!PredictedChange::NoChange.will_change());
        assert!(!PredictedChange::Unknown.will_change());
        assert!(!PredictedChange::NotSupported.will_change());
    }

    #[test]
    fn test_parse_mode() {
        assert_eq!(parse_mode(&json!(755)), Some(755));
        assert_eq!(parse_mode(&json!("0755")), Some(493)); // 0o755 = 493
        assert_eq!(parse_mode(&json!("755")), Some(755));
        assert_eq!(parse_mode(&json!(null)), None);
    }
}
