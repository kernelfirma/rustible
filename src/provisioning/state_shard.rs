//! State sharding for large-scale infrastructure deployments
//!
//! This module provides state sharding capabilities to split large state files
//! into smaller, more manageable pieces. This improves performance for deployments
//! with hundreds or thousands of resources by reducing the amount of state that
//! needs to be read/written for each operation.
//!
//! ## Sharding Strategies
//!
//! - **None**: Single monolithic state file (default)
//! - **ByProvider**: Shard by cloud provider (aws, azure, gcp)
//! - **ByResourceType**: Shard by resource type (aws_vpc, aws_subnet, etc.)
//! - **ByPrefix**: Shard by a user-defined prefix pattern
//!
//! ## Usage
//!
//! ```rust,no_run
//! # use rustible::provisioning::state_shard::{ShardedState, ShardingStrategy};
//! # use rustible::provisioning::state::ProvisioningState;
//! // Split a large state into shards
//! let state = ProvisioningState::new();
//! let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);
//!
//! // Merge shards back into a single state
//! let merged = sharded.merge();
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing;

use super::state::{ProvisioningState, ResourceState};

// ============================================================================
// Sharding Strategy
// ============================================================================

/// Strategy for splitting state into shards
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ShardingStrategy {
    /// No sharding - single monolithic state
    None,

    /// Shard by cloud provider name (e.g., "aws", "azure", "gcp")
    ByProvider,

    /// Shard by resource type (e.g., "aws_vpc", "aws_subnet")
    ByResourceType,

    /// Shard by a prefix extracted from the resource address
    ///
    /// The prefix is the portion of the resource name before the delimiter.
    /// For example, with delimiter "_", resource "prod_vpc_main" would be
    /// sharded under prefix "prod".
    ByPrefix(String),
}

impl ShardingStrategy {
    /// Compute the shard key for a given resource state
    pub fn shard_key(&self, resource: &ResourceState) -> String {
        match self {
            ShardingStrategy::None => "default".to_string(),

            ShardingStrategy::ByProvider => resource.provider.clone(),

            ShardingStrategy::ByResourceType => resource.resource_type.clone(),

            ShardingStrategy::ByPrefix(delimiter) => {
                let name = &resource.id.name;
                match name.find(delimiter.as_str()) {
                    Some(idx) => name[..idx].to_string(),
                    // If no delimiter found, use the full name as key
                    None => name.to_string(),
                }
            }
        }
    }
}

impl std::fmt::Display for ShardingStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShardingStrategy::None => write!(f, "none"),
            ShardingStrategy::ByProvider => write!(f, "by-provider"),
            ShardingStrategy::ByResourceType => write!(f, "by-resource-type"),
            ShardingStrategy::ByPrefix(delim) => write!(f, "by-prefix({})", delim),
        }
    }
}

// ============================================================================
// Sharded State
// ============================================================================

/// State split into multiple shards based on a sharding strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardedState {
    /// The sharding strategy used
    pub strategy: ShardingStrategy,

    /// Individual state shards keyed by shard key
    pub shards: HashMap<String, ProvisioningState>,
}

impl ShardedState {
    /// Create a new empty sharded state
    pub fn new(strategy: ShardingStrategy) -> Self {
        Self {
            strategy,
            shards: HashMap::new(),
        }
    }

    /// Split a monolithic state into shards using the given strategy
    ///
    /// Each resource is assigned to a shard based on the strategy's shard key.
    /// Metadata (version, lineage, outputs, providers) is copied to all shards.
    pub fn split(state: &ProvisioningState, strategy: ShardingStrategy) -> Self {
        let mut sharded = Self::new(strategy.clone());

        if strategy == ShardingStrategy::None {
            // No sharding - just wrap the entire state
            sharded.shards.insert("default".to_string(), state.clone());

            tracing::debug!("No sharding applied, single shard created");
            return sharded;
        }

        // Group resources by shard key
        let mut resource_groups: HashMap<String, Vec<(String, ResourceState)>> = HashMap::new();

        for (address, resource) in &state.resources {
            let key = strategy.shard_key(resource);
            resource_groups
                .entry(key)
                .or_default()
                .push((address.clone(), resource.clone()));
        }

        // Create a shard for each group
        for (shard_key, resources) in resource_groups {
            let mut shard = ProvisioningState::new();
            // Preserve metadata from original state
            shard.version = state.version;
            shard.lineage = state.lineage.clone();
            shard.outputs = state.outputs.clone();
            shard.providers = state.providers.clone();
            shard.last_modified = state.last_modified;

            for (address, resource) in resources {
                shard.resources.insert(address, resource);
            }

            tracing::debug!(
                shard_key = %shard_key,
                resources = shard.resources.len(),
                "Created state shard"
            );

            sharded.shards.insert(shard_key, shard);
        }

        tracing::info!(
            strategy = %strategy,
            shard_count = sharded.shards.len(),
            "Split state into shards"
        );

        sharded
    }

    /// Merge all shards back into a unified state
    ///
    /// Combines all resources from all shards into a single state.
    /// Uses the metadata (version, lineage, etc.) from the first shard found.
    pub fn merge(&self) -> ProvisioningState {
        let mut merged = ProvisioningState::new();

        // Use metadata from first shard
        if let Some(first_shard) = self.shards.values().next() {
            merged.version = first_shard.version;
            merged.lineage = first_shard.lineage.clone();
            merged.outputs = first_shard.outputs.clone();
            merged.providers = first_shard.providers.clone();
            merged.last_modified = first_shard.last_modified;
        }

        // Merge all resources and collect outputs/providers from all shards
        for shard in self.shards.values() {
            for (address, resource) in &shard.resources {
                merged.resources.insert(address.clone(), resource.clone());
            }

            // Merge outputs (later shards may override earlier ones)
            for (key, value) in &shard.outputs {
                merged.outputs.insert(key.clone(), value.clone());
            }

            // Merge providers
            for (key, value) in &shard.providers {
                merged.providers.insert(key.clone(), value.clone());
            }
        }

        tracing::info!(
            shard_count = self.shards.len(),
            total_resources = merged.resources.len(),
            "Merged state shards"
        );

        merged
    }

    /// Get the number of shards
    pub fn shard_count(&self) -> usize {
        self.shards.len()
    }

    /// Get a specific shard by key
    pub fn get_shard(&self, key: &str) -> Option<&ProvisioningState> {
        self.shards.get(key)
    }

    /// Get a mutable reference to a specific shard
    pub fn get_shard_mut(&mut self, key: &str) -> Option<&mut ProvisioningState> {
        self.shards.get_mut(key)
    }

    /// List all shard keys
    pub fn shard_keys(&self) -> Vec<&str> {
        self.shards.keys().map(|k| k.as_str()).collect()
    }

    /// Get total resource count across all shards
    pub fn total_resources(&self) -> usize {
        self.shards.values().map(|s| s.resources.len()).sum()
    }

    /// Get a summary of resources per shard
    pub fn shard_summary(&self) -> HashMap<&str, usize> {
        self.shards
            .iter()
            .map(|(k, v)| (k.as_str(), v.resources.len()))
            .collect()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::state::{ResourceId, ResourceState};
    use serde_json::json;

    /// Create a test resource state with the given parameters
    fn make_resource(resource_type: &str, name: &str, provider: &str) -> (String, ResourceState) {
        let id = ResourceId::new(resource_type, name);
        let address = id.address();
        let resource = ResourceState::new(
            id,
            format!("{}-id-{}", resource_type, name),
            provider,
            json!({"key": "value"}),
            json!({"attr": "computed"}),
        );
        (address, resource)
    }

    /// Create a test state with multiple resources
    fn create_test_state() -> ProvisioningState {
        let mut state = ProvisioningState::new();

        let resources = vec![
            make_resource("aws_vpc", "main", "aws"),
            make_resource("aws_subnet", "public", "aws"),
            make_resource("aws_instance", "web", "aws"),
            make_resource("azure_resource_group", "rg1", "azure"),
            make_resource("azure_virtual_network", "vnet1", "azure"),
            make_resource("gcp_compute_instance", "worker", "gcp"),
        ];

        for (address, resource) in resources {
            state.resources.insert(address, resource);
        }

        state
    }

    /// Create a test state with prefixed resources
    fn create_prefixed_state() -> ProvisioningState {
        let mut state = ProvisioningState::new();

        let resources = vec![
            make_resource("aws_vpc", "prod_main", "aws"),
            make_resource("aws_subnet", "prod_public", "aws"),
            make_resource("aws_vpc", "dev_main", "aws"),
            make_resource("aws_subnet", "dev_private", "aws"),
            make_resource("aws_instance", "staging_web", "aws"),
        ];

        for (address, resource) in resources {
            state.resources.insert(address, resource);
        }

        state
    }

    #[test]
    fn test_sharding_strategy_display() {
        assert_eq!(ShardingStrategy::None.to_string(), "none");
        assert_eq!(ShardingStrategy::ByProvider.to_string(), "by-provider");
        assert_eq!(
            ShardingStrategy::ByResourceType.to_string(),
            "by-resource-type"
        );
        assert_eq!(
            ShardingStrategy::ByPrefix("_".to_string()).to_string(),
            "by-prefix(_)"
        );
    }

    #[test]
    fn test_shard_key_none() {
        let (_, resource) = make_resource("aws_vpc", "main", "aws");
        let strategy = ShardingStrategy::None;
        assert_eq!(strategy.shard_key(&resource), "default");
    }

    #[test]
    fn test_shard_key_by_provider() {
        let strategy = ShardingStrategy::ByProvider;

        let (_, aws_resource) = make_resource("aws_vpc", "main", "aws");
        assert_eq!(strategy.shard_key(&aws_resource), "aws");

        let (_, azure_resource) = make_resource("azure_vnet", "main", "azure");
        assert_eq!(strategy.shard_key(&azure_resource), "azure");
    }

    #[test]
    fn test_shard_key_by_resource_type() {
        let strategy = ShardingStrategy::ByResourceType;

        let (_, vpc) = make_resource("aws_vpc", "main", "aws");
        assert_eq!(strategy.shard_key(&vpc), "aws_vpc");

        let (_, subnet) = make_resource("aws_subnet", "public", "aws");
        assert_eq!(strategy.shard_key(&subnet), "aws_subnet");
    }

    #[test]
    fn test_shard_key_by_prefix() {
        let strategy = ShardingStrategy::ByPrefix("_".to_string());

        let (_, prod) = make_resource("aws_vpc", "prod_main", "aws");
        assert_eq!(strategy.shard_key(&prod), "prod");

        let (_, dev) = make_resource("aws_vpc", "dev_main", "aws");
        assert_eq!(strategy.shard_key(&dev), "dev");

        // No delimiter -> full name
        let (_, no_prefix) = make_resource("aws_vpc", "main", "aws");
        assert_eq!(strategy.shard_key(&no_prefix), "main");
    }

    #[test]
    fn test_split_none_strategy() {
        let state = create_test_state();
        let sharded = ShardedState::split(&state, ShardingStrategy::None);

        assert_eq!(sharded.shard_count(), 1);
        assert_eq!(sharded.total_resources(), 6);

        let default = sharded.get_shard("default").unwrap();
        assert_eq!(default.resources.len(), 6);
    }

    #[test]
    fn test_split_by_provider() {
        let state = create_test_state();
        let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);

        assert_eq!(sharded.shard_count(), 3);
        assert_eq!(sharded.total_resources(), 6);

        let aws = sharded.get_shard("aws").unwrap();
        assert_eq!(aws.resources.len(), 3);

        let azure = sharded.get_shard("azure").unwrap();
        assert_eq!(azure.resources.len(), 2);

        let gcp = sharded.get_shard("gcp").unwrap();
        assert_eq!(gcp.resources.len(), 1);
    }

    #[test]
    fn test_split_by_resource_type() {
        let state = create_test_state();
        let sharded = ShardedState::split(&state, ShardingStrategy::ByResourceType);

        assert_eq!(sharded.total_resources(), 6);

        // Each unique resource type should have its own shard
        assert!(sharded.get_shard("aws_vpc").is_some());
        assert!(sharded.get_shard("aws_subnet").is_some());
        assert!(sharded.get_shard("aws_instance").is_some());
        assert!(sharded.get_shard("azure_resource_group").is_some());
        assert!(sharded.get_shard("azure_virtual_network").is_some());
        assert!(sharded.get_shard("gcp_compute_instance").is_some());

        let vpc_shard = sharded.get_shard("aws_vpc").unwrap();
        assert_eq!(vpc_shard.resources.len(), 1);
    }

    #[test]
    fn test_split_by_prefix() {
        let state = create_prefixed_state();
        let sharded = ShardedState::split(&state, ShardingStrategy::ByPrefix("_".to_string()));

        assert_eq!(sharded.total_resources(), 5);

        let prod = sharded.get_shard("prod").unwrap();
        assert_eq!(prod.resources.len(), 2);

        let dev = sharded.get_shard("dev").unwrap();
        assert_eq!(dev.resources.len(), 2);

        let staging = sharded.get_shard("staging").unwrap();
        assert_eq!(staging.resources.len(), 1);
    }

    #[test]
    fn test_merge_roundtrip() {
        let state = create_test_state();
        let original_count = state.resources.len();

        // Split and merge should preserve all resources
        let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);
        let merged = sharded.merge();

        assert_eq!(merged.resources.len(), original_count);

        // All original resource addresses should be present
        for address in state.resources.keys() {
            assert!(
                merged.resources.contains_key(address),
                "Missing resource: {}",
                address
            );
        }
    }

    #[test]
    fn test_merge_preserves_metadata() {
        let mut state = create_test_state();
        state.version = 2;
        state.lineage = "test-lineage-123".to_string();

        let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);
        let merged = sharded.merge();

        assert_eq!(merged.version, 2);
        assert_eq!(merged.lineage, "test-lineage-123");
    }

    #[test]
    fn test_merge_empty_shards() {
        let sharded = ShardedState::new(ShardingStrategy::None);
        let merged = sharded.merge();

        // Should produce a valid empty state
        assert!(merged.resources.is_empty());
    }

    #[test]
    fn test_shard_keys() {
        let state = create_test_state();
        let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);

        let mut keys = sharded.shard_keys();
        keys.sort();
        assert_eq!(keys, vec!["aws", "azure", "gcp"]);
    }

    #[test]
    fn test_shard_summary() {
        let state = create_test_state();
        let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);

        let summary = sharded.shard_summary();
        assert_eq!(*summary.get("aws").unwrap(), 3);
        assert_eq!(*summary.get("azure").unwrap(), 2);
        assert_eq!(*summary.get("gcp").unwrap(), 1);
    }

    #[test]
    fn test_get_shard_mut() {
        let state = create_test_state();
        let mut sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);

        let aws_shard = sharded.get_shard_mut("aws").unwrap();
        let (address, new_resource) = make_resource("aws_s3_bucket", "data", "aws");
        aws_shard.resources.insert(address, new_resource);

        assert_eq!(sharded.get_shard("aws").unwrap().resources.len(), 4);
        assert_eq!(sharded.total_resources(), 7);
    }

    #[test]
    fn test_sharded_state_serialization() {
        let state = create_test_state();
        let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);

        let json = serde_json::to_string_pretty(&sharded).unwrap();
        let deserialized: ShardedState = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.strategy, sharded.strategy);
        assert_eq!(deserialized.shard_count(), sharded.shard_count());
        assert_eq!(deserialized.total_resources(), sharded.total_resources());
    }

    #[test]
    fn test_split_empty_state() {
        let state = ProvisioningState::new();

        let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);
        assert_eq!(sharded.shard_count(), 0);
        assert_eq!(sharded.total_resources(), 0);

        let merged = sharded.merge();
        assert!(merged.resources.is_empty());
    }

    #[test]
    fn test_sharding_strategy_equality() {
        assert_eq!(ShardingStrategy::None, ShardingStrategy::None);
        assert_eq!(ShardingStrategy::ByProvider, ShardingStrategy::ByProvider);
        assert_ne!(ShardingStrategy::None, ShardingStrategy::ByProvider);
        assert_eq!(
            ShardingStrategy::ByPrefix("_".to_string()),
            ShardingStrategy::ByPrefix("_".to_string())
        );
        assert_ne!(
            ShardingStrategy::ByPrefix("_".to_string()),
            ShardingStrategy::ByPrefix("-".to_string())
        );
    }

    #[test]
    fn test_by_prefix_with_multi_char_delimiter() {
        let strategy = ShardingStrategy::ByPrefix("--".to_string());

        let (_, resource) = make_resource("aws_vpc", "env--prod--main", "aws");
        assert_eq!(strategy.shard_key(&resource), "env");

        let (_, no_delim) = make_resource("aws_vpc", "simple", "aws");
        assert_eq!(strategy.shard_key(&no_delim), "simple");
    }

    #[test]
    fn test_split_preserves_resource_attributes() {
        let state = create_test_state();
        let sharded = ShardedState::split(&state, ShardingStrategy::ByProvider);

        let aws_shard = sharded.get_shard("aws").unwrap();
        let vpc = aws_shard.resources.get("aws_vpc.main").unwrap();

        assert_eq!(vpc.provider, "aws");
        assert_eq!(vpc.resource_type, "aws_vpc");
        assert_eq!(vpc.id.name, "main");
        assert_eq!(vpc.config, json!({"key": "value"}));
        assert_eq!(vpc.attributes, json!({"attr": "computed"}));
    }
}
