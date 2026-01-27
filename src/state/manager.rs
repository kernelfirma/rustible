//! State Management System
//!
//! This module provides comprehensive state management for configuration management,
//! including state tracking, versioning, and dependency resolution.

use crate::error::{RustibleError, Result};
use crate::state::storage::{StateBackend, StateFile};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use blake3::Hash;
use futures::future::join_all;

/// Represents the lifecycle state of a resource
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceState {
    Absent,
    Present,
    Desired,
    Deleting,
    Failed,
}

/// Represents a single resource in the state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub id: String,
    pub resource_type: String,
    pub state: ResourceState,
    pub config_hash: String,
    pub last_modified: DateTime<Utc>,
    pub checksum: Option<String>,
    pub depends_on: Vec<String>,
    pub tags: Vec<String>,
    pub metadata: HashMap<String, String>,
}

impl Resource {
    pub fn new(id: String, resource_type: String, config_hash: String) -> Self {
        Self {
            id,
            resource_type,
            state: ResourceState::Absent,
            config_hash,
            last_modified: Utc::now(),
            checksum: None,
            depends_on: Vec::new(),
            tags: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn calculate_hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.id.as_bytes());
        hasher.update(self.resource_type.as_bytes());
        hasher.update(self.config_hash.as_bytes());
        hasher.finalize()
    }

    pub fn has_changed(&self, other: &Resource) -> bool {
        self.config_hash != other.config_hash
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, ResourceState::Desired | ResourceState::Failed)
    }
}

/// Represents a state transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub resource_id: String,
    pub from_state: ResourceState,
    pub to_state: ResourceState,
    pub timestamp: DateTime<Utc>,
    pub reason: String,
    pub config_hash: String,
    pub success: bool,
    pub error_message: Option<String>,
}

impl StateTransition {
    pub fn new(
        resource_id: String,
        from_state: ResourceState,
        to_state: ResourceState,
        reason: String,
        config_hash: String,
    ) -> Self {
        Self {
            resource_id,
            from_state,
            to_state,
            timestamp: Utc::now(),
            reason,
            config_hash,
            success: true,
            error_message: None,
        }
    }

    pub fn failed(mut self, error: String) -> Self {
        self.success = false;
        self.error_message = Some(error);
        self
    }
}

/// Configuration for the state manager
#[derive(Debug, Clone)]
pub struct StateManagerConfig {
    pub max_history_size: usize,
    pub enable_versioning: bool,
    pub enable_dependencies: bool,
    pub persistence_interval: u64,
    pub enable_compression: bool,
}

impl Default for StateManagerConfig {
    fn default() -> Self {
        Self {
            max_history_size: 1000,
            enable_versioning: true,
            enable_dependencies: true,
            persistence_interval: 300,
            enable_compression: true,
        }
    }
}

/// State manager handles all state management operations
pub struct StateManager {
    backend: Arc<dyn StateBackend>,
    resources: Arc<RwLock<HashMap<String, Resource>>>,
    history: Arc<RwLock<Vec<StateTransition>>>,
    config: StateManagerConfig,
    version: Arc<RwLock<u64>>,
}

impl StateManager {
    pub fn new(backend: Arc<dyn StateBackend>, config: StateManagerConfig) -> Self {
        Self {
            backend,
            resources: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
            config,
            version: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn initialize(&self) -> Result<()> {
        if let Some(state_file) = self.backend.load_state().await? {
            let mut resources = self.resources.write().await;
            *resources = state_file.resources;

            let mut history = self.history.write().await;
            *history = state_file.history;

            let mut version = self.version.write().await;
            *version = state_file.serial;

            tracing::info!(
                "Loaded state with {} resources, {} transitions, version {}",
                resources.len(),
                history.len(),
                version
            );
        } else {
            tracing::info!("No existing state found, starting fresh");
        }

        Ok(())
    }

    pub async fn upsert_resource(&self, mut resource: Resource) -> Result<()> {
        let mut resources = self.resources.write().await;
        let old_state = resources.get(&resource.id).cloned();
        resource.last_modified = Utc::now();
        resources.insert(resource.id.clone(), resource.clone());

        if let Some(old) = old_state {
            if old.state != resource.state {
                self.record_transition(StateTransition::new(
                    resource.id.clone(),
                    old.state,
                    resource.state,
                    "State updated".to_string(),
                    resource.config_hash.clone(),
                )).await?;
            }
        } else {
            self.record_transition(StateTransition::new(
                resource.id.clone(),
                ResourceState::Absent,
                resource.state,
                "Resource created".to_string(),
                resource.config_hash.clone(),
            )).await?;
        }

        Ok(())
    }

    pub async fn get_resource(&self, id: &str) -> Result<Option<Resource>> {
        let resources = self.resources.read().await;
        Ok(resources.get(id).cloned())
    }

    pub async fn get_resources(&self) -> Result<Vec<Resource>> {
        let resources = self.resources.read().await;
        Ok(resources.values().cloned().collect())
    }

    pub async fn update_resource_state(
        &self,
        id: &str,
        new_state: ResourceState,
        reason: &str,
    ) -> Result<()> {
        let mut resources = self.resources.write().await;

        if let Some(resource) = resources.get_mut(id) {
            let old_state = resource.state;
            resource.state = new_state;
            resource.last_modified = Utc::now();

            self.record_transition(StateTransition::new(
                id.to_string(),
                old_state,
                new_state,
                reason.to_string(),
                resource.config_hash.clone(),
            )).await?;

            Ok(())
        } else {
            Err(RustibleError::State(format!("Resource not found: {}", id)))
        }
    }

    pub async fn record_transition(&self, transition: StateTransition) -> Result<()> {
        let mut history = self.history.write().await;
        history.push(transition);

        if history.len() > self.config.max_history_size {
            let remove_count = history.len() - self.config.max_history_size;
            history.drain(0..remove_count);
        }

        Ok(())
    }

    pub async fn get_history(&self, resource_id: &str) -> Result<Vec<StateTransition>> {
        let history = self.history.read().await;
        Ok(history
            .iter()
            .filter(|t| t.resource_id == resource_id)
            .cloned()
            .collect())
    }

    pub async fn persist_state(&self) -> Result<()> {
        let resources = self.resources.read().await;
        let history = self.history.read().await;
        let mut version = self.version.write().await;

        *version += 1;

        let state_file = StateFile {
            serial: *version,
            resources: resources.clone(),
            history: history.clone(),
            checksum: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        self.backend.save_state(state_file).await?;

        tracing::info!("State persisted, version {}", version);

        Ok(())
    }

    pub async fn get_version(&self) -> Result<u64> {
        let version = self.version.read().await;
        Ok(*version)
    }
}

/// Dependency graph for resource ordering
#[derive(Debug)]
pub struct DependencyGraph {
    graph: HashMap<String, Vec<String>>,
}

impl DependencyGraph {
    pub fn topological_sort(&self) -> Result<Vec<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut result = Vec::new();
        let mut queue = Vec::new();

        for node in self.graph.keys() {
            in_degree.entry(node.clone()).or_insert(0);
        }

        for dependencies in self.graph.values() {
            for dep in dependencies {
                *in_degree.entry(dep.clone()).or_insert(0) += 1;
            }
        }

        for (node, degree) in &in_degree {
            if *degree == 0 {
                queue.push(node.clone());
            }
        }

        while let Some(node) = queue.pop() {
            result.push(node.clone());

            if let Some(dependents) = self.graph.get(&node) {
                for dependent in dependents {
                    if let Some(degree) = in_degree.get_mut(dependent) {
                        *degree -= 1;
                        if *degree == 0 {
                            queue.push(dependent.clone());
                        }
                    }
                }
            }
        }

        if result.len() != self.graph.len() {
            return Err(RustibleError::State(
                "Cycle detected in dependency graph".to_string(),
            ));
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::storage::LocalBackend;

    #[tokio::test]
    async fn test_resource_creation() {
        let backend = Arc::new(LocalBackend::new("/tmp/test_state".to_string()));
        let manager = StateManager::new(backend, StateManagerConfig::default());

        manager.initialize().await.unwrap();

        let mut resource = Resource::new(
            "test-resource".to_string(),
            "file".to_string(),
            "hash123".to_string(),
        );
        resource.state = ResourceState::Present;

        manager.upsert_resource(resource).await.unwrap();

        let retrieved = manager.get_resource("test-resource").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().resource_type, "file");
    }
}
