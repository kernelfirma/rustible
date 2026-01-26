//! Per-host State Manifest Storage and Drift Detection
//!
//! This module provides state manifests that track the desired and actual state
//! of resources on each managed host. It enables drift detection by comparing
//! the stored manifest against the current system state.
//!
//! ## Key Types
//!
//! - [`HostManifest`]: Complete state manifest for a single host
//! - [`ResourceState`]: State of a single managed resource
//! - [`ManifestStore`]: Persistence layer for manifests
//!
//! ## Usage
//!
//! ```ignore
//! use rustible::state::manifest::{HostManifest, ManifestStore, ResourceState};
//!
//! let mut store = ManifestStore::new("./state/manifests");
//!
//! // Record a resource state
//! let mut manifest = store.load_or_create("webserver1")?;
//! manifest.record_resource(ResourceState {
//!     resource_type: "file".to_string(),
//!     resource_id: "/etc/nginx/nginx.conf".to_string(),
//!     desired_state: json!({"owner": "root", "mode": "0644"}),
//!     actual_state: Some(json!({"owner": "root", "mode": "0644"})),
//!     ..Default::default()
//! });
//!
//! store.save(&manifest)?;
//!
//! // Check for drift
//! let drift = manifest.check_drift()?;
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use super::StateError;

/// Complete state manifest for a single host
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostManifest {
    /// Schema version for forward compatibility
    pub schema_version: u32,
    /// Hostname this manifest belongs to
    pub hostname: String,
    /// When this manifest was last updated
    pub last_updated: DateTime<Utc>,
    /// When the last drift check was performed
    pub last_drift_check: Option<DateTime<Utc>>,
    /// Playbook that defined this manifest
    pub source_playbook: Option<String>,
    /// Resources tracked in this manifest
    pub resources: HashMap<String, ResourceState>,
    /// Aggregate drift status
    pub drift_detected: bool,
    /// Number of resources with drift
    pub drift_count: usize,
    /// Metadata/facts about the host
    pub host_facts: HashMap<String, JsonValue>,
}

impl HostManifest {
    /// Create a new empty manifest for a host
    pub fn new(hostname: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            hostname: hostname.into(),
            last_updated: Utc::now(),
            last_drift_check: None,
            source_playbook: None,
            resources: HashMap::new(),
            drift_detected: false,
            drift_count: 0,
            host_facts: HashMap::new(),
        }
    }

    /// Create a manifest with a source playbook
    pub fn with_playbook(hostname: impl Into<String>, playbook: impl Into<String>) -> Self {
        let mut manifest = Self::new(hostname);
        manifest.source_playbook = Some(playbook.into());
        manifest
    }

    /// Generate a unique resource key
    pub fn resource_key(resource_type: &str, resource_id: &str) -> String {
        format!("{}::{}", resource_type, resource_id)
    }

    /// Record or update a resource state
    pub fn record_resource(&mut self, state: ResourceState) {
        let key = Self::resource_key(&state.resource_type, &state.resource_id);
        self.resources.insert(key, state);
        self.last_updated = Utc::now();
    }

    /// Get a resource state
    pub fn get_resource(&self, resource_type: &str, resource_id: &str) -> Option<&ResourceState> {
        let key = Self::resource_key(resource_type, resource_id);
        self.resources.get(&key)
    }

    /// Remove a resource from the manifest
    pub fn remove_resource(&mut self, resource_type: &str, resource_id: &str) -> Option<ResourceState> {
        let key = Self::resource_key(resource_type, resource_id);
        self.resources.remove(&key)
    }

    /// List all resources of a given type
    pub fn resources_by_type(&self, resource_type: &str) -> Vec<&ResourceState> {
        self.resources
            .values()
            .filter(|r| r.resource_type == resource_type)
            .collect()
    }

    /// Update drift status based on resource states
    pub fn update_drift_status(&mut self) {
        let drifted_count = self
            .resources
            .values()
            .filter(|r| r.drift_status == DriftState::Drifted)
            .count();

        self.drift_detected = drifted_count > 0;
        self.drift_count = drifted_count;
        self.last_drift_check = Some(Utc::now());
    }

    /// Get all drifted resources
    pub fn drifted_resources(&self) -> Vec<&ResourceState> {
        self.resources
            .values()
            .filter(|r| r.drift_status == DriftState::Drifted)
            .collect()
    }

    /// Get a summary of all resources by drift status
    pub fn drift_summary(&self) -> DriftSummary {
        let mut summary = DriftSummary::default();

        for resource in self.resources.values() {
            match resource.drift_status {
                DriftState::InSync => summary.in_sync += 1,
                DriftState::Drifted => summary.drifted += 1,
                DriftState::Missing => summary.missing += 1,
                DriftState::Extra => summary.extra += 1,
                DriftState::Unknown => summary.unknown += 1,
            }
        }

        summary.total = self.resources.len();
        summary
    }

    /// Store a host fact
    pub fn set_fact(&mut self, key: impl Into<String>, value: JsonValue) {
        self.host_facts.insert(key.into(), value);
    }

    /// Get a host fact
    pub fn get_fact(&self, key: &str) -> Option<&JsonValue> {
        self.host_facts.get(key)
    }
}

impl Default for HostManifest {
    fn default() -> Self {
        Self::new("unknown")
    }
}

/// State of a single managed resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceState {
    /// Type of resource (file, package, service, user, etc.)
    pub resource_type: String,
    /// Unique identifier for this resource (path, name, etc.)
    pub resource_id: String,
    /// Human-readable name
    pub display_name: Option<String>,
    /// Task name that manages this resource
    pub task_name: Option<String>,
    /// Module used to manage this resource
    pub module: String,
    /// Desired state (from playbook)
    pub desired_state: JsonValue,
    /// Actual state (from last check)
    pub actual_state: Option<JsonValue>,
    /// Current drift status
    pub drift_status: DriftState,
    /// When this resource was first tracked
    pub created_at: DateTime<Utc>,
    /// When this resource was last updated
    pub updated_at: DateTime<Utc>,
    /// When actual state was last gathered
    pub last_checked: Option<DateTime<Utc>>,
    /// Difference between desired and actual state
    pub drift_details: Option<DriftDetails>,
    /// Whether this resource should exist
    pub should_exist: bool,
    /// Tags associated with this resource
    pub tags: Vec<String>,
}

impl ResourceState {
    /// Create a new resource state
    pub fn new(
        resource_type: impl Into<String>,
        resource_id: impl Into<String>,
        module: impl Into<String>,
        desired_state: JsonValue,
    ) -> Self {
        Self {
            resource_type: resource_type.into(),
            resource_id: resource_id.into(),
            display_name: None,
            task_name: None,
            module: module.into(),
            desired_state,
            actual_state: None,
            drift_status: DriftState::Unknown,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_checked: None,
            drift_details: None,
            should_exist: true,
            tags: Vec::new(),
        }
    }

    /// Set the actual state and compute drift
    pub fn set_actual_state(&mut self, actual: JsonValue) {
        self.actual_state = Some(actual);
        self.last_checked = Some(Utc::now());
        self.updated_at = Utc::now();
        self.compute_drift();
    }

    /// Mark resource as missing
    pub fn mark_missing(&mut self) {
        self.actual_state = None;
        self.drift_status = if self.should_exist {
            DriftState::Missing
        } else {
            DriftState::InSync
        };
        self.last_checked = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Compute drift between desired and actual state
    pub fn compute_drift(&mut self) {
        let Some(ref actual) = self.actual_state else {
            self.drift_status = if self.should_exist {
                DriftState::Missing
            } else {
                DriftState::InSync
            };
            return;
        };

        // If resource shouldn't exist but does
        if !self.should_exist {
            self.drift_status = DriftState::Extra;
            self.drift_details = Some(DriftDetails {
                changed_fields: vec![FieldDiff {
                    field: "existence".to_string(),
                    expected: "absent".to_string(),
                    actual: "present".to_string(),
                }],
            });
            return;
        }

        // Compare desired vs actual
        let diff = compare_json(&self.desired_state, actual);
        if diff.is_empty() {
            self.drift_status = DriftState::InSync;
            self.drift_details = None;
        } else {
            self.drift_status = DriftState::Drifted;
            self.drift_details = Some(DriftDetails {
                changed_fields: diff,
            });
        }
    }

    /// Set display name
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Set task name
    pub fn with_task_name(mut self, name: impl Into<String>) -> Self {
        self.task_name = Some(name.into());
        self
    }

    /// Add tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }
}

impl Default for ResourceState {
    fn default() -> Self {
        Self::new("unknown", "unknown", "unknown", JsonValue::Null)
    }
}

/// Drift state of a resource
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriftState {
    /// Resource matches desired state
    InSync,
    /// Resource differs from desired state
    Drifted,
    /// Resource should exist but doesn't
    Missing,
    /// Resource exists but shouldn't
    Extra,
    /// Unable to determine drift status
    Unknown,
}

impl std::fmt::Display for DriftState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriftState::InSync => write!(f, "in-sync"),
            DriftState::Drifted => write!(f, "drifted"),
            DriftState::Missing => write!(f, "missing"),
            DriftState::Extra => write!(f, "extra"),
            DriftState::Unknown => write!(f, "unknown"),
        }
    }
}

/// Detailed drift information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftDetails {
    /// Fields that differ between desired and actual
    pub changed_fields: Vec<FieldDiff>,
}

/// Difference in a single field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDiff {
    /// Field path (e.g., "owner", "mode", "content")
    pub field: String,
    /// Expected value (from desired state)
    pub expected: String,
    /// Actual value (from system)
    pub actual: String,
}

/// Summary of drift across resources
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DriftSummary {
    /// Total resources
    pub total: usize,
    /// Resources in sync
    pub in_sync: usize,
    /// Resources with drift
    pub drifted: usize,
    /// Missing resources
    pub missing: usize,
    /// Extra resources
    pub extra: usize,
    /// Unknown status
    pub unknown: usize,
}

impl DriftSummary {
    /// Check if any drift was detected
    pub fn has_drift(&self) -> bool {
        self.drifted > 0 || self.missing > 0 || self.extra > 0
    }

    /// Get percentage of resources in sync
    pub fn sync_percentage(&self) -> f64 {
        if self.total == 0 {
            100.0
        } else {
            (self.in_sync as f64 / self.total as f64) * 100.0
        }
    }
}

/// Compare two JSON values and return field differences
fn compare_json(expected: &JsonValue, actual: &JsonValue) -> Vec<FieldDiff> {
    let mut diffs = Vec::new();
    compare_json_recursive(expected, actual, "", &mut diffs);
    diffs
}

fn compare_json_recursive(
    expected: &JsonValue,
    actual: &JsonValue,
    path: &str,
    diffs: &mut Vec<FieldDiff>,
) {
    match (expected, actual) {
        (JsonValue::Object(exp_map), JsonValue::Object(act_map)) => {
            // Check for missing/different keys in actual
            for (key, exp_val) in exp_map {
                let field_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };

                match act_map.get(key) {
                    Some(act_val) => {
                        compare_json_recursive(exp_val, act_val, &field_path, diffs);
                    }
                    None => {
                        diffs.push(FieldDiff {
                            field: field_path,
                            expected: format_json_value(exp_val),
                            actual: "<missing>".to_string(),
                        });
                    }
                }
            }

            // Check for extra keys in actual
            for key in act_map.keys() {
                if !exp_map.contains_key(key) {
                    let field_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", path, key)
                    };
                    diffs.push(FieldDiff {
                        field: field_path,
                        expected: "<not expected>".to_string(),
                        actual: format_json_value(act_map.get(key).unwrap()),
                    });
                }
            }
        }
        (JsonValue::Array(exp_arr), JsonValue::Array(act_arr)) => {
            if exp_arr.len() != act_arr.len() {
                diffs.push(FieldDiff {
                    field: format!("{}.length", path),
                    expected: exp_arr.len().to_string(),
                    actual: act_arr.len().to_string(),
                });
            } else {
                for (i, (exp_item, act_item)) in exp_arr.iter().zip(act_arr.iter()).enumerate() {
                    let field_path = format!("{}[{}]", path, i);
                    compare_json_recursive(exp_item, act_item, &field_path, diffs);
                }
            }
        }
        _ => {
            if expected != actual {
                diffs.push(FieldDiff {
                    field: path.to_string(),
                    expected: format_json_value(expected),
                    actual: format_json_value(actual),
                });
            }
        }
    }
}

fn format_json_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        JsonValue::Null => "null".to_string(),
        _ => value.to_string(),
    }
}

/// Persistence layer for host manifests
pub struct ManifestStore {
    /// Base directory for manifest storage
    base_dir: PathBuf,
}

impl ManifestStore {
    /// Create a new manifest store
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    /// Get the path for a host's manifest
    fn manifest_path(&self, hostname: &str) -> PathBuf {
        self.base_dir.join(format!("{}.manifest.json", hostname))
    }

    /// Ensure the base directory exists
    fn ensure_dir(&self) -> io::Result<()> {
        if !self.base_dir.exists() {
            fs::create_dir_all(&self.base_dir)?;
        }
        Ok(())
    }

    /// Load a manifest for a host, creating if it doesn't exist
    pub fn load_or_create(&self, hostname: &str) -> Result<HostManifest, StateError> {
        let path = self.manifest_path(hostname);
        if path.exists() {
            self.load(hostname)
        } else {
            Ok(HostManifest::new(hostname))
        }
    }

    /// Load a manifest for a host
    pub fn load(&self, hostname: &str) -> Result<HostManifest, StateError> {
        let path = self.manifest_path(hostname);
        let content = fs::read_to_string(&path).map_err(|e| {
            StateError::Persistence(format!("Failed to read manifest for {}: {}", hostname, e))
        })?;

        serde_json::from_str(&content).map_err(|e| {
            StateError::Persistence(format!("Failed to parse manifest for {}: {}", hostname, e))
        })
    }

    /// Save a manifest
    pub fn save(&self, manifest: &HostManifest) -> Result<(), StateError> {
        self.ensure_dir().map_err(|e| {
            StateError::Persistence(format!("Failed to create manifest directory: {}", e))
        })?;

        let path = self.manifest_path(&manifest.hostname);
        let content = serde_json::to_string_pretty(manifest)?;
        fs::write(&path, content).map_err(|e| {
            StateError::Persistence(format!(
                "Failed to write manifest for {}: {}",
                manifest.hostname, e
            ))
        })?;

        Ok(())
    }

    /// Delete a manifest
    pub fn delete(&self, hostname: &str) -> Result<(), StateError> {
        let path = self.manifest_path(hostname);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| {
                StateError::Persistence(format!(
                    "Failed to delete manifest for {}: {}",
                    hostname, e
                ))
            })?;
        }
        Ok(())
    }

    /// List all stored manifests
    pub fn list(&self) -> Result<Vec<String>, StateError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let mut hosts = Vec::new();
        let entries = fs::read_dir(&self.base_dir).map_err(|e| {
            StateError::Persistence(format!("Failed to read manifest directory: {}", e))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                StateError::Persistence(format!("Failed to read directory entry: {}", e))
            })?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Some(filename) = path.file_stem() {
                    let name = filename.to_string_lossy();
                    if let Some(hostname) = name.strip_suffix(".manifest") {
                        hosts.push(hostname.to_string());
                    }
                }
            }
        }

        Ok(hosts)
    }

    /// Load all manifests
    pub fn load_all(&self) -> Result<Vec<HostManifest>, StateError> {
        let hosts = self.list()?;
        hosts.into_iter().map(|h| self.load(&h)).collect()
    }

    /// Get aggregate drift summary across all hosts
    pub fn aggregate_drift_summary(&self) -> Result<DriftSummary, StateError> {
        let manifests = self.load_all()?;
        let mut summary = DriftSummary::default();

        for manifest in manifests {
            let host_summary = manifest.drift_summary();
            summary.total += host_summary.total;
            summary.in_sync += host_summary.in_sync;
            summary.drifted += host_summary.drifted;
            summary.missing += host_summary.missing;
            summary.extra += host_summary.extra;
            summary.unknown += host_summary.unknown;
        }

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn test_host_manifest_new() {
        let manifest = HostManifest::new("testhost");
        assert_eq!(manifest.hostname, "testhost");
        assert!(manifest.resources.is_empty());
        assert!(!manifest.drift_detected);
    }

    #[test]
    fn test_resource_state_in_sync() {
        let mut resource = ResourceState::new("file", "/etc/test.conf", "file", json!({
            "owner": "root",
            "mode": "0644"
        }));

        resource.set_actual_state(json!({
            "owner": "root",
            "mode": "0644"
        }));

        assert_eq!(resource.drift_status, DriftState::InSync);
        assert!(resource.drift_details.is_none());
    }

    #[test]
    fn test_resource_state_drifted() {
        let mut resource = ResourceState::new("file", "/etc/test.conf", "file", json!({
            "owner": "root",
            "mode": "0644"
        }));

        resource.set_actual_state(json!({
            "owner": "nobody",
            "mode": "0644"
        }));

        assert_eq!(resource.drift_status, DriftState::Drifted);
        assert!(resource.drift_details.is_some());
        let details = resource.drift_details.unwrap();
        assert_eq!(details.changed_fields.len(), 1);
        assert_eq!(details.changed_fields[0].field, "owner");
    }

    #[test]
    fn test_resource_state_missing() {
        let mut resource = ResourceState::new("file", "/etc/test.conf", "file", json!({
            "state": "present"
        }));

        resource.mark_missing();
        assert_eq!(resource.drift_status, DriftState::Missing);
    }

    #[test]
    fn test_manifest_record_resource() {
        let mut manifest = HostManifest::new("testhost");

        let resource = ResourceState::new("file", "/etc/test.conf", "file", json!({}));
        manifest.record_resource(resource);

        assert_eq!(manifest.resources.len(), 1);
        assert!(manifest.get_resource("file", "/etc/test.conf").is_some());
    }

    #[test]
    fn test_manifest_drift_summary() {
        let mut manifest = HostManifest::new("testhost");

        let mut r1 = ResourceState::new("file", "/etc/a.conf", "file", json!({}));
        r1.drift_status = DriftState::InSync;
        manifest.record_resource(r1);

        let mut r2 = ResourceState::new("file", "/etc/b.conf", "file", json!({}));
        r2.drift_status = DriftState::Drifted;
        manifest.record_resource(r2);

        let mut r3 = ResourceState::new("file", "/etc/c.conf", "file", json!({}));
        r3.drift_status = DriftState::Missing;
        manifest.record_resource(r3);

        let summary = manifest.drift_summary();
        assert_eq!(summary.total, 3);
        assert_eq!(summary.in_sync, 1);
        assert_eq!(summary.drifted, 1);
        assert_eq!(summary.missing, 1);
    }

    #[test]
    fn test_manifest_store_save_load() {
        let dir = tempdir().unwrap();
        let store = ManifestStore::new(dir.path());

        let mut manifest = HostManifest::new("testhost");
        manifest.record_resource(ResourceState::new("file", "/etc/test.conf", "file", json!({})));

        store.save(&manifest).unwrap();

        let loaded = store.load("testhost").unwrap();
        assert_eq!(loaded.hostname, "testhost");
        assert_eq!(loaded.resources.len(), 1);
    }

    #[test]
    fn test_manifest_store_list() {
        let dir = tempdir().unwrap();
        let store = ManifestStore::new(dir.path());

        store.save(&HostManifest::new("host1")).unwrap();
        store.save(&HostManifest::new("host2")).unwrap();

        let hosts = store.list().unwrap();
        assert_eq!(hosts.len(), 2);
        assert!(hosts.contains(&"host1".to_string()));
        assert!(hosts.contains(&"host2".to_string()));
    }

    #[test]
    fn test_compare_json_nested() {
        let expected = json!({
            "outer": {
                "inner": "value1"
            }
        });

        let actual = json!({
            "outer": {
                "inner": "value2"
            }
        });

        let diffs = compare_json(&expected, &actual);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].field, "outer.inner");
    }

    #[test]
    fn test_drift_summary_sync_percentage() {
        let summary = DriftSummary {
            total: 10,
            in_sync: 8,
            drifted: 2,
            missing: 0,
            extra: 0,
            unknown: 0,
        };

        assert_eq!(summary.sync_percentage(), 80.0);
    }
}
