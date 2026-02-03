//! Rollback and State Management Module
//!
//! Provides mechanisms for tracking state changes and rolling back on failure:
//!
//! - **State Snapshots**: Capture system state before operations
//! - **Change Tracking**: Record all state changes during execution
//! - **Rollback Plans**: Generate and execute rollback sequences
//! - **Undo Operations**: Define reversible operations
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::recovery::rollback::{RollbackManager, StateChange};
//!
//! let mut manager = RollbackManager::new();
//! let context = manager.begin_context();
//!
//! // Record state changes
//! manager.record_change(&context.id, StateChange::FileCreated {
//!     path: "/etc/nginx/conf.d/app.conf".into(),
//!     backup_path: Some("/tmp/backup/app.conf".into()),
//! })?;
//!
//! // On failure, rollback
//! let plan = manager.create_rollback_plan(&context.id)?;
//! for action in &plan.actions {
//!     manager.execute_rollback_action(action).await?;
//! }
//! manager.complete_rollback(&context.id)?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::modules::ModuleRegistry;

/// Error type for rollback operations
#[derive(Error, Debug)]
pub enum RollbackError {
    #[error("Context not found: {0}")]
    ContextNotFound(String),

    #[error("Rollback already in progress for context: {0}")]
    RollbackInProgress(String),

    #[error("Rollback failed: {0}")]
    RollbackFailed(String),

    #[error("Cannot rollback: {0}")]
    CannotRollback(String),

    #[error("State snapshot failed: {0}")]
    SnapshotFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// State of a rollback context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RollbackState {
    /// Context is active and recording changes
    Active,
    /// Rollback is in progress
    RollingBack,
    /// Rollback completed successfully
    RolledBack,
    /// Rollback failed
    Failed,
    /// Context committed (no rollback needed)
    Committed,
}

/// A state change that can be rolled back
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StateChange {
    /// A file was created
    FileCreated {
        path: PathBuf,
        backup_path: Option<PathBuf>,
    },

    /// A file was modified
    FileModified {
        path: PathBuf,
        backup_path: PathBuf,
        original_content_hash: Option<String>,
    },

    /// A file was deleted
    FileDeleted { path: PathBuf, backup_path: PathBuf },

    /// A directory was created
    DirectoryCreated { path: PathBuf },

    /// A service state was changed
    ServiceStateChanged {
        service: String,
        previous_state: String,
        new_state: String,
    },

    /// A package was installed
    PackageInstalled {
        name: String,
        version: Option<String>,
    },

    /// A package was removed
    PackageRemoved {
        name: String,
        version: Option<String>,
    },

    /// A user was created
    UserCreated { username: String },

    /// A user was modified
    UserModified {
        username: String,
        previous_state: serde_json::Value,
    },

    /// A user was deleted
    UserDeleted {
        username: String,
        backup_data: serde_json::Value,
    },

    /// A custom state change
    Custom {
        description: String,
        undo_command: Option<String>,
        undo_data: Option<serde_json::Value>,
    },
}

impl StateChange {
    /// Get a description of this state change
    pub fn description(&self) -> String {
        match self {
            StateChange::FileCreated { path, .. } => {
                format!("Created file: {}", path.display())
            }
            StateChange::FileModified { path, .. } => {
                format!("Modified file: {}", path.display())
            }
            StateChange::FileDeleted { path, .. } => {
                format!("Deleted file: {}", path.display())
            }
            StateChange::DirectoryCreated { path } => {
                format!("Created directory: {}", path.display())
            }
            StateChange::ServiceStateChanged {
                service,
                previous_state,
                new_state,
            } => {
                format!(
                    "Changed service '{}' from {} to {}",
                    service, previous_state, new_state
                )
            }
            StateChange::PackageInstalled { name, version } => {
                if let Some(v) = version {
                    format!("Installed package: {}={}", name, v)
                } else {
                    format!("Installed package: {}", name)
                }
            }
            StateChange::PackageRemoved { name, .. } => {
                format!("Removed package: {}", name)
            }
            StateChange::UserCreated { username } => {
                format!("Created user: {}", username)
            }
            StateChange::UserModified { username, .. } => {
                format!("Modified user: {}", username)
            }
            StateChange::UserDeleted { username, .. } => {
                format!("Deleted user: {}", username)
            }
            StateChange::Custom { description, .. } => description.clone(),
        }
    }
}

/// An operation to undo a state change
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UndoOperation {
    /// Delete a file
    DeleteFile { path: PathBuf },

    /// Restore a file from backup
    RestoreFile { path: PathBuf, backup_path: PathBuf },

    /// Delete a directory
    DeleteDirectory { path: PathBuf, recursive: bool },

    /// Change service state
    ChangeServiceState {
        service: String,
        target_state: String,
    },

    /// Remove a package
    RemovePackage { name: String },

    /// Install a package
    InstallPackage {
        name: String,
        version: Option<String>,
    },

    /// Delete a user
    DeleteUser { username: String },

    /// Restore user from backup
    RestoreUser {
        username: String,
        backup_data: serde_json::Value,
    },

    /// Execute a custom command
    ExecuteCommand { command: String, args: Vec<String> },

    /// No-op (for changes that can't be undone)
    NoOp { reason: String },
}

/// A snapshot of system state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Unique identifier
    pub id: String,

    /// Timestamp when snapshot was taken
    pub timestamp: u64,

    /// Description of the snapshot
    pub description: Option<String>,

    /// Captured state data
    pub data: HashMap<String, serde_json::Value>,
}

impl StateSnapshot {
    /// Create a new state snapshot
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let random: u32 = rand::random();

        Self {
            id: format!("snap-{}-{:08x}", timestamp, random),
            timestamp,
            description: None,
            data: HashMap::new(),
        }
    }

    /// Add a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Store state data
    pub fn store(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.data.insert(key.into(), value);
    }

    /// Retrieve state data
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.data.get(key)
    }
}

impl Default for StateSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

/// An action in a rollback plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackAction {
    /// The undo operation to perform
    pub operation: UndoOperation,

    /// Description of what this action does
    pub description: String,

    /// Priority (higher = execute first during rollback)
    pub priority: i32,

    /// Whether this action is critical (failure aborts rollback)
    pub critical: bool,

    /// Original state change this undoes
    pub original_change: Option<StateChange>,
}

impl RollbackAction {
    /// Create a rollback action from a state change
    pub fn from_state_change(change: &StateChange) -> Self {
        let (operation, description, priority) = match change {
            StateChange::FileCreated { path, .. } => (
                UndoOperation::DeleteFile { path: path.clone() },
                format!("Delete created file: {}", path.display()),
                10,
            ),

            StateChange::FileModified {
                path, backup_path, ..
            } => (
                UndoOperation::RestoreFile {
                    path: path.clone(),
                    backup_path: backup_path.clone(),
                },
                format!("Restore file from backup: {}", path.display()),
                20,
            ),

            StateChange::FileDeleted { path, backup_path } => (
                UndoOperation::RestoreFile {
                    path: path.clone(),
                    backup_path: backup_path.clone(),
                },
                format!("Restore deleted file: {}", path.display()),
                20,
            ),

            StateChange::DirectoryCreated { path } => (
                UndoOperation::DeleteDirectory {
                    path: path.clone(),
                    recursive: true,
                },
                format!("Delete created directory: {}", path.display()),
                5,
            ),

            StateChange::ServiceStateChanged {
                service,
                previous_state,
                ..
            } => (
                UndoOperation::ChangeServiceState {
                    service: service.clone(),
                    target_state: previous_state.clone(),
                },
                format!("Restore service '{}' to state: {}", service, previous_state),
                30,
            ),

            StateChange::PackageInstalled { name, .. } => (
                UndoOperation::RemovePackage { name: name.clone() },
                format!("Remove installed package: {}", name),
                15,
            ),

            StateChange::PackageRemoved { name, version } => (
                UndoOperation::InstallPackage {
                    name: name.clone(),
                    version: version.clone(),
                },
                format!("Reinstall removed package: {}", name),
                15,
            ),

            StateChange::UserCreated { username } => (
                UndoOperation::DeleteUser {
                    username: username.clone(),
                },
                format!("Delete created user: {}", username),
                25,
            ),

            StateChange::UserModified {
                username,
                previous_state,
            } => (
                UndoOperation::RestoreUser {
                    username: username.clone(),
                    backup_data: previous_state.clone(),
                },
                format!("Restore user '{}' to previous state", username),
                25,
            ),

            StateChange::UserDeleted {
                username,
                backup_data,
            } => (
                UndoOperation::RestoreUser {
                    username: username.clone(),
                    backup_data: backup_data.clone(),
                },
                format!("Restore deleted user: {}", username),
                25,
            ),

            StateChange::Custom {
                description,
                undo_command,
                ..
            } => {
                let op = if let Some(cmd) = undo_command {
                    UndoOperation::ExecuteCommand {
                        command: cmd.clone(),
                        args: Vec::new(),
                    }
                } else {
                    UndoOperation::NoOp {
                        reason: "No undo command specified".to_string(),
                    }
                };
                (op, format!("Undo: {}", description), 0)
            }
        };

        Self {
            operation,
            description,
            priority,
            critical: false,
            original_change: Some(change.clone()),
        }
    }
}

/// A plan for rolling back changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackPlan {
    /// Context ID this plan is for
    pub context_id: String,

    /// Actions to execute (in order)
    pub actions: Vec<RollbackAction>,

    /// Whether the plan is complete
    pub complete: bool,

    /// Timestamp when plan was created
    pub created_at: u64,
}

impl RollbackPlan {
    /// Create a new rollback plan
    pub fn new(context_id: impl Into<String>) -> Self {
        Self {
            context_id: context_id.into(),
            actions: Vec::new(),
            complete: false,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Add an action to the plan
    pub fn add_action(&mut self, action: RollbackAction) {
        self.actions.push(action);
    }

    /// Sort actions by priority (highest first)
    pub fn sort_by_priority(&mut self) {
        self.actions.sort_by(|a, b| b.priority.cmp(&a.priority));
    }
}

/// Context for tracking rollback state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackContext {
    /// Unique identifier
    pub id: String,

    /// Description of what this context covers
    pub description: Option<String>,

    /// Current state
    pub state: RollbackState,

    /// Recorded state changes
    pub changes: Vec<StateChange>,

    /// Initial state snapshot
    pub initial_snapshot: Option<StateSnapshot>,

    /// Timestamp when context was created
    pub created_at: u64,
}

impl RollbackContext {
    /// Create a new rollback context
    pub fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let random: u32 = rand::random();

        Self {
            id: format!("rb-{}-{:08x}", timestamp, random),
            description: None,
            state: RollbackState::Active,
            changes: Vec::new(),
            initial_snapshot: None,
            created_at: timestamp,
        }
    }

    /// Add a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set initial snapshot
    pub fn with_snapshot(mut self, snapshot: StateSnapshot) -> Self {
        self.initial_snapshot = Some(snapshot);
        self
    }
}

impl Default for RollbackContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Manager for rollback operations
pub struct RollbackManager {
    contexts: HashMap<String, RollbackContext>,
    plans: HashMap<String, RollbackPlan>,
    /// Optional module registry for executing rollback actions via modules
    module_registry: Option<Arc<ModuleRegistry>>,
}

impl RollbackManager {
    /// Create a new rollback manager
    pub fn new() -> Self {
        Self {
            contexts: HashMap::new(),
            plans: HashMap::new(),
            module_registry: None,
        }
    }

    /// Set the module registry for executing rollback actions via modules
    pub fn with_module_registry(mut self, registry: Arc<ModuleRegistry>) -> Self {
        self.module_registry = Some(registry);
        self
    }

    /// Set the module registry (mutable reference version)
    pub fn set_module_registry(&mut self, registry: Arc<ModuleRegistry>) {
        self.module_registry = Some(registry);
    }

    /// Begin a new rollback context
    pub fn begin_context(&mut self) -> RollbackContext {
        let context = RollbackContext::new();
        let id = context.id.clone();
        self.contexts.insert(id, context.clone());
        debug!("Created rollback context: {}", context.id);
        context
    }

    /// Begin a context with a snapshot
    pub fn begin_context_with_snapshot(&mut self, snapshot: StateSnapshot) -> RollbackContext {
        let context = RollbackContext::new().with_snapshot(snapshot);
        let id = context.id.clone();
        self.contexts.insert(id, context.clone());
        debug!("Created rollback context with snapshot: {}", context.id);
        context
    }

    /// Record a state change
    pub fn record_change(
        &mut self,
        context_id: &str,
        change: StateChange,
    ) -> Result<(), RollbackError> {
        let context = self
            .contexts
            .get_mut(context_id)
            .ok_or_else(|| RollbackError::ContextNotFound(context_id.to_string()))?;

        if context.state != RollbackState::Active {
            return Err(RollbackError::CannotRollback(format!(
                "Context {} is not active (state: {:?})",
                context_id, context.state
            )));
        }

        debug!(
            "Recording change in context {}: {}",
            context_id,
            change.description()
        );
        context.changes.push(change);

        Ok(())
    }

    /// Create a rollback plan for a context
    pub fn create_rollback_plan(
        &mut self,
        context_id: &str,
    ) -> Result<RollbackPlan, RollbackError> {
        let context = self
            .contexts
            .get(context_id)
            .ok_or_else(|| RollbackError::ContextNotFound(context_id.to_string()))?;

        let mut plan = RollbackPlan::new(context_id);

        // Create rollback actions for each change (in reverse order)
        for change in context.changes.iter().rev() {
            let action = RollbackAction::from_state_change(change);
            plan.add_action(action);
        }

        plan.sort_by_priority();

        info!(
            "Created rollback plan for context {} with {} actions",
            context_id,
            plan.actions.len()
        );

        self.plans.insert(context_id.to_string(), plan.clone());
        Ok(plan)
    }

    /// Execute a rollback action
    pub async fn execute_rollback_action(
        &self,
        action: &RollbackAction,
    ) -> Result<(), RollbackError> {
        debug!("Executing rollback action: {}", action.description);

        match &action.operation {
            UndoOperation::DeleteFile { path } => {
                if path.exists() {
                    std::fs::remove_file(path)?;
                }
            }

            UndoOperation::RestoreFile { path, backup_path } => {
                if backup_path.exists() {
                    std::fs::copy(backup_path, path)?;
                } else {
                    warn!(
                        "Backup file not found: {}, cannot restore {}",
                        backup_path.display(),
                        path.display()
                    );
                }
            }

            UndoOperation::DeleteDirectory { path, recursive } => {
                if path.exists() {
                    if *recursive {
                        std::fs::remove_dir_all(path)?;
                    } else {
                        std::fs::remove_dir(path)?;
                    }
                }
            }

            UndoOperation::ChangeServiceState {
                service,
                target_state,
            } => {
                if let Some(ref registry) = self.module_registry {
                    let params: crate::modules::ModuleParams = [
                        (
                            "name".to_string(),
                            serde_json::Value::String(service.clone()),
                        ),
                        (
                            "state".to_string(),
                            serde_json::Value::String(target_state.clone()),
                        ),
                    ]
                    .into_iter()
                    .collect();
                    let ctx = crate::modules::ModuleContext::default();
                    if let Err(e) = registry.execute("service", &params, &ctx) {
                        return Err(RollbackError::RollbackFailed(format!(
                            "Failed to change service {} to {}: {}",
                            service, target_state, e
                        )));
                    }
                } else {
                    warn!(
                        "Service state change not implemented (no module registry): {} -> {}",
                        service, target_state
                    );
                }
            }

            UndoOperation::RemovePackage { name } => {
                if let Some(ref registry) = self.module_registry {
                    let params: crate::modules::ModuleParams = [
                        ("name".to_string(), serde_json::Value::String(name.clone())),
                        (
                            "state".to_string(),
                            serde_json::Value::String("absent".to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect();
                    let ctx = crate::modules::ModuleContext::default();
                    if let Err(e) = registry.execute("package", &params, &ctx) {
                        return Err(RollbackError::RollbackFailed(format!(
                            "Failed to remove package {}: {}",
                            name, e
                        )));
                    }
                } else {
                    warn!(
                        "Package removal not implemented (no module registry): {}",
                        name
                    );
                }
            }

            UndoOperation::InstallPackage { name, version } => {
                if let Some(ref registry) = self.module_registry {
                    let mut params: crate::modules::ModuleParams = [
                        ("name".to_string(), serde_json::Value::String(name.clone())),
                        (
                            "state".to_string(),
                            serde_json::Value::String("present".to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect();
                    if let Some(ver) = version {
                        params.insert(
                            "version".to_string(),
                            serde_json::Value::String(ver.clone()),
                        );
                    }
                    let ctx = crate::modules::ModuleContext::default();
                    if let Err(e) = registry.execute("package", &params, &ctx) {
                        return Err(RollbackError::RollbackFailed(format!(
                            "Failed to install package {}: {}",
                            name, e
                        )));
                    }
                } else {
                    warn!(
                        "Package installation not implemented (no module registry): {}{}",
                        name,
                        version
                            .as_ref()
                            .map(|v| format!("={}", v))
                            .unwrap_or_default()
                    );
                }
            }

            UndoOperation::DeleteUser { username } => {
                if let Some(ref registry) = self.module_registry {
                    let params: crate::modules::ModuleParams = [
                        (
                            "name".to_string(),
                            serde_json::Value::String(username.clone()),
                        ),
                        (
                            "state".to_string(),
                            serde_json::Value::String("absent".to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect();
                    let ctx = crate::modules::ModuleContext::default();
                    if let Err(e) = registry.execute("user", &params, &ctx) {
                        return Err(RollbackError::RollbackFailed(format!(
                            "Failed to delete user {}: {}",
                            username, e
                        )));
                    }
                } else {
                    warn!(
                        "User deletion not implemented (no module registry): {}",
                        username
                    );
                }
            }

            UndoOperation::RestoreUser { username, .. } => {
                if let Some(ref registry) = self.module_registry {
                    let params: crate::modules::ModuleParams = [
                        (
                            "name".to_string(),
                            serde_json::Value::String(username.clone()),
                        ),
                        (
                            "state".to_string(),
                            serde_json::Value::String("present".to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect();
                    let ctx = crate::modules::ModuleContext::default();
                    if let Err(e) = registry.execute("user", &params, &ctx) {
                        return Err(RollbackError::RollbackFailed(format!(
                            "Failed to restore user {}: {}",
                            username, e
                        )));
                    }
                } else {
                    warn!(
                        "User restoration not implemented (no module registry): {}",
                        username
                    );
                }
            }

            UndoOperation::ExecuteCommand { command, args } => {
                let output = std::process::Command::new(command).args(args).output()?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(RollbackError::RollbackFailed(format!(
                        "Command failed: {} ({})",
                        command, stderr
                    )));
                }
            }

            UndoOperation::NoOp { reason } => {
                debug!("No-op rollback action: {}", reason);
            }
        }

        Ok(())
    }

    /// Complete a rollback
    pub fn complete_rollback(&mut self, context_id: &str) -> Result<(), RollbackError> {
        let context = self
            .contexts
            .get_mut(context_id)
            .ok_or_else(|| RollbackError::ContextNotFound(context_id.to_string()))?;

        context.state = RollbackState::RolledBack;
        info!("Rollback completed for context {}", context_id);

        Ok(())
    }

    /// Mark a rollback as failed
    pub fn fail_rollback(&mut self, context_id: &str) -> Result<(), RollbackError> {
        let context = self
            .contexts
            .get_mut(context_id)
            .ok_or_else(|| RollbackError::ContextNotFound(context_id.to_string()))?;

        context.state = RollbackState::Failed;
        error!("Rollback failed for context {}", context_id);

        Ok(())
    }

    /// Commit a context (no rollback needed)
    pub fn commit(&mut self, context_id: &str) -> Result<(), RollbackError> {
        let context = self
            .contexts
            .get_mut(context_id)
            .ok_or_else(|| RollbackError::ContextNotFound(context_id.to_string()))?;

        context.state = RollbackState::Committed;
        info!("Context {} committed", context_id);

        Ok(())
    }

    /// Get a context by ID
    pub fn get_context(&self, context_id: &str) -> Option<&RollbackContext> {
        self.contexts.get(context_id)
    }

    /// Get a rollback plan by context ID
    pub fn get_plan(&self, context_id: &str) -> Option<&RollbackPlan> {
        self.plans.get(context_id)
    }
}

impl Default for RollbackManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_rollback_context_creation() {
        let context = RollbackContext::new();

        assert!(context.id.starts_with("rb-"));
        assert_eq!(context.state, RollbackState::Active);
        assert!(context.changes.is_empty());
    }

    #[test]
    fn test_state_change_description() {
        let change = StateChange::FileCreated {
            path: PathBuf::from("/tmp/test.txt"),
            backup_path: None,
        };

        assert!(change.description().contains("/tmp/test.txt"));
    }

    #[test]
    fn test_rollback_action_from_file_created() {
        let change = StateChange::FileCreated {
            path: PathBuf::from("/tmp/test.txt"),
            backup_path: None,
        };

        let action = RollbackAction::from_state_change(&change);

        match action.operation {
            UndoOperation::DeleteFile { path } => {
                assert_eq!(path, PathBuf::from("/tmp/test.txt"));
            }
            _ => panic!("Expected DeleteFile operation"),
        }
    }

    #[test]
    fn test_rollback_manager() {
        let mut manager = RollbackManager::new();
        let context = manager.begin_context();

        manager
            .record_change(
                &context.id,
                StateChange::FileCreated {
                    path: PathBuf::from("/tmp/test.txt"),
                    backup_path: None,
                },
            )
            .unwrap();

        let plan = manager.create_rollback_plan(&context.id).unwrap();
        assert_eq!(plan.actions.len(), 1);
    }

    #[test]
    fn test_state_snapshot() {
        let mut snapshot = StateSnapshot::new();
        snapshot.store("key1", serde_json::json!({"value": 42}));

        assert!(snapshot.get("key1").is_some());
        assert!(snapshot.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_execute_file_rollback() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        // Create a file
        std::fs::write(&file_path, "test content").unwrap();
        assert!(file_path.exists());

        // Create rollback action
        let action = RollbackAction {
            operation: UndoOperation::DeleteFile {
                path: file_path.clone(),
            },
            description: "Delete test file".to_string(),
            priority: 10,
            critical: false,
            original_change: None,
        };

        // Execute rollback
        let manager = RollbackManager::new();
        manager.execute_rollback_action(&action).await.unwrap();

        assert!(!file_path.exists());
    }
}
