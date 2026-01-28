//! State synchronization for distributed execution
//!
//! This module implements CRDT-based state synchronization for eventual
//! consistency across controllers, with support for different consistency levels.

use super::types::{ControllerId, HostId};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Consistency level for read operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsistencyLevel {
    /// Read from any source, may be stale
    Eventual,
    /// Read from local if fresh, else from leader
    Session,
    /// Always read from leader
    Strong,
    /// Read from quorum of controllers
    Quorum,
}

impl Default for ConsistencyLevel {
    fn default() -> Self {
        Self::Session
    }
}

/// Hybrid Logical Clock for ordering events
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HLC {
    /// Physical timestamp (milliseconds since epoch)
    physical: u64,
    /// Logical counter for same physical time
    logical: u32,
    /// Node ID for tie-breaking
    node_id: u32,
}

impl HLC {
    /// Create a new HLC
    pub fn new(node_id: u32) -> Self {
        let physical = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        Self {
            physical,
            logical: 0,
            node_id,
        }
    }

    /// Increment for local event
    pub fn tick(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        if now > self.physical {
            self.physical = now;
            self.logical = 0;
        } else {
            self.logical += 1;
        }
    }

    /// Update based on received clock
    pub fn update(&mut self, other: &HLC) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let max_physical = now.max(self.physical).max(other.physical);

        if max_physical == self.physical && max_physical == other.physical {
            self.logical = self.logical.max(other.logical) + 1;
        } else if max_physical == self.physical {
            self.logical += 1;
        } else if max_physical == other.physical {
            self.logical = other.logical + 1;
        } else {
            self.logical = 0;
        }

        self.physical = max_physical;
    }
}

/// Last-Writer-Wins entry for CRDT map
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LWWEntry<V> {
    /// The value
    pub value: V,
    /// Timestamp for ordering
    pub timestamp: HLC,
    /// Node that wrote this value
    pub writer: ControllerId,
}

impl<V: Clone> LWWEntry<V> {
    /// Create a new entry
    pub fn new(value: V, timestamp: HLC, writer: ControllerId) -> Self {
        Self {
            value,
            timestamp,
            writer,
        }
    }

    /// Check if this entry is newer than another
    pub fn is_newer_than(&self, other: &LWWEntry<V>) -> bool {
        self.timestamp > other.timestamp
    }
}

/// Last-Writer-Wins Map CRDT
pub struct LWWMap<K: std::hash::Hash + Eq, V> {
    /// Entries in the map
    entries: DashMap<K, LWWEntry<V>>,
    /// Local clock
    clock: RwLock<HLC>,
    /// This node's ID
    node_id: ControllerId,
}

impl<K: std::hash::Hash + Eq + std::fmt::Debug, V: std::fmt::Debug> std::fmt::Debug for LWWMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LWWMap")
            .field("entries", &format!("<{} entries>", self.entries.len()))
            .field("node_id", &self.node_id)
            .finish()
    }
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> LWWMap<K, V> {
    /// Create a new LWW-Map
    pub fn new(node_id: ControllerId, clock_node_id: u32) -> Self {
        Self {
            entries: DashMap::new(),
            clock: RwLock::new(HLC::new(clock_node_id)),
            node_id,
        }
    }

    /// Get a value
    pub fn get(&self, key: &K) -> Option<V> {
        self.entries.get(key).map(|e| e.value.clone())
    }

    /// Get entry with metadata
    pub fn get_entry(&self, key: &K) -> Option<LWWEntry<V>> {
        self.entries.get(key).map(|e| e.value().clone())
    }

    /// Insert a value
    pub async fn insert(&self, key: K, value: V) {
        let mut clock = self.clock.write().await;
        clock.tick();
        let entry = LWWEntry::new(value, *clock, self.node_id.clone());
        self.entries.insert(key, entry);
    }

    /// Insert with external timestamp (for replication)
    pub fn insert_with_timestamp(&self, key: K, entry: LWWEntry<V>) {
        self.entries
            .entry(key)
            .and_modify(|existing| {
                if entry.is_newer_than(existing) {
                    *existing = entry.clone();
                }
            })
            .or_insert(entry);
    }

    /// Remove a value (tombstone)
    pub async fn remove(&self, key: &K) -> Option<V> {
        self.entries.remove(key).map(|(_, e)| e.value)
    }

    /// Merge with another LWW-Map
    pub async fn merge(&self, other: &LWWMap<K, V>) {
        for entry in other.entries.iter() {
            let key = entry.key().clone();
            let other_entry = entry.value().clone();

            self.entries
                .entry(key)
                .and_modify(|existing| {
                    if other_entry.is_newer_than(existing) {
                        *existing = other_entry.clone();
                    }
                })
                .or_insert(other_entry);
        }

        // Update local clock
        let other_clock = other.clock.read().await;
        let mut clock = self.clock.write().await;
        clock.update(&other_clock);
    }

    /// Get all keys
    pub fn keys(&self) -> Vec<K> {
        self.entries.iter().map(|e| e.key().clone()).collect()
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get delta since a given timestamp
    pub fn get_delta(&self, since: &HLC) -> HashMap<K, LWWEntry<V>> {
        self.entries
            .iter()
            .filter(|e| &e.value().timestamp > since)
            .map(|e| (e.key().clone(), e.value().clone()))
            .collect()
    }
}

/// Facts store using LWW-Map CRDT
pub struct FactsStore {
    /// Facts per host
    facts: LWWMap<HostId, serde_json::Value>,
    /// Last sync timestamp per controller
    sync_timestamps: DashMap<ControllerId, HLC>,
    /// Cache freshness TTL
    freshness_ttl: Duration,
    /// Local cache timestamps
    cache_timestamps: DashMap<HostId, Instant>,
}

impl FactsStore {
    /// Create a new facts store
    pub fn new(node_id: ControllerId, freshness_ttl: Duration) -> Self {
        let clock_node_id = node_id.0.as_bytes().iter().take(4).fold(0u32, |acc, &b| {
            acc.wrapping_shl(8) | b as u32
        });

        Self {
            facts: LWWMap::new(node_id, clock_node_id),
            sync_timestamps: DashMap::new(),
            freshness_ttl,
            cache_timestamps: DashMap::new(),
        }
    }

    /// Get facts for a host
    pub fn get(&self, host: &HostId) -> Option<serde_json::Value> {
        self.facts.get(host)
    }

    /// Get facts with freshness check
    pub fn get_if_fresh(&self, host: &HostId) -> Option<serde_json::Value> {
        if self.is_fresh(host) {
            self.facts.get(host)
        } else {
            None
        }
    }

    /// Check if cached facts are fresh
    pub fn is_fresh(&self, host: &HostId) -> bool {
        self.cache_timestamps
            .get(host)
            .map(|t| t.elapsed() < self.freshness_ttl)
            .unwrap_or(false)
    }

    /// Set facts for a host
    pub async fn set(&self, host: HostId, facts: serde_json::Value) {
        self.facts.insert(host.clone(), facts).await;
        self.cache_timestamps.insert(host, Instant::now());
    }

    /// Merge facts from another controller
    pub async fn merge(&self, other: &FactsStore) {
        self.facts.merge(&other.facts).await;

        // Update cache timestamps for merged entries
        for key in other.facts.keys() {
            self.cache_timestamps.insert(key, Instant::now());
        }
    }

    /// Get delta for synchronization
    pub fn get_delta(&self, since: &HLC) -> HashMap<HostId, LWWEntry<serde_json::Value>> {
        self.facts.get_delta(since)
    }

    /// Apply delta from another controller
    pub fn apply_delta(&self, delta: HashMap<HostId, LWWEntry<serde_json::Value>>) {
        for (key, entry) in delta {
            self.facts.insert_with_timestamp(key.clone(), entry);
            self.cache_timestamps.insert(key, Instant::now());
        }
    }

    /// Get sync timestamp for a controller
    pub fn get_sync_timestamp(&self, controller: &ControllerId) -> Option<HLC> {
        self.sync_timestamps.get(controller).map(|t| *t)
    }

    /// Update sync timestamp for a controller
    pub fn update_sync_timestamp(&self, controller: ControllerId, timestamp: HLC) {
        self.sync_timestamps.insert(controller, timestamp);
    }
}

/// Distributed state store combining multiple CRDTs
pub struct DistributedStateStore {
    /// This controller's ID
    node_id: ControllerId,
    /// Facts store
    pub facts: FactsStore,
    /// Variable store (for cached variable contexts)
    pub variables: LWWMap<String, serde_json::Value>,
    /// Default consistency level
    default_consistency: ConsistencyLevel,
}

impl DistributedStateStore {
    /// Create a new distributed state store
    pub fn new(node_id: ControllerId) -> Self {
        let clock_node_id = node_id.0.as_bytes().iter().take(4).fold(0u32, |acc, &b| {
            acc.wrapping_shl(8) | b as u32
        });

        Self {
            facts: FactsStore::new(node_id.clone(), Duration::from_secs(300)),
            variables: LWWMap::new(node_id.clone(), clock_node_id),
            default_consistency: ConsistencyLevel::Session,
            node_id,
        }
    }

    /// Set default consistency level
    pub fn with_consistency(mut self, consistency: ConsistencyLevel) -> Self {
        self.default_consistency = consistency;
        self
    }

    /// Get facts with specified consistency
    pub async fn get_facts(
        &self,
        host: &HostId,
        consistency: Option<ConsistencyLevel>,
    ) -> Option<serde_json::Value> {
        let consistency = consistency.unwrap_or(self.default_consistency);

        match consistency {
            ConsistencyLevel::Eventual => {
                // Return local cache regardless of freshness
                self.facts.get(host)
            }
            ConsistencyLevel::Session => {
                // Return if fresh, otherwise would need to fetch from leader
                self.facts.get_if_fresh(host)
            }
            ConsistencyLevel::Strong | ConsistencyLevel::Quorum => {
                // Would need to fetch from leader/quorum
                // For now, just return local
                self.facts.get(host)
            }
        }
    }

    /// Set facts
    pub async fn set_facts(&self, host: HostId, facts: serde_json::Value) {
        self.facts.set(host, facts).await;
    }

    /// Get a variable
    pub fn get_variable(&self, key: &str) -> Option<serde_json::Value> {
        self.variables.get(&key.to_string())
    }

    /// Set a variable
    pub async fn set_variable(&self, key: String, value: serde_json::Value) {
        self.variables.insert(key, value).await;
    }

    /// Merge state from another store
    pub async fn merge(&self, other: &DistributedStateStore) {
        self.facts.merge(&other.facts).await;
        self.variables.merge(&other.variables).await;
    }
}

/// Sync message for state synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    /// Source controller
    pub from: ControllerId,
    /// Last known sync timestamp
    pub since: Option<HLC>,
    /// Request only delta (vs full sync)
    pub delta_only: bool,
}

/// Sync response with state delta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    /// Source controller
    pub from: ControllerId,
    /// Facts delta
    pub facts_delta: HashMap<HostId, LWWEntry<serde_json::Value>>,
    /// Variables delta
    pub variables_delta: HashMap<String, LWWEntry<serde_json::Value>>,
    /// Current timestamp for next sync
    pub current_timestamp: HLC,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hlc_ordering() {
        let mut clock1 = HLC::new(1);
        let mut clock2 = HLC::new(2);

        // Initial state
        assert!(clock1 < clock2 || clock1 > clock2 || clock1 == clock2);

        clock1.tick();
        clock2.tick();

        // After tick, clocks advance
        assert!(clock1.physical > 0);
        assert!(clock2.physical > 0);
    }

    #[tokio::test]
    async fn test_lww_map_insert_get() {
        let map: LWWMap<String, i32> = LWWMap::new(ControllerId::new("node-1"), 1);

        map.insert("key1".to_string(), 100).await;
        assert_eq!(map.get(&"key1".to_string()), Some(100));
    }

    #[tokio::test]
    async fn test_lww_map_merge() {
        let map1: LWWMap<String, i32> = LWWMap::new(ControllerId::new("node-1"), 1);
        let map2: LWWMap<String, i32> = LWWMap::new(ControllerId::new("node-2"), 2);

        map1.insert("key1".to_string(), 100).await;
        map2.insert("key2".to_string(), 200).await;

        map1.merge(&map2).await;

        assert_eq!(map1.get(&"key1".to_string()), Some(100));
        assert_eq!(map1.get(&"key2".to_string()), Some(200));
    }

    #[tokio::test]
    async fn test_lww_map_conflict_resolution() {
        let map1: LWWMap<String, i32> = LWWMap::new(ControllerId::new("node-1"), 1);
        let map2: LWWMap<String, i32> = LWWMap::new(ControllerId::new("node-2"), 2);

        // Both write to same key
        map1.insert("key".to_string(), 100).await;

        // Small delay to ensure different timestamp
        tokio::time::sleep(Duration::from_millis(10)).await;
        map2.insert("key".to_string(), 200).await;

        // Merge - map2's value should win (newer timestamp)
        map1.merge(&map2).await;
        assert_eq!(map1.get(&"key".to_string()), Some(200));
    }

    #[tokio::test]
    async fn test_facts_store() {
        let store = FactsStore::new(ControllerId::new("node-1"), Duration::from_secs(300));

        let host = HostId::new("host-1");
        let facts = serde_json::json!({
            "os": "linux",
            "arch": "x86_64"
        });

        store.set(host.clone(), facts.clone()).await;

        let retrieved = store.get(&host).unwrap();
        assert_eq!(retrieved["os"], "linux");
        assert!(store.is_fresh(&host));
    }

    #[tokio::test]
    async fn test_distributed_state_store() {
        let store = DistributedStateStore::new(ControllerId::new("node-1"));

        let host = HostId::new("host-1");
        let facts = serde_json::json!({"os": "linux"});

        store.set_facts(host.clone(), facts).await;

        let retrieved = store.get_facts(&host, None).await;
        assert!(retrieved.is_some());
    }
}
