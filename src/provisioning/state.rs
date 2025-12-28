//! Provisioning state management
//!
//! This module handles persistent state for provisioned infrastructure resources.
//! It tracks what resources have been created, their current attributes, and
//! dependencies between them.
//!
//! ## Features
//!
//! - **State Persistence**: Save and load state from disk with integrity verification
//! - **State Diffing**: Compare states to identify added, removed, and modified resources
//! - **State Migration**: Upgrade state files from older versions
//! - **Change History**: Track and query historical changes
//! - **Import/Export**: Convert between Rustible and Terraform state formats

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::error::{ProvisioningError, ProvisioningResult};

// ============================================================================
// Resource State
// ============================================================================

/// Unique identifier for a resource in state
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceId {
    /// Resource type (e.g., "aws_vpc")
    pub resource_type: String,
    /// Resource name (e.g., "main")
    pub name: String,
}

impl ResourceId {
    /// Create a new resource ID
    pub fn new(resource_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            resource_type: resource_type.into(),
            name: name.into(),
        }
    }

    /// Get the full address (type.name)
    pub fn address(&self) -> String {
        format!("{}.{}", self.resource_type, self.name)
    }

    /// Parse from address string
    pub fn from_address(address: &str) -> Option<Self> {
        let parts: Vec<&str> = address.splitn(2, '.').collect();
        if parts.len() == 2 {
            Some(Self::new(parts[0], parts[1]))
        } else {
            None
        }
    }
}

impl std::fmt::Display for ResourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.address())
    }
}

/// State of a single provisioned resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceState {
    /// Unique identifier within Rustible
    pub id: ResourceId,

    /// Cloud provider resource ID (e.g., "vpc-12345678")
    pub cloud_id: String,

    /// Resource type (e.g., "aws_vpc")
    pub resource_type: String,

    /// Provider name (e.g., "aws")
    pub provider: String,

    /// The configuration used to create/update the resource
    pub config: Value,

    /// Computed attributes from the cloud (read-only values)
    pub attributes: Value,

    /// Resources this depends on
    pub dependencies: Vec<ResourceId>,

    /// Resources that depend on this one
    pub dependents: Vec<ResourceId>,

    /// When this resource was first created
    pub created_at: DateTime<Utc>,

    /// When this resource was last updated
    pub updated_at: DateTime<Utc>,

    /// Resource-specific metadata
    pub metadata: HashMap<String, String>,

    /// Whether this resource is tainted (needs replacement)
    pub tainted: bool,

    /// Index for count/for_each resources
    pub index: Option<ResourceIndex>,
}

/// Index for resources created with count or for_each
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ResourceIndex {
    /// Numeric index (from count)
    Number(usize),
    /// String key (from for_each)
    Key(String),
}

impl ResourceState {
    /// Create a new resource state
    pub fn new(
        id: ResourceId,
        cloud_id: impl Into<String>,
        provider: impl Into<String>,
        config: Value,
        attributes: Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            resource_type: id.resource_type.clone(),
            id,
            cloud_id: cloud_id.into(),
            provider: provider.into(),
            config,
            attributes,
            dependencies: Vec::new(),
            dependents: Vec::new(),
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
            tainted: false,
            index: None,
        }
    }

    /// Mark the resource as tainted (needs replacement on next apply)
    pub fn taint(&mut self) {
        self.tainted = true;
        self.updated_at = Utc::now();
    }

    /// Remove taint from resource
    pub fn untaint(&mut self) {
        self.tainted = false;
        self.updated_at = Utc::now();
    }

    /// Update resource attributes
    pub fn update_attributes(&mut self, attributes: Value) {
        self.attributes = attributes;
        self.updated_at = Utc::now();
    }

    /// Get a specific attribute value
    pub fn get_attribute(&self, path: &str) -> Option<&Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = &self.attributes;

        for part in parts {
            match current {
                Value::Object(map) => {
                    current = map.get(part)?;
                }
                Value::Array(arr) => {
                    let idx: usize = part.parse().ok()?;
                    current = arr.get(idx)?;
                }
                _ => return None,
            }
        }

        Some(current)
    }
}

// ============================================================================
// State Diff and Change Tracking
// ============================================================================

/// Summary of differences between two states
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiffSummary {
    /// Number of resources added
    pub added_count: usize,
    /// Number of resources removed
    pub removed_count: usize,
    /// Number of resources modified
    pub modified_count: usize,
    /// Number of unchanged resources
    pub unchanged_count: usize,
}

impl DiffSummary {
    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        self.added_count > 0 || self.removed_count > 0 || self.modified_count > 0
    }

    /// Get total resource count
    pub fn total(&self) -> usize {
        self.added_count + self.removed_count + self.modified_count + self.unchanged_count
    }
}

impl std::fmt::Display for DiffSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} added, {} removed, {} modified, {} unchanged",
            self.added_count, self.removed_count, self.modified_count, self.unchanged_count
        )
    }
}

/// Diff between two provisioning states
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisioningStateDiff {
    /// Resources that were added
    pub added: Vec<ResourceId>,
    /// Resources that were removed
    pub removed: Vec<ResourceId>,
    /// Resources that were modified (id, old_attrs, new_attrs)
    pub modified: Vec<(ResourceId, Value, Value)>,
    /// Outputs that changed (name -> (old_value, new_value))
    pub output_changes: HashMap<String, (Option<Value>, Option<Value>)>,
    /// Summary statistics
    pub summary: DiffSummary,
}

impl Default for ProvisioningStateDiff {
    fn default() -> Self {
        Self::new()
    }
}

impl ProvisioningStateDiff {
    /// Create an empty diff
    pub fn new() -> Self {
        Self {
            added: Vec::new(),
            removed: Vec::new(),
            modified: Vec::new(),
            output_changes: HashMap::new(),
            summary: DiffSummary::default(),
        }
    }

    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        self.summary.has_changes() || !self.output_changes.is_empty()
    }

    /// Get a human-readable summary
    pub fn display_summary(&self) -> String {
        let mut parts = Vec::new();

        if !self.added.is_empty() {
            parts.push(format!(
                "  + {} to add: {}",
                self.added.len(),
                self.added
                    .iter()
                    .map(|r| r.address())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if !self.removed.is_empty() {
            parts.push(format!(
                "  - {} to remove: {}",
                self.removed.len(),
                self.removed
                    .iter()
                    .map(|r| r.address())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if !self.modified.is_empty() {
            parts.push(format!(
                "  ~ {} to modify: {}",
                self.modified.len(),
                self.modified
                    .iter()
                    .map(|(r, _, _)| r.address())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if !self.output_changes.is_empty() {
            parts.push(format!(
                "  * {} output changes: {}",
                self.output_changes.len(),
                self.output_changes
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if parts.is_empty() {
            "No changes detected".to_string()
        } else {
            parts.join("\n")
        }
    }
}

impl std::fmt::Display for ProvisioningStateDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "State Diff Summary")?;
        writeln!(f, "==================")?;
        write!(f, "{}", self.display_summary())
    }
}

/// Type of state change
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StateChangeType {
    /// A new resource was created
    ResourceCreated,
    /// An existing resource was updated
    ResourceUpdated,
    /// A resource was deleted
    ResourceDeleted,
    /// An output value changed
    OutputChanged,
    /// A provider was configured
    ProviderConfigured,
    /// State was migrated to a new version
    StateMigrated,
    /// State was imported from another format
    StateImported,
}

impl std::fmt::Display for StateChangeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateChangeType::ResourceCreated => write!(f, "created"),
            StateChangeType::ResourceUpdated => write!(f, "updated"),
            StateChangeType::ResourceDeleted => write!(f, "deleted"),
            StateChangeType::OutputChanged => write!(f, "output_changed"),
            StateChangeType::ProviderConfigured => write!(f, "provider_configured"),
            StateChangeType::StateMigrated => write!(f, "migrated"),
            StateChangeType::StateImported => write!(f, "imported"),
        }
    }
}

/// Individual state change record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateChange {
    /// Serial number when this change occurred
    pub serial: u64,
    /// Timestamp of the change
    pub timestamp: DateTime<Utc>,
    /// Type of change
    pub change_type: StateChangeType,
    /// Resource affected (if applicable)
    pub resource_id: Option<ResourceId>,
    /// Human-readable description
    pub description: String,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, Value>,
}

impl StateChange {
    /// Create a new state change record
    pub fn new(
        serial: u64,
        change_type: StateChangeType,
        resource_id: Option<ResourceId>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            serial,
            timestamp: Utc::now(),
            change_type,
            resource_id,
            description: description.into(),
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the change
    pub fn with_metadata(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }
}

impl std::fmt::Display for StateChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let resource_str = self
            .resource_id
            .as_ref()
            .map(|r| r.address())
            .unwrap_or_else(|| "N/A".to_string());
        write!(
            f,
            "[{}] Serial {} - {} {}: {}",
            self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            self.serial,
            self.change_type,
            resource_str,
            self.description
        )
    }
}

// ============================================================================
// State Migration
// ============================================================================

/// Trait for state migrations between versions
pub trait StateMigration: Send + Sync {
    /// Source version this migration upgrades from
    fn from_version(&self) -> u32;

    /// Target version this migration upgrades to
    fn to_version(&self) -> u32;

    /// Perform the migration
    fn migrate(&self, state: &mut ProvisioningState) -> ProvisioningResult<()>;

    /// Get a description of what this migration does
    fn description(&self) -> &str;
}

/// Migration from version 1 to version 2
/// Example migration that adds history tracking
pub struct MigrationV1ToV2;

impl StateMigration for MigrationV1ToV2 {
    fn from_version(&self) -> u32 {
        1
    }

    fn to_version(&self) -> u32 {
        2
    }

    fn migrate(&self, state: &mut ProvisioningState) -> ProvisioningResult<()> {
        // Initialize history if not present
        if state.history.is_empty() {
            state.record_change(StateChange::new(
                state.serial,
                StateChangeType::StateMigrated,
                None,
                "Migrated from v1 to v2: added history tracking",
            ));
        }
        state.version = 2;
        Ok(())
    }

    fn description(&self) -> &str {
        "Add history tracking support"
    }
}

/// Registry of available migrations
pub struct MigrationRegistry {
    migrations: Vec<Box<dyn StateMigration>>,
}

impl Default for MigrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationRegistry {
    /// Create a new migration registry with built-in migrations
    pub fn new() -> Self {
        let mut registry = Self {
            migrations: Vec::new(),
        };
        // Register built-in migrations
        registry.register(Box::new(MigrationV1ToV2));
        registry
    }

    /// Register a migration
    pub fn register(&mut self, migration: Box<dyn StateMigration>) {
        self.migrations.push(migration);
        // Sort by from_version to ensure proper order
        self.migrations.sort_by_key(|m| m.from_version());
    }

    /// Get migrations needed to go from one version to another
    pub fn get_path(&self, from_version: u32, to_version: u32) -> Vec<&dyn StateMigration> {
        let mut path = Vec::new();
        let mut current = from_version;

        while current < to_version {
            if let Some(migration) = self.migrations.iter().find(|m| m.from_version() == current) {
                path.push(migration.as_ref());
                current = migration.to_version();
            } else {
                break;
            }
        }

        path
    }
}

// ============================================================================
// Provisioning State (Full State File)
// ============================================================================

/// Output value from provisioning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputValue {
    /// The output value
    pub value: Value,
    /// Description of the output
    pub description: Option<String>,
    /// Whether this is a sensitive value
    pub sensitive: bool,
}

/// Complete provisioning state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisioningState {
    /// State format version
    pub version: u32,

    /// Serial number (incremented on each save)
    pub serial: u64,

    /// Unique lineage identifier (prevents mixing states)
    pub lineage: String,

    /// All provisioned resources
    pub resources: HashMap<String, ResourceState>,

    /// Output values
    pub outputs: HashMap<String, OutputValue>,

    /// Provider configurations used
    pub providers: HashMap<String, Value>,

    /// When the state was last modified
    pub last_modified: DateTime<Utc>,

    /// Checksum for integrity verification
    pub checksum: Option<String>,

    /// Change history
    #[serde(default)]
    pub history: Vec<StateChange>,
}

impl Default for ProvisioningState {
    fn default() -> Self {
        Self::new()
    }
}

impl ProvisioningState {
    /// Current state format version
    pub const VERSION: u32 = 2;

    /// Maximum history entries to keep by default
    pub const DEFAULT_HISTORY_LIMIT: usize = 100;

    /// Create a new empty state
    pub fn new() -> Self {
        Self {
            version: Self::VERSION,
            serial: 0,
            lineage: Uuid::new_v4().to_string(),
            resources: HashMap::new(),
            outputs: HashMap::new(),
            providers: HashMap::new(),
            last_modified: Utc::now(),
            checksum: None,
            history: Vec::new(),
        }
    }

    /// Load state from a file
    pub async fn load(path: impl AsRef<Path>) -> ProvisioningResult<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(Self::new());
        }

        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!("Failed to read state file: {}", e))
        })?;

        let state: Self = serde_json::from_str(&content)?;

        // Verify checksum if present
        if let Some(ref stored_checksum) = state.checksum {
            let computed = state.compute_checksum();
            if stored_checksum != &computed {
                return Err(ProvisioningError::StateCorruption(
                    "State file checksum mismatch".to_string(),
                ));
            }
        }

        // Validate version
        if state.version > Self::VERSION {
            return Err(ProvisioningError::StateCorruption(format!(
                "State file version {} is newer than supported version {}",
                state.version,
                Self::VERSION
            )));
        }

        Ok(state)
    }

    /// Save state to a file
    pub async fn save(&mut self, path: impl AsRef<Path>) -> ProvisioningResult<()> {
        let path = path.as_ref();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ProvisioningError::StatePersistenceError(format!(
                    "Failed to create state directory: {}",
                    e
                ))
            })?;
        }

        // Update metadata
        self.serial += 1;
        self.last_modified = Utc::now();
        self.checksum = Some(self.compute_checksum());

        // Serialize to pretty JSON
        let content = serde_json::to_string_pretty(self)?;

        // Write atomically using temp file
        let temp_path = path.with_extension("tmp");
        tokio::fs::write(&temp_path, &content).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!("Failed to write state file: {}", e))
        })?;

        tokio::fs::rename(&temp_path, path).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to finalize state file: {}",
                e
            ))
        })?;

        Ok(())
    }

    /// Compute checksum of state content (excluding the checksum field itself)
    fn compute_checksum(&self) -> String {
        use sha2::{Digest, Sha256};

        let mut state_for_hash = self.clone();
        state_for_hash.checksum = None;

        let content = serde_json::to_string(&state_for_hash).unwrap_or_default();
        let hash = Sha256::digest(content.as_bytes());
        format!("{:x}", hash)
    }

    /// Add a resource to state
    pub fn add_resource(&mut self, state: ResourceState) {
        self.resources.insert(state.id.address(), state);
    }

    /// Remove a resource from state
    pub fn remove_resource(&mut self, id: &ResourceId) -> Option<ResourceState> {
        self.resources.remove(&id.address())
    }

    /// Get a resource by ID
    pub fn get_resource(&self, id: &ResourceId) -> Option<&ResourceState> {
        self.resources.get(&id.address())
    }

    /// Get a mutable reference to a resource
    pub fn get_resource_mut(&mut self, id: &ResourceId) -> Option<&mut ResourceState> {
        self.resources.get_mut(&id.address())
    }

    /// Get a resource by address string
    pub fn get_by_address(&self, address: &str) -> Option<&ResourceState> {
        self.resources.get(address)
    }

    /// Check if a resource exists
    pub fn has_resource(&self, id: &ResourceId) -> bool {
        self.resources.contains_key(&id.address())
    }

    /// Get all resources of a specific type
    pub fn resources_by_type(&self, resource_type: &str) -> Vec<&ResourceState> {
        self.resources
            .values()
            .filter(|r| r.resource_type == resource_type)
            .collect()
    }

    /// Get all resources from a specific provider
    pub fn resources_by_provider(&self, provider: &str) -> Vec<&ResourceState> {
        self.resources
            .values()
            .filter(|r| r.provider == provider)
            .collect()
    }

    /// Get all tainted resources
    pub fn tainted_resources(&self) -> Vec<&ResourceState> {
        self.resources.values().filter(|r| r.tainted).collect()
    }

    /// Set an output value
    pub fn set_output(&mut self, name: impl Into<String>, value: OutputValue) {
        self.outputs.insert(name.into(), value);
    }

    /// Get an output value
    pub fn get_output(&self, name: &str) -> Option<&OutputValue> {
        self.outputs.get(name)
    }

    /// Get the count of resources
    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    /// Check if the state is empty
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }

    /// Create a backup of the current state
    pub async fn backup(&self, backup_dir: impl AsRef<Path>) -> ProvisioningResult<PathBuf> {
        let backup_dir = backup_dir.as_ref();
        tokio::fs::create_dir_all(backup_dir).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!(
                "Failed to create backup directory: {}",
                e
            ))
        })?;

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let backup_path = backup_dir.join(format!("state_{}.json.backup", timestamp));

        let content = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&backup_path, content).await.map_err(|e| {
            ProvisioningError::StatePersistenceError(format!("Failed to write backup: {}", e))
        })?;

        Ok(backup_path)
    }

    /// Get a summary of the state
    pub fn summary(&self) -> StateSummary {
        let mut by_provider: HashMap<String, usize> = HashMap::new();
        let mut by_type: HashMap<String, usize> = HashMap::new();

        for resource in self.resources.values() {
            *by_provider.entry(resource.provider.clone()).or_insert(0) += 1;
            *by_type.entry(resource.resource_type.clone()).or_insert(0) += 1;
        }

        StateSummary {
            total_resources: self.resources.len(),
            tainted_resources: self.tainted_resources().len(),
            outputs_count: self.outputs.len(),
            by_provider,
            by_type,
            serial: self.serial,
            last_modified: self.last_modified,
        }
    }

    // ========================================================================
    // State Diff Methods
    // ========================================================================

    /// Compare this state with another state to find differences
    pub fn diff(&self, other: &ProvisioningState) -> ProvisioningStateDiff {
        let mut diff = ProvisioningStateDiff::new();

        // Get all resource addresses from both states
        let self_keys: HashSet<&String> = self.resources.keys().collect();
        let other_keys: HashSet<&String> = other.resources.keys().collect();

        // Find added resources (in other but not in self)
        for key in other_keys.difference(&self_keys) {
            if let Some(id) = ResourceId::from_address(key) {
                diff.added.push(id);
            }
        }

        // Find removed resources (in self but not in other)
        for key in self_keys.difference(&other_keys) {
            if let Some(id) = ResourceId::from_address(key) {
                diff.removed.push(id);
            }
        }

        // Find modified resources (in both but with different attributes)
        let mut unchanged_count = 0;
        for key in self_keys.intersection(&other_keys) {
            let self_resource = self.resources.get(*key).unwrap();
            let other_resource = other.resources.get(*key).unwrap();

            if self_resource.attributes != other_resource.attributes
                || self_resource.config != other_resource.config
            {
                if let Some(id) = ResourceId::from_address(key) {
                    diff.modified.push((
                        id,
                        self_resource.attributes.clone(),
                        other_resource.attributes.clone(),
                    ));
                }
            } else {
                unchanged_count += 1;
            }
        }

        // Compare outputs
        let self_output_keys: HashSet<&String> = self.outputs.keys().collect();
        let other_output_keys: HashSet<&String> = other.outputs.keys().collect();

        // Added outputs
        for key in other_output_keys.difference(&self_output_keys) {
            let new_value = other.outputs.get(*key).map(|o| o.value.clone());
            diff.output_changes
                .insert((*key).clone(), (None, new_value));
        }

        // Removed outputs
        for key in self_output_keys.difference(&other_output_keys) {
            let old_value = self.outputs.get(*key).map(|o| o.value.clone());
            diff.output_changes
                .insert((*key).clone(), (old_value, None));
        }

        // Modified outputs
        for key in self_output_keys.intersection(&other_output_keys) {
            let self_output = self.outputs.get(*key).unwrap();
            let other_output = other.outputs.get(*key).unwrap();

            if self_output.value != other_output.value {
                diff.output_changes.insert(
                    (*key).clone(),
                    (
                        Some(self_output.value.clone()),
                        Some(other_output.value.clone()),
                    ),
                );
            }
        }

        // Update summary
        diff.summary = DiffSummary {
            added_count: diff.added.len(),
            removed_count: diff.removed.len(),
            modified_count: diff.modified.len(),
            unchanged_count,
        };

        diff
    }

    /// Get changes since a specific serial number
    pub fn changes_since(&self, serial: u64) -> ProvisioningResult<Vec<StateChange>> {
        Ok(self
            .history
            .iter()
            .filter(|change| change.serial > serial)
            .cloned()
            .collect())
    }

    /// Get changes within a time range
    pub fn changes_in_range(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Vec<&StateChange> {
        self.history
            .iter()
            .filter(|change| change.timestamp >= start && change.timestamp <= end)
            .collect()
    }

    // ========================================================================
    // History Tracking Methods
    // ========================================================================

    /// Record a change in history
    pub fn record_change(&mut self, change: StateChange) {
        self.history.push(change);
    }

    /// Get the change history
    pub fn history(&self) -> &[StateChange] {
        &self.history
    }

    /// Compact history to keep only the most recent entries
    pub fn compact_history(&mut self, keep_count: usize) {
        if self.history.len() > keep_count {
            let start = self.history.len() - keep_count;
            self.history = self.history[start..].to_vec();
        }
    }

    /// Clear all history
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Get the most recent change
    pub fn last_change(&self) -> Option<&StateChange> {
        self.history.last()
    }

    /// Record a resource creation
    pub fn record_resource_created(&mut self, id: &ResourceId, description: impl Into<String>) {
        self.record_change(StateChange::new(
            self.serial,
            StateChangeType::ResourceCreated,
            Some(id.clone()),
            description,
        ));
    }

    /// Record a resource update
    pub fn record_resource_updated(&mut self, id: &ResourceId, description: impl Into<String>) {
        self.record_change(StateChange::new(
            self.serial,
            StateChangeType::ResourceUpdated,
            Some(id.clone()),
            description,
        ));
    }

    /// Record a resource deletion
    pub fn record_resource_deleted(&mut self, id: &ResourceId, description: impl Into<String>) {
        self.record_change(StateChange::new(
            self.serial,
            StateChangeType::ResourceDeleted,
            Some(id.clone()),
            description,
        ));
    }

    // ========================================================================
    // State Migration Methods
    // ========================================================================

    /// Check if migration is needed to reach the current version
    pub fn needs_migration(&self) -> bool {
        self.version < Self::VERSION
    }

    /// Migrate state to the current version
    pub fn migrate_to_current(&mut self) -> ProvisioningResult<()> {
        let registry = MigrationRegistry::new();
        self.migrate_with_registry(&registry)
    }

    /// Migrate state using a specific migration registry
    pub fn migrate_with_registry(
        &mut self,
        registry: &MigrationRegistry,
    ) -> ProvisioningResult<()> {
        if !self.needs_migration() {
            return Ok(());
        }

        let migrations = registry.get_path(self.version, Self::VERSION);

        if migrations.is_empty() && self.version < Self::VERSION {
            return Err(ProvisioningError::StateCorruption(format!(
                "No migration path from version {} to {}",
                self.version,
                Self::VERSION
            )));
        }

        for migration in migrations {
            migration.migrate(self)?;
        }

        Ok(())
    }

    // ========================================================================
    // Import/Export Methods
    // ========================================================================

    /// Export to Terraform-compatible JSON state format
    pub fn export_terraform_format(&self) -> ProvisioningResult<Value> {
        let mut tf_resources = Vec::new();

        for resource in self.resources.values() {
            let tf_resource = serde_json::json!({
                "mode": "managed",
                "type": resource.resource_type,
                "name": resource.id.name,
                "provider": format!("provider[\"{}\"]", resource.provider),
                "instances": [{
                    "schema_version": 0,
                    "attributes": resource.attributes,
                    "private": null,
                    "dependencies": resource.dependencies.iter()
                        .map(|d| d.address())
                        .collect::<Vec<_>>()
                }]
            });
            tf_resources.push(tf_resource);
        }

        let mut tf_outputs = serde_json::Map::new();
        for (name, output) in &self.outputs {
            tf_outputs.insert(
                name.clone(),
                serde_json::json!({
                    "value": output.value,
                    "type": Self::infer_terraform_type(&output.value),
                    "sensitive": output.sensitive
                }),
            );
        }

        let tf_state = serde_json::json!({
            "version": 4,
            "terraform_version": "1.0.0",
            "serial": self.serial,
            "lineage": self.lineage,
            "outputs": tf_outputs,
            "resources": tf_resources
        });

        Ok(tf_state)
    }

    /// Infer Terraform type from a JSON value
    fn infer_terraform_type(value: &Value) -> Value {
        match value {
            Value::String(_) => serde_json::json!("string"),
            Value::Number(_) => serde_json::json!("number"),
            Value::Bool(_) => serde_json::json!("bool"),
            Value::Array(arr) => {
                if arr.is_empty() {
                    serde_json::json!(["tuple", []])
                } else {
                    let inner_type = Self::infer_terraform_type(&arr[0]);
                    serde_json::json!(["list", inner_type])
                }
            }
            Value::Object(_) => serde_json::json!(["object", {}]),
            Value::Null => serde_json::json!("string"),
        }
    }

    /// Import from Terraform state JSON
    pub fn import_from_terraform(tf_state: &Value) -> ProvisioningResult<Self> {
        let mut state = Self::new();

        // Extract lineage if present
        if let Some(lineage) = tf_state.get("lineage").and_then(|v| v.as_str()) {
            state.lineage = lineage.to_string();
        }

        // Extract serial
        if let Some(serial) = tf_state.get("serial").and_then(|v| v.as_u64()) {
            state.serial = serial;
        }

        // Import resources
        if let Some(resources) = tf_state.get("resources").and_then(|v| v.as_array()) {
            for tf_resource in resources {
                let resource_type = tf_resource
                    .get("type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ProvisioningError::ImportError {
                        resource_type: "unknown".to_string(),
                        resource_id: "unknown".to_string(),
                        message: "Missing resource type".to_string(),
                    })?;

                let name = tf_resource
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ProvisioningError::ImportError {
                        resource_type: resource_type.to_string(),
                        resource_id: "unknown".to_string(),
                        message: "Missing resource name".to_string(),
                    })?;

                // Extract provider from provider string like "provider[\"aws\"]"
                let provider = tf_resource
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .map(|p| {
                        p.trim_start_matches("provider[\"")
                            .trim_end_matches("\"]")
                            .to_string()
                    })
                    .unwrap_or_else(|| {
                        // Try to infer from resource type
                        resource_type
                            .split('_')
                            .next()
                            .unwrap_or("unknown")
                            .to_string()
                    });

                // Get instances (Terraform supports multiple instances via count/for_each)
                if let Some(instances) = tf_resource.get("instances").and_then(|v| v.as_array()) {
                    for (idx, instance) in instances.iter().enumerate() {
                        let attributes = instance
                            .get("attributes")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));

                        // Extract cloud ID from attributes if available
                        let cloud_id = attributes
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("imported-{}-{}", resource_type, idx));

                        // Extract dependencies
                        let dependencies: Vec<ResourceId> = instance
                            .get("dependencies")
                            .and_then(|v| v.as_array())
                            .map(|deps| {
                                deps.iter()
                                    .filter_map(|d| d.as_str())
                                    .filter_map(ResourceId::from_address)
                                    .collect()
                            })
                            .unwrap_or_default();

                        let id = ResourceId::new(resource_type, name);
                        let mut resource_state = ResourceState::new(
                            id.clone(),
                            cloud_id,
                            &provider,
                            serde_json::json!({}),
                            attributes,
                        );
                        resource_state.dependencies = dependencies;

                        // Set index for count/for_each
                        if let Some(index_key) = instance.get("index_key") {
                            if let Some(n) = index_key.as_u64() {
                                resource_state.index = Some(ResourceIndex::Number(n as usize));
                            } else if let Some(s) = index_key.as_str() {
                                resource_state.index = Some(ResourceIndex::Key(s.to_string()));
                            }
                        }

                        state.add_resource(resource_state);
                    }
                }
            }
        }

        // Import outputs
        if let Some(outputs) = tf_state.get("outputs").and_then(|v| v.as_object()) {
            for (name, output) in outputs {
                let value = output.get("value").cloned().unwrap_or(Value::Null);
                let sensitive = output
                    .get("sensitive")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                state.set_output(
                    name.clone(),
                    OutputValue {
                        value,
                        description: None,
                        sensitive,
                    },
                );
            }
        }

        // Record the import in history
        state.record_change(StateChange::new(
            state.serial,
            StateChangeType::StateImported,
            None,
            format!(
                "Imported {} resources from Terraform state",
                state.resource_count()
            ),
        ));

        Ok(state)
    }

    /// Export to HCL format (Terraform configuration-style)
    pub fn export_hcl(&self) -> ProvisioningResult<String> {
        let mut hcl = String::new();

        // Header comment
        hcl.push_str("# Generated by Rustible\n");
        hcl.push_str(&format!(
            "# Exported at: {}\n",
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ));
        hcl.push_str(&format!("# Serial: {}\n\n", self.serial));

        // Provider blocks
        for (provider_name, config) in &self.providers {
            hcl.push_str(&format!("provider \"{}\" {{\n", provider_name));
            if let Value::Object(map) = config {
                for (key, value) in map {
                    hcl.push_str(&format!("  {} = {}\n", key, Self::value_to_hcl(value)));
                }
            }
            hcl.push_str("}\n\n");
        }

        // Resource blocks
        for resource in self.resources.values() {
            hcl.push_str(&format!(
                "resource \"{}\" \"{}\" {{\n",
                resource.resource_type, resource.id.name
            ));

            // Use config if available, otherwise use attributes
            let config_to_use = if resource.config != serde_json::json!({}) {
                &resource.config
            } else {
                &resource.attributes
            };

            if let Value::Object(map) = config_to_use {
                for (key, value) in map {
                    // Skip computed-only attributes
                    if ![
                        "id",
                        "arn",
                        "owner_id",
                        "default_network_acl_id",
                        "default_route_table_id",
                        "default_security_group_id",
                        "main_route_table_id",
                        "ipv6_association_id",
                    ]
                    .contains(&key.as_str())
                    {
                        hcl.push_str(&format!("  {} = {}\n", key, Self::value_to_hcl(value)));
                    }
                }
            }

            // Add dependencies if present
            if !resource.dependencies.is_empty() {
                hcl.push_str("\n  depends_on = [\n");
                for dep in &resource.dependencies {
                    hcl.push_str(&format!("    {},\n", dep.address()));
                }
                hcl.push_str("  ]\n");
            }

            hcl.push_str("}\n\n");
        }

        // Output blocks
        for (name, output) in &self.outputs {
            hcl.push_str(&format!("output \"{}\" {{\n", name));
            hcl.push_str(&format!(
                "  value = {}\n",
                Self::value_to_hcl(&output.value)
            ));
            if output.sensitive {
                hcl.push_str("  sensitive = true\n");
            }
            if let Some(ref desc) = output.description {
                hcl.push_str(&format!(
                    "  description = \"{}\"\n",
                    desc.replace('"', "\\\"")
                ));
            }
            hcl.push_str("}\n\n");
        }

        Ok(hcl)
    }

    /// Convert a JSON value to HCL syntax
    fn value_to_hcl(value: &Value) -> String {
        match value {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
            Value::Array(arr) => {
                let items: Vec<String> = arr.iter().map(Self::value_to_hcl).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Object(map) => {
                let items: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{} = {}", k, Self::value_to_hcl(v)))
                    .collect();
                format!("{{\n    {}\n  }}", items.join("\n    "))
            }
        }
    }

    /// Import from HCL string (simplified parsing)
    /// Note: This is a basic implementation for simple HCL structures
    pub fn import_from_hcl(hcl: &str) -> ProvisioningResult<Self> {
        let mut state = Self::new();
        let mut current_block_type: Option<String> = None;
        let mut current_resource_type: Option<String> = None;
        let mut current_name: Option<String> = None;
        let mut current_attributes: serde_json::Map<String, Value> = serde_json::Map::new();
        let mut brace_depth = 0;

        for line in hcl.lines() {
            let trimmed = line.trim();

            // Skip comments and empty lines
            if trimmed.starts_with('#') || trimmed.starts_with("//") || trimmed.is_empty() {
                continue;
            }

            // Detect block start
            if trimmed.starts_with("resource ") && trimmed.contains('{') {
                let parts: Vec<&str> = trimmed
                    .trim_start_matches("resource ")
                    .trim_end_matches('{')
                    .trim()
                    .split('"')
                    .filter(|s| !s.trim().is_empty())
                    .collect();

                if parts.len() >= 2 {
                    current_block_type = Some("resource".to_string());
                    current_resource_type = Some(parts[0].to_string());
                    current_name = Some(parts[1].to_string());
                    brace_depth = 1;
                }
            } else if trimmed.starts_with("output ") && trimmed.contains('{') {
                let parts: Vec<&str> = trimmed
                    .trim_start_matches("output ")
                    .trim_end_matches('{')
                    .trim()
                    .split('"')
                    .filter(|s| !s.trim().is_empty())
                    .collect();

                if !parts.is_empty() {
                    current_block_type = Some("output".to_string());
                    current_name = Some(parts[0].to_string());
                    brace_depth = 1;
                }
            } else if trimmed == "}" {
                brace_depth -= 1;
                if brace_depth == 0 {
                    // End of block - save it
                    match current_block_type.as_deref() {
                        Some("resource") => {
                            if let (Some(ref rt), Some(ref name)) =
                                (&current_resource_type, &current_name)
                            {
                                let provider =
                                    rt.split('_').next().unwrap_or("unknown").to_string();
                                let id = ResourceId::new(rt.clone(), name.clone());
                                let attrs = Value::Object(current_attributes.clone());
                                let cloud_id = current_attributes
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| format!("hcl-{}-{}", rt, name));

                                let resource = ResourceState::new(
                                    id,
                                    cloud_id,
                                    provider,
                                    attrs.clone(),
                                    attrs,
                                );
                                state.add_resource(resource);
                            }
                        }
                        Some("output") => {
                            if let Some(ref name) = current_name {
                                let value = current_attributes
                                    .get("value")
                                    .cloned()
                                    .unwrap_or(Value::Null);
                                let sensitive = current_attributes
                                    .get("sensitive")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let description = current_attributes
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());

                                state.set_output(
                                    name.clone(),
                                    OutputValue {
                                        value,
                                        description,
                                        sensitive,
                                    },
                                );
                            }
                        }
                        _ => {}
                    }

                    // Reset state
                    current_block_type = None;
                    current_resource_type = None;
                    current_name = None;
                    current_attributes = serde_json::Map::new();
                }
            } else if brace_depth > 0 && trimmed.contains('=') {
                // Parse attribute
                let parts: Vec<&str> = trimmed.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim().to_string();
                    let value_str = parts[1].trim();
                    let value = Self::parse_hcl_value(value_str);
                    current_attributes.insert(key, value);
                }
            }
        }

        state.record_change(StateChange::new(
            state.serial,
            StateChangeType::StateImported,
            None,
            format!("Imported {} resources from HCL", state.resource_count()),
        ));

        Ok(state)
    }

    /// Parse a simple HCL value string into a JSON Value
    fn parse_hcl_value(s: &str) -> Value {
        let trimmed = s.trim();

        // Boolean
        if trimmed == "true" {
            return Value::Bool(true);
        }
        if trimmed == "false" {
            return Value::Bool(false);
        }

        // Null
        if trimmed == "null" {
            return Value::Null;
        }

        // Number
        if let Ok(n) = trimmed.parse::<i64>() {
            return Value::Number(n.into());
        }
        if let Ok(n) = trimmed.parse::<f64>() {
            if let Some(num) = serde_json::Number::from_f64(n) {
                return Value::Number(num);
            }
        }

        // String (with quotes)
        if trimmed.starts_with('"') && trimmed.ends_with('"') {
            return Value::String(
                trimmed[1..trimmed.len() - 1]
                    .replace("\\\"", "\"")
                    .replace("\\\\", "\\"),
            );
        }

        // Array
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];
            let items: Vec<Value> = inner
                .split(',')
                .map(|item| Self::parse_hcl_value(item.trim()))
                .collect();
            return Value::Array(items);
        }

        // Default to string without quotes
        Value::String(trimmed.to_string())
    }
}

/// Summary of provisioning state
#[derive(Debug, Clone)]
pub struct StateSummary {
    /// Total number of resources
    pub total_resources: usize,
    /// Number of tainted resources
    pub tainted_resources: usize,
    /// Number of outputs
    pub outputs_count: usize,
    /// Resources by provider
    pub by_provider: HashMap<String, usize>,
    /// Resources by type
    pub by_type: HashMap<String, usize>,
    /// State serial number
    pub serial: u64,
    /// Last modification time
    pub last_modified: DateTime<Utc>,
}

impl std::fmt::Display for StateSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Provisioning State Summary")?;
        writeln!(f, "==========================")?;
        writeln!(f, "Total resources: {}", self.total_resources)?;
        if self.tainted_resources > 0 {
            writeln!(f, "Tainted resources: {}", self.tainted_resources)?;
        }
        writeln!(f, "Outputs: {}", self.outputs_count)?;
        writeln!(f, "Serial: {}", self.serial)?;
        writeln!(
            f,
            "Last modified: {}",
            self.last_modified.format("%Y-%m-%d %H:%M:%S UTC")
        )?;

        if !self.by_provider.is_empty() {
            writeln!(f, "\nBy Provider:")?;
            for (provider, count) in &self.by_provider {
                writeln!(f, "  {}: {}", provider, count)?;
            }
        }

        if !self.by_type.is_empty() {
            writeln!(f, "\nBy Type:")?;
            for (resource_type, count) in &self.by_type {
                writeln!(f, "  {}: {}", resource_type, count)?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_id() {
        let id = ResourceId::new("aws_vpc", "main");
        assert_eq!(id.address(), "aws_vpc.main");
        assert_eq!(id.to_string(), "aws_vpc.main");

        let parsed = ResourceId::from_address("aws_vpc.main").unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn test_resource_state_creation() {
        let id = ResourceId::new("aws_vpc", "main");
        let config = serde_json::json!({"cidr_block": "10.0.0.0/16"});
        let attrs = serde_json::json!({"id": "vpc-123", "cidr_block": "10.0.0.0/16"});

        let state = ResourceState::new(id.clone(), "vpc-123", "aws", config, attrs);

        assert_eq!(state.id, id);
        assert_eq!(state.cloud_id, "vpc-123");
        assert!(!state.tainted);
    }

    #[test]
    fn test_resource_state_taint() {
        let id = ResourceId::new("aws_vpc", "main");
        let mut state = ResourceState::new(
            id,
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({}),
        );

        assert!(!state.tainted);
        state.taint();
        assert!(state.tainted);
        state.untaint();
        assert!(!state.tainted);
    }

    #[test]
    fn test_provisioning_state() {
        let mut state = ProvisioningState::new();
        assert!(state.is_empty());
        assert_eq!(state.version, ProvisioningState::VERSION);

        let id = ResourceId::new("aws_vpc", "main");
        let resource = ResourceState::new(
            id.clone(),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({}),
        );

        state.add_resource(resource);
        assert!(!state.is_empty());
        assert_eq!(state.resource_count(), 1);
        assert!(state.has_resource(&id));

        let retrieved = state.get_resource(&id).unwrap();
        assert_eq!(retrieved.cloud_id, "vpc-123");
    }

    #[test]
    fn test_state_summary() {
        let mut state = ProvisioningState::new();

        let vpc = ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({}),
        );

        let subnet = ResourceState::new(
            ResourceId::new("aws_subnet", "public"),
            "subnet-456",
            "aws",
            serde_json::json!({}),
            serde_json::json!({}),
        );

        state.add_resource(vpc);
        state.add_resource(subnet);

        let summary = state.summary();
        assert_eq!(summary.total_resources, 2);
        assert_eq!(summary.by_provider.get("aws"), Some(&2));
        assert_eq!(summary.by_type.get("aws_vpc"), Some(&1));
        assert_eq!(summary.by_type.get("aws_subnet"), Some(&1));
    }

    #[test]
    fn test_get_attribute() {
        let id = ResourceId::new("aws_vpc", "main");
        let attrs = serde_json::json!({
            "id": "vpc-123",
            "tags": {
                "Name": "production"
            },
            "subnets": ["subnet-1", "subnet-2"]
        });

        let state = ResourceState::new(id, "vpc-123", "aws", serde_json::json!({}), attrs);

        assert_eq!(
            state.get_attribute("id"),
            Some(&serde_json::json!("vpc-123"))
        );
        assert_eq!(
            state.get_attribute("tags.Name"),
            Some(&serde_json::json!("production"))
        );
        assert_eq!(
            state.get_attribute("subnets.0"),
            Some(&serde_json::json!("subnet-1"))
        );
        assert_eq!(state.get_attribute("nonexistent"), None);
    }

    // ========================================================================
    // State Diff Tests
    // ========================================================================

    #[test]
    fn test_diff_empty_states() {
        let state1 = ProvisioningState::new();
        let state2 = ProvisioningState::new();

        let diff = state1.diff(&state2);
        assert!(!diff.has_changes());
        assert_eq!(diff.summary.added_count, 0);
        assert_eq!(diff.summary.removed_count, 0);
        assert_eq!(diff.summary.modified_count, 0);
    }

    #[test]
    fn test_diff_added_resources() {
        let state1 = ProvisioningState::new();
        let mut state2 = ProvisioningState::new();

        let vpc = ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({"id": "vpc-123"}),
        );
        state2.add_resource(vpc);

        let diff = state1.diff(&state2);
        assert!(diff.has_changes());
        assert_eq!(diff.summary.added_count, 1);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.added[0].address(), "aws_vpc.main");
    }

    #[test]
    fn test_diff_removed_resources() {
        let mut state1 = ProvisioningState::new();
        let state2 = ProvisioningState::new();

        let vpc = ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({"id": "vpc-123"}),
        );
        state1.add_resource(vpc);

        let diff = state1.diff(&state2);
        assert!(diff.has_changes());
        assert_eq!(diff.summary.removed_count, 1);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].address(), "aws_vpc.main");
    }

    #[test]
    fn test_diff_modified_resources() {
        let mut state1 = ProvisioningState::new();
        let mut state2 = ProvisioningState::new();

        let vpc1 = ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({"id": "vpc-123", "cidr_block": "10.0.0.0/16"}),
        );
        state1.add_resource(vpc1);

        let vpc2 = ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({"id": "vpc-123", "cidr_block": "10.1.0.0/16"}),
        );
        state2.add_resource(vpc2);

        let diff = state1.diff(&state2);
        assert!(diff.has_changes());
        assert_eq!(diff.summary.modified_count, 1);
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.modified[0].0.address(), "aws_vpc.main");
    }

    #[test]
    fn test_diff_output_changes() {
        let mut state1 = ProvisioningState::new();
        let mut state2 = ProvisioningState::new();

        state1.set_output(
            "vpc_id",
            OutputValue {
                value: serde_json::json!("vpc-123"),
                description: None,
                sensitive: false,
            },
        );

        state2.set_output(
            "vpc_id",
            OutputValue {
                value: serde_json::json!("vpc-456"),
                description: None,
                sensitive: false,
            },
        );

        let diff = state1.diff(&state2);
        assert!(diff.has_changes());
        assert!(diff.output_changes.contains_key("vpc_id"));
    }

    #[test]
    fn test_diff_summary_display() {
        let diff = ProvisioningStateDiff {
            added: vec![ResourceId::new("aws_vpc", "new")],
            removed: vec![ResourceId::new("aws_vpc", "old")],
            modified: vec![(
                ResourceId::new("aws_vpc", "main"),
                serde_json::json!({}),
                serde_json::json!({}),
            )],
            output_changes: HashMap::new(),
            summary: DiffSummary {
                added_count: 1,
                removed_count: 1,
                modified_count: 1,
                unchanged_count: 2,
            },
        };

        let summary = diff.display_summary();
        assert!(summary.contains("to add"));
        assert!(summary.contains("to remove"));
        assert!(summary.contains("to modify"));
    }

    // ========================================================================
    // History Tracking Tests
    // ========================================================================

    #[test]
    fn test_record_change() {
        let mut state = ProvisioningState::new();
        let id = ResourceId::new("aws_vpc", "main");

        state.record_resource_created(&id, "Created VPC");

        assert_eq!(state.history().len(), 1);
        let change = state.last_change().unwrap();
        assert_eq!(change.change_type, StateChangeType::ResourceCreated);
        assert!(change.resource_id.is_some());
    }

    #[test]
    fn test_compact_history() {
        let mut state = ProvisioningState::new();
        let id = ResourceId::new("aws_vpc", "main");

        // Add 10 changes
        for i in 0..10 {
            state.record_resource_updated(&id, format!("Update {}", i));
        }

        assert_eq!(state.history().len(), 10);

        // Compact to keep only 5
        state.compact_history(5);
        assert_eq!(state.history().len(), 5);

        // Verify we kept the most recent ones
        let last = state.last_change().unwrap();
        assert!(last.description.contains("Update 9"));
    }

    #[test]
    fn test_changes_since() {
        let mut state = ProvisioningState::new();
        let id = ResourceId::new("aws_vpc", "main");

        state.serial = 5;
        state.record_resource_created(&id, "First");

        state.serial = 10;
        state.record_resource_updated(&id, "Second");

        state.serial = 15;
        state.record_resource_updated(&id, "Third");

        let changes = state.changes_since(5).unwrap();
        assert_eq!(changes.len(), 2);
    }

    // ========================================================================
    // Migration Tests
    // ========================================================================

    #[test]
    fn test_needs_migration() {
        let mut state = ProvisioningState::new();
        state.version = 1;

        assert!(state.needs_migration());

        state.version = ProvisioningState::VERSION;
        assert!(!state.needs_migration());
    }

    #[test]
    fn test_migration_registry() {
        let registry = MigrationRegistry::new();
        let path = registry.get_path(1, 2);

        assert_eq!(path.len(), 1);
        assert_eq!(path[0].from_version(), 1);
        assert_eq!(path[0].to_version(), 2);
    }

    #[test]
    fn test_migrate_v1_to_v2() {
        let mut state = ProvisioningState::new();
        state.version = 1;
        state.history.clear();

        let migration = MigrationV1ToV2;
        migration.migrate(&mut state).unwrap();

        assert_eq!(state.version, 2);
        assert!(!state.history.is_empty());
    }

    // ========================================================================
    // Import/Export Tests
    // ========================================================================

    #[test]
    fn test_export_terraform_format() {
        let mut state = ProvisioningState::new();
        state.serial = 42;

        let vpc = ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({"id": "vpc-123", "cidr_block": "10.0.0.0/16"}),
        );
        state.add_resource(vpc);

        state.set_output(
            "vpc_id",
            OutputValue {
                value: serde_json::json!("vpc-123"),
                description: Some("The VPC ID".to_string()),
                sensitive: false,
            },
        );

        let tf_state = state.export_terraform_format().unwrap();

        assert_eq!(tf_state["version"], 4);
        assert_eq!(tf_state["serial"], 42);
        assert!(tf_state["resources"].is_array());
        assert!(tf_state["outputs"].is_object());
    }

    #[test]
    fn test_import_from_terraform() {
        let tf_state = serde_json::json!({
            "version": 4,
            "terraform_version": "1.0.0",
            "serial": 100,
            "lineage": "test-lineage-123",
            "resources": [
                {
                    "mode": "managed",
                    "type": "aws_vpc",
                    "name": "main",
                    "provider": "provider[\"aws\"]",
                    "instances": [
                        {
                            "schema_version": 0,
                            "attributes": {
                                "id": "vpc-123",
                                "cidr_block": "10.0.0.0/16"
                            }
                        }
                    ]
                }
            ],
            "outputs": {
                "vpc_id": {
                    "value": "vpc-123",
                    "type": "string",
                    "sensitive": false
                }
            }
        });

        let state = ProvisioningState::import_from_terraform(&tf_state).unwrap();

        assert_eq!(state.lineage, "test-lineage-123");
        assert_eq!(state.serial, 100);
        assert_eq!(state.resource_count(), 1);
        assert!(state.has_resource(&ResourceId::new("aws_vpc", "main")));
        assert!(state.get_output("vpc_id").is_some());
    }

    #[test]
    fn test_export_hcl() {
        let mut state = ProvisioningState::new();

        let vpc = ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({"cidr_block": "10.0.0.0/16", "enable_dns_support": true}),
            serde_json::json!({}),
        );
        state.add_resource(vpc);

        state.set_output(
            "vpc_id",
            OutputValue {
                value: serde_json::json!("vpc-123"),
                description: Some("The VPC ID".to_string()),
                sensitive: true,
            },
        );

        let hcl = state.export_hcl().unwrap();

        assert!(hcl.contains("resource \"aws_vpc\" \"main\""));
        assert!(hcl.contains("cidr_block = \"10.0.0.0/16\""));
        assert!(hcl.contains("output \"vpc_id\""));
        assert!(hcl.contains("sensitive = true"));
    }

    #[test]
    fn test_import_from_hcl() {
        let hcl = r#"
# Test HCL file
resource "aws_vpc" "main" {
  cidr_block = "10.0.0.0/16"
  enable_dns_support = true
}

output "vpc_id" {
  value = "vpc-123"
  sensitive = true
}
"#;

        let state = ProvisioningState::import_from_hcl(hcl).unwrap();

        assert_eq!(state.resource_count(), 1);
        assert!(state.has_resource(&ResourceId::new("aws_vpc", "main")));

        let output = state.get_output("vpc_id").unwrap();
        assert_eq!(output.value, serde_json::json!("vpc-123"));
        assert!(output.sensitive);
    }

    #[test]
    fn test_roundtrip_terraform_format() {
        let mut original = ProvisioningState::new();

        let vpc = ResourceState::new(
            ResourceId::new("aws_vpc", "main"),
            "vpc-123",
            "aws",
            serde_json::json!({}),
            serde_json::json!({"id": "vpc-123", "cidr_block": "10.0.0.0/16"}),
        );
        original.add_resource(vpc);

        original.set_output(
            "vpc_id",
            OutputValue {
                value: serde_json::json!("vpc-123"),
                description: None,
                sensitive: false,
            },
        );

        // Export and reimport
        let tf_state = original.export_terraform_format().unwrap();
        let imported = ProvisioningState::import_from_terraform(&tf_state).unwrap();

        assert_eq!(original.resource_count(), imported.resource_count());
        assert_eq!(original.outputs.len(), imported.outputs.len());
    }

    #[test]
    fn test_hcl_value_parsing() {
        // Test various HCL value types
        assert_eq!(
            ProvisioningState::parse_hcl_value("true"),
            Value::Bool(true)
        );
        assert_eq!(
            ProvisioningState::parse_hcl_value("false"),
            Value::Bool(false)
        );
        assert_eq!(ProvisioningState::parse_hcl_value("null"), Value::Null);
        assert_eq!(
            ProvisioningState::parse_hcl_value("42"),
            Value::Number(42.into())
        );
        assert_eq!(
            ProvisioningState::parse_hcl_value("\"hello\""),
            Value::String("hello".to_string())
        );
        assert_eq!(
            ProvisioningState::parse_hcl_value("[1, 2, 3]"),
            Value::Array(vec![
                Value::Number(1.into()),
                Value::Number(2.into()),
                Value::Number(3.into())
            ])
        );
    }

    #[test]
    fn test_state_change_display() {
        let id = ResourceId::new("aws_vpc", "main");
        let change = StateChange::new(
            1,
            StateChangeType::ResourceCreated,
            Some(id),
            "Created new VPC",
        );

        let display = format!("{}", change);
        assert!(display.contains("Serial 1"));
        assert!(display.contains("created"));
        assert!(display.contains("aws_vpc.main"));
        assert!(display.contains("Created new VPC"));
    }

    #[test]
    fn test_diff_summary_has_changes() {
        let empty = DiffSummary::default();
        assert!(!empty.has_changes());

        let with_added = DiffSummary {
            added_count: 1,
            ..Default::default()
        };
        assert!(with_added.has_changes());

        let with_removed = DiffSummary {
            removed_count: 1,
            ..Default::default()
        };
        assert!(with_removed.has_changes());

        let with_modified = DiffSummary {
            modified_count: 1,
            ..Default::default()
        };
        assert!(with_modified.has_changes());
    }
}
