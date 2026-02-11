//! Workspace management for infrastructure provisioning
//!
//! This module provides workspace functionality similar to Terraform workspaces,
//! allowing users to maintain separate state files for different environments
//! (e.g., dev, staging, production) within a single configuration.
//!
//! ## Features
//!
//! - **Workspace Isolation**: Each workspace has its own state file
//! - **Default Workspace**: A "default" workspace is always available
//! - **State Path Resolution**: Workspace-specific state file paths
//! - **Workspace Lifecycle**: Create, select, list, and delete workspaces
//!
//! ## Usage
//!
//! ```rust,no_run
//! # use rustible::provisioning::workspace::WorkspaceManager;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut manager = WorkspaceManager::new(".rustible").await?;
//!
//! // Create and switch to a new workspace
//! manager.create("staging").await?;
//! manager.select("staging").await?;
//!
//! // Get workspace-specific state path
//! let state_path = manager.state_path();
//! // -> ".rustible/workspaces/staging/provisioning.state.json"
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing;

use super::error::{ProvisioningError, ProvisioningResult};

// ============================================================================
// Constants
// ============================================================================

/// Default workspace name
pub const DEFAULT_WORKSPACE: &str = "default";

/// Directory under root for workspace storage
const WORKSPACES_DIR: &str = "workspaces";

/// State file name within each workspace directory
const STATE_FILE: &str = "provisioning.state.json";

/// File tracking the currently selected workspace
const CURRENT_WORKSPACE_FILE: &str = "workspace";

// ============================================================================
// Workspace Info
// ============================================================================

/// Information about a workspace
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceInfo {
    /// Workspace name
    pub name: String,

    /// Whether this is the currently selected workspace
    pub is_current: bool,

    /// Path to the workspace's state file
    pub state_path: PathBuf,

    /// Whether a state file exists for this workspace
    pub has_state: bool,
}

// ============================================================================
// Workspace Manager
// ============================================================================

/// Manager for workspace operations
///
/// Workspaces allow maintaining multiple independent state files
/// under a single configuration directory. Each workspace stores its
/// state in `.rustible/workspaces/{name}/provisioning.state.json`.
pub struct WorkspaceManager {
    /// Root directory (e.g., `.rustible`)
    root_dir: PathBuf,

    /// Currently selected workspace name
    current: String,
}

impl WorkspaceManager {
    /// Create a new workspace manager
    ///
    /// Initializes the workspace directory structure and loads the
    /// currently selected workspace from disk. If no workspace file exists,
    /// the default workspace is selected.
    pub async fn new(root_dir: impl Into<PathBuf>) -> ProvisioningResult<Self> {
        let root_dir = root_dir.into();

        // Ensure the workspaces directory exists
        let workspaces_dir = root_dir.join(WORKSPACES_DIR);
        tokio::fs::create_dir_all(&workspaces_dir).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to create workspaces directory: {}",
                e
            ))
        })?;

        // Ensure the default workspace directory exists
        let default_dir = workspaces_dir.join(DEFAULT_WORKSPACE);
        tokio::fs::create_dir_all(&default_dir).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to create default workspace directory: {}",
                e
            ))
        })?;

        // Load current workspace from file
        let current = Self::load_current_workspace(&root_dir).await;

        tracing::debug!(workspace = %current, "Workspace manager initialized");

        Ok(Self { root_dir, current })
    }

    /// Get the currently selected workspace name
    pub fn current(&self) -> &str {
        &self.current
    }

    /// Get the state file path for the current workspace
    pub fn state_path(&self) -> PathBuf {
        self.workspace_state_path(&self.current)
    }

    /// Get the state file path for a specific workspace
    pub fn workspace_state_path(&self, name: &str) -> PathBuf {
        self.root_dir
            .join(WORKSPACES_DIR)
            .join(name)
            .join(STATE_FILE)
    }

    /// Get the workspace directory path for a specific workspace
    fn workspace_dir(&self, name: &str) -> PathBuf {
        self.root_dir.join(WORKSPACES_DIR).join(name)
    }

    /// Create a new workspace
    ///
    /// Creates the workspace directory. Does not switch to it.
    /// Returns an error if the workspace already exists.
    pub async fn create(&self, name: &str) -> ProvisioningResult<()> {
        Self::validate_workspace_name(name)?;

        let workspace_dir = self.workspace_dir(name);

        if workspace_dir.exists() {
            return Err(ProvisioningError::ValidationError(format!(
                "Workspace '{}' already exists",
                name
            )));
        }

        tokio::fs::create_dir_all(&workspace_dir).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to create workspace directory: {}",
                e
            ))
        })?;

        tracing::info!(workspace = %name, "Created workspace");

        Ok(())
    }

    /// Select (switch to) a workspace
    ///
    /// The workspace must already exist.
    pub async fn select(&mut self, name: &str) -> ProvisioningResult<()> {
        let workspace_dir = self.workspace_dir(name);

        if !workspace_dir.exists() {
            return Err(ProvisioningError::ValidationError(format!(
                "Workspace '{}' does not exist",
                name
            )));
        }

        self.current = name.to_string();
        self.save_current_workspace().await?;

        tracing::info!(workspace = %name, "Switched to workspace");

        Ok(())
    }

    /// Delete a workspace
    ///
    /// Cannot delete the default workspace or the currently selected workspace.
    pub async fn delete(&self, name: &str) -> ProvisioningResult<()> {
        if name == DEFAULT_WORKSPACE {
            return Err(ProvisioningError::ValidationError(
                "Cannot delete the default workspace".to_string(),
            ));
        }

        if name == self.current {
            return Err(ProvisioningError::ValidationError(format!(
                "Cannot delete the currently selected workspace '{}'. Switch to a different workspace first.",
                name
            )));
        }

        let workspace_dir = self.workspace_dir(name);

        if !workspace_dir.exists() {
            return Err(ProvisioningError::ValidationError(format!(
                "Workspace '{}' does not exist",
                name
            )));
        }

        tokio::fs::remove_dir_all(&workspace_dir).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to delete workspace directory: {}",
                e
            ))
        })?;

        tracing::info!(workspace = %name, "Deleted workspace");

        Ok(())
    }

    /// List all available workspaces
    pub async fn list(&self) -> ProvisioningResult<Vec<WorkspaceInfo>> {
        let workspaces_dir = self.root_dir.join(WORKSPACES_DIR);
        let mut workspaces = Vec::new();

        let mut entries = tokio::fs::read_dir(&workspaces_dir).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to read workspaces directory: {}",
                e
            ))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to read workspace entry: {}",
                e
            ))
        })? {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    let state_path = self.workspace_state_path(name);
                    workspaces.push(WorkspaceInfo {
                        name: name.to_string(),
                        is_current: name == self.current,
                        has_state: state_path.exists(),
                        state_path,
                    });
                }
            }
        }

        // Sort by name for consistent output
        workspaces.sort_by(|a, b| a.name.cmp(&b.name));

        Ok(workspaces)
    }

    /// Check if a workspace exists
    pub fn exists(&self, name: &str) -> bool {
        self.workspace_dir(name).exists()
    }

    /// Validate a workspace name
    fn validate_workspace_name(name: &str) -> ProvisioningResult<()> {
        if name.is_empty() {
            return Err(ProvisioningError::ValidationError(
                "Workspace name cannot be empty".to_string(),
            ));
        }

        if name.len() > 64 {
            return Err(ProvisioningError::ValidationError(
                "Workspace name cannot exceed 64 characters".to_string(),
            ));
        }

        // Only allow alphanumeric, hyphens, and underscores
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ProvisioningError::ValidationError(format!(
                "Workspace name '{}' contains invalid characters. Only alphanumeric, hyphens, and underscores are allowed.",
                name
            )));
        }

        Ok(())
    }

    /// Load the current workspace name from the workspace file
    async fn load_current_workspace(root_dir: &Path) -> String {
        let workspace_file = root_dir.join(CURRENT_WORKSPACE_FILE);

        match tokio::fs::read_to_string(&workspace_file).await {
            Ok(content) => {
                let name = content.trim().to_string();
                if name.is_empty() {
                    DEFAULT_WORKSPACE.to_string()
                } else {
                    name
                }
            }
            Err(_) => DEFAULT_WORKSPACE.to_string(),
        }
    }

    /// Save the current workspace name to the workspace file
    async fn save_current_workspace(&self) -> ProvisioningResult<()> {
        let workspace_file = self.root_dir.join(CURRENT_WORKSPACE_FILE);

        tokio::fs::write(&workspace_file, &self.current)
            .await
            .map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to save current workspace: {}",
                    e
                ))
            })?;

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_manager(dir: &TempDir) -> WorkspaceManager {
        let root = dir.path().join(".rustible");
        WorkspaceManager::new(&root).await.unwrap()
    }

    #[tokio::test]
    async fn test_workspace_manager_init() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        assert_eq!(manager.current(), DEFAULT_WORKSPACE);

        // Default workspace directory should exist
        let default_dir = dir
            .path()
            .join(".rustible/workspaces/default");
        assert!(default_dir.exists());
    }

    #[tokio::test]
    async fn test_workspace_state_path() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        let state_path = manager.state_path();
        let expected = dir
            .path()
            .join(".rustible/workspaces/default/provisioning.state.json");
        assert_eq!(state_path, expected);
    }

    #[tokio::test]
    async fn test_workspace_create() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        manager.create("staging").await.unwrap();

        let staging_dir = dir.path().join(".rustible/workspaces/staging");
        assert!(staging_dir.exists());
    }

    #[tokio::test]
    async fn test_workspace_create_duplicate() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        manager.create("staging").await.unwrap();

        // Creating duplicate should fail
        let result = manager.create("staging").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workspace_create_invalid_name() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        // Empty name
        assert!(manager.create("").await.is_err());

        // Invalid characters
        assert!(manager.create("my workspace").await.is_err());
        assert!(manager.create("my/workspace").await.is_err());
        assert!(manager.create("../evil").await.is_err());

        // Valid names
        assert!(manager.create("staging").await.is_ok());
        assert!(manager.create("dev-01").await.is_ok());
        assert!(manager.create("prod_us_east").await.is_ok());
    }

    #[tokio::test]
    async fn test_workspace_select() {
        let dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&dir).await;

        manager.create("staging").await.unwrap();
        manager.select("staging").await.unwrap();

        assert_eq!(manager.current(), "staging");

        let expected_state = dir
            .path()
            .join(".rustible/workspaces/staging/provisioning.state.json");
        assert_eq!(manager.state_path(), expected_state);
    }

    #[tokio::test]
    async fn test_workspace_select_nonexistent() {
        let dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&dir).await;

        let result = manager.select("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workspace_select_persists() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join(".rustible");

        {
            let mut manager = WorkspaceManager::new(&root).await.unwrap();
            manager.create("staging").await.unwrap();
            manager.select("staging").await.unwrap();
        }

        // Create new manager, should restore workspace selection
        let manager = WorkspaceManager::new(&root).await.unwrap();
        assert_eq!(manager.current(), "staging");
    }

    #[tokio::test]
    async fn test_workspace_delete() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        manager.create("staging").await.unwrap();
        manager.delete("staging").await.unwrap();

        let staging_dir = dir.path().join(".rustible/workspaces/staging");
        assert!(!staging_dir.exists());
    }

    #[tokio::test]
    async fn test_workspace_delete_default() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        // Cannot delete default workspace
        let result = manager.delete("default").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workspace_delete_current() {
        let dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&dir).await;

        manager.create("staging").await.unwrap();
        manager.select("staging").await.unwrap();

        // Cannot delete currently selected workspace
        let result = manager.delete("staging").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workspace_delete_nonexistent() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        let result = manager.delete("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workspace_list() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        manager.create("staging").await.unwrap();
        manager.create("production").await.unwrap();

        let workspaces = manager.list().await.unwrap();

        assert_eq!(workspaces.len(), 3); // default + staging + production

        let names: Vec<&str> = workspaces.iter().map(|w| w.name.as_str()).collect();
        assert!(names.contains(&"default"));
        assert!(names.contains(&"staging"));
        assert!(names.contains(&"production"));

        // default should be current
        let default_ws = workspaces.iter().find(|w| w.name == "default").unwrap();
        assert!(default_ws.is_current);

        let staging_ws = workspaces.iter().find(|w| w.name == "staging").unwrap();
        assert!(!staging_ws.is_current);
    }

    #[tokio::test]
    async fn test_workspace_exists() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        assert!(manager.exists("default"));
        assert!(!manager.exists("staging"));

        manager.create("staging").await.unwrap();
        assert!(manager.exists("staging"));
    }

    #[tokio::test]
    async fn test_workspace_state_path_specific() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        let path = manager.workspace_state_path("production");
        let expected = dir
            .path()
            .join(".rustible/workspaces/production/provisioning.state.json");
        assert_eq!(path, expected);
    }

    #[test]
    fn test_validate_workspace_name() {
        // Valid names
        assert!(WorkspaceManager::validate_workspace_name("default").is_ok());
        assert!(WorkspaceManager::validate_workspace_name("staging").is_ok());
        assert!(WorkspaceManager::validate_workspace_name("dev-01").is_ok());
        assert!(WorkspaceManager::validate_workspace_name("prod_us").is_ok());
        assert!(WorkspaceManager::validate_workspace_name("Test123").is_ok());

        // Invalid names
        assert!(WorkspaceManager::validate_workspace_name("").is_err());
        assert!(WorkspaceManager::validate_workspace_name("has space").is_err());
        assert!(WorkspaceManager::validate_workspace_name("has/slash").is_err());
        assert!(WorkspaceManager::validate_workspace_name("has.dot").is_err());

        // Too long
        let long_name = "a".repeat(65);
        assert!(WorkspaceManager::validate_workspace_name(&long_name).is_err());
    }

    #[tokio::test]
    async fn test_workspace_list_marks_current() {
        let dir = TempDir::new().unwrap();
        let mut manager = create_test_manager(&dir).await;

        manager.create("staging").await.unwrap();
        manager.select("staging").await.unwrap();

        let workspaces = manager.list().await.unwrap();

        let staging = workspaces.iter().find(|w| w.name == "staging").unwrap();
        assert!(staging.is_current);

        let default = workspaces.iter().find(|w| w.name == "default").unwrap();
        assert!(!default.is_current);
    }

    #[tokio::test]
    async fn test_workspace_info_has_state() {
        let dir = TempDir::new().unwrap();
        let manager = create_test_manager(&dir).await;

        manager.create("staging").await.unwrap();

        // Create a state file in the staging workspace
        let state_path = manager.workspace_state_path("staging");
        tokio::fs::write(&state_path, "{}").await.unwrap();

        let workspaces = manager.list().await.unwrap();
        let staging = workspaces.iter().find(|w| w.name == "staging").unwrap();
        assert!(staging.has_state);

        let default = workspaces.iter().find(|w| w.name == "default").unwrap();
        assert!(!default.has_state);
    }
}
