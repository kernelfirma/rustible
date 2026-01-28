//! Work distribution for distributed execution
//!
//! This module handles distributing work units across controllers using
//! various assignment strategies.

use super::types::{ControllerId, ControllerLoad, HostId, WorkUnit, WorkUnitId, WorkUnitState};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;

/// Work assignment strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentStrategy {
    /// Simple round-robin distribution
    RoundRobin,
    /// Assign based on controller capacity and current load
    CapacityAware,
    /// Assign based on host affinity (keep hosts on same controller)
    Affinity,
    /// Adaptive strategy combining multiple signals
    Adaptive,
}

impl Default for AssignmentStrategy {
    fn default() -> Self {
        Self::Adaptive
    }
}

/// Trait for work assignment strategies
pub trait WorkAssigner: Send + Sync {
    /// Assign a work unit to a controller
    fn assign(&self, work_unit: &WorkUnit) -> Option<ControllerId>;

    /// Update controller load information
    fn update_load(&self, controller_id: ControllerId, load: ControllerLoad);

    /// Mark controller as unavailable
    fn mark_unavailable(&self, controller_id: &ControllerId);

    /// Mark controller as available
    fn mark_available(&self, controller_id: &ControllerId);
}

/// Round-robin work assigner
pub struct RoundRobinAssigner {
    controllers: RwLock<Vec<ControllerId>>,
    next_index: AtomicUsize,
    unavailable: DashMap<ControllerId, ()>,
}

impl RoundRobinAssigner {
    /// Create a new round-robin assigner
    pub fn new(controllers: Vec<ControllerId>) -> Self {
        Self {
            controllers: RwLock::new(controllers),
            next_index: AtomicUsize::new(0),
            unavailable: DashMap::new(),
        }
    }

    /// Add a controller
    pub async fn add_controller(&self, id: ControllerId) {
        let mut controllers = self.controllers.write().await;
        if !controllers.contains(&id) {
            controllers.push(id);
        }
    }

    /// Remove a controller
    pub async fn remove_controller(&self, id: &ControllerId) {
        let mut controllers = self.controllers.write().await;
        controllers.retain(|c| c != id);
    }
}

impl WorkAssigner for RoundRobinAssigner {
    fn assign(&self, _work_unit: &WorkUnit) -> Option<ControllerId> {
        // This is a synchronous method so we need to handle this differently
        // For now, we'll use a simpler approach
        None // Will be handled in async context
    }

    fn update_load(&self, _controller_id: ControllerId, _load: ControllerLoad) {
        // Round-robin doesn't use load information
    }

    fn mark_unavailable(&self, controller_id: &ControllerId) {
        self.unavailable.insert(controller_id.clone(), ());
    }

    fn mark_available(&self, controller_id: &ControllerId) {
        self.unavailable.remove(controller_id);
    }
}

impl RoundRobinAssigner {
    /// Assign a work unit (async version)
    pub async fn assign_async(&self, _work_unit: &WorkUnit) -> Option<ControllerId> {
        let controllers = self.controllers.read().await;
        if controllers.is_empty() {
            return None;
        }

        // Find next available controller
        let start_idx = self.next_index.fetch_add(1, Ordering::SeqCst);
        for i in 0..controllers.len() {
            let idx = (start_idx + i) % controllers.len();
            let controller = &controllers[idx];
            if !self.unavailable.contains_key(controller) {
                return Some(controller.clone());
            }
        }

        None
    }
}

/// Capacity-aware work assigner
pub struct CapacityAwareAssigner {
    controller_loads: DashMap<ControllerId, ControllerLoad>,
    unavailable: DashMap<ControllerId, ()>,
}

impl CapacityAwareAssigner {
    /// Create a new capacity-aware assigner
    pub fn new() -> Self {
        Self {
            controller_loads: DashMap::new(),
            unavailable: DashMap::new(),
        }
    }

    /// Add a controller with initial load
    pub fn add_controller(&self, id: ControllerId, load: ControllerLoad) {
        self.controller_loads.insert(id, load);
    }

    /// Remove a controller
    pub fn remove_controller(&self, id: &ControllerId) {
        self.controller_loads.remove(id);
    }

    /// Get controller with lowest load
    fn get_least_loaded(&self) -> Option<ControllerId> {
        self.controller_loads
            .iter()
            .filter(|entry| !self.unavailable.contains_key(entry.key()))
            .filter(|entry| entry.value().can_accept_work())
            .min_by(|a, b| {
                let score_a = a.value().load_score();
                let score_b = b.value().load_score();
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|entry| entry.key().clone())
    }
}

impl Default for CapacityAwareAssigner {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkAssigner for CapacityAwareAssigner {
    fn assign(&self, _work_unit: &WorkUnit) -> Option<ControllerId> {
        self.get_least_loaded()
    }

    fn update_load(&self, controller_id: ControllerId, load: ControllerLoad) {
        self.controller_loads.insert(controller_id, load);
    }

    fn mark_unavailable(&self, controller_id: &ControllerId) {
        self.unavailable.insert(controller_id.clone(), ());
    }

    fn mark_available(&self, controller_id: &ControllerId) {
        self.unavailable.remove(controller_id);
    }
}

/// Affinity-based work assigner
pub struct AffinityAssigner {
    /// Host to controller affinity map
    host_affinity: DashMap<HostId, ControllerId>,
    /// Region to controllers map
    region_controllers: RwLock<HashMap<String, Vec<ControllerId>>>,
    /// Controller loads for fallback selection
    controller_loads: DashMap<ControllerId, ControllerLoad>,
    /// Unavailable controllers
    unavailable: DashMap<ControllerId, ()>,
}

impl AffinityAssigner {
    /// Create a new affinity-based assigner
    pub fn new() -> Self {
        Self {
            host_affinity: DashMap::new(),
            region_controllers: RwLock::new(HashMap::new()),
            controller_loads: DashMap::new(),
            unavailable: DashMap::new(),
        }
    }

    /// Set host affinity to a controller
    pub fn set_host_affinity(&self, host: HostId, controller: ControllerId) {
        self.host_affinity.insert(host, controller);
    }

    /// Add controller to a region
    pub async fn add_controller_to_region(&self, controller: ControllerId, region: String) {
        let mut regions = self.region_controllers.write().await;
        regions
            .entry(region)
            .or_insert_with(Vec::new)
            .push(controller);
    }

    /// Get controller for host based on affinity
    fn get_by_affinity(&self, work_unit: &WorkUnit) -> Option<ControllerId> {
        // Check if any host in the work unit has affinity
        for host in &work_unit.hosts {
            if let Some(controller) = self.host_affinity.get(host) {
                let ctrl_id = controller.value().clone();
                if !self.unavailable.contains_key(&ctrl_id) {
                    // Check if controller can accept work
                    if let Some(load) = self.controller_loads.get(&ctrl_id) {
                        if load.can_accept_work() {
                            return Some(ctrl_id);
                        }
                    } else {
                        return Some(ctrl_id);
                    }
                }
            }
        }
        None
    }

    /// Get least loaded controller as fallback
    fn get_least_loaded(&self) -> Option<ControllerId> {
        self.controller_loads
            .iter()
            .filter(|entry| !self.unavailable.contains_key(entry.key()))
            .filter(|entry| entry.value().can_accept_work())
            .min_by(|a, b| {
                let score_a = a.value().load_score();
                let score_b = b.value().load_score();
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|entry| entry.key().clone())
    }
}

impl Default for AffinityAssigner {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkAssigner for AffinityAssigner {
    fn assign(&self, work_unit: &WorkUnit) -> Option<ControllerId> {
        // First try affinity-based assignment
        if let Some(controller) = self.get_by_affinity(work_unit) {
            return Some(controller);
        }

        // Fall back to least loaded
        self.get_least_loaded()
    }

    fn update_load(&self, controller_id: ControllerId, load: ControllerLoad) {
        self.controller_loads.insert(controller_id, load);
    }

    fn mark_unavailable(&self, controller_id: &ControllerId) {
        self.unavailable.insert(controller_id.clone(), ());
    }

    fn mark_available(&self, controller_id: &ControllerId) {
        self.unavailable.remove(controller_id);
    }
}

/// Work queue for managing pending work units
pub struct WorkQueue {
    /// Pending work units by priority
    pending: RwLock<VecDeque<WorkUnit>>,
    /// Work units by ID for quick lookup
    by_id: DashMap<WorkUnitId, WorkUnitState>,
    /// Assigned work units
    assigned: DashMap<WorkUnitId, ControllerId>,
    /// Completed work unit IDs
    completed: RwLock<HashSet<WorkUnitId>>,
}

impl WorkQueue {
    /// Create a new work queue
    pub fn new() -> Self {
        Self {
            pending: RwLock::new(VecDeque::new()),
            by_id: DashMap::new(),
            assigned: DashMap::new(),
            completed: RwLock::new(HashSet::new()),
        }
    }

    /// Add a work unit to the queue
    pub async fn enqueue(&self, work_unit: WorkUnit) {
        let id = work_unit.id.clone();
        self.by_id.insert(id.clone(), WorkUnitState::Pending);

        let mut pending = self.pending.write().await;

        // Insert based on priority (higher priority first)
        let pos = pending
            .iter()
            .position(|wu| wu.priority < work_unit.priority)
            .unwrap_or(pending.len());

        pending.insert(pos, work_unit);
    }

    /// Get the next work unit that has all dependencies satisfied
    pub async fn dequeue(&self) -> Option<WorkUnit> {
        let completed = self.completed.read().await;
        let mut pending = self.pending.write().await;

        // Find first work unit with satisfied dependencies
        let pos = pending.iter().position(|wu| {
            wu.dependencies
                .iter()
                .all(|dep| completed.contains(dep))
        });

        if let Some(pos) = pos {
            let work_unit = pending.remove(pos)?;
            self.by_id.insert(work_unit.id.clone(), WorkUnitState::Assigned);
            Some(work_unit)
        } else {
            None
        }
    }

    /// Mark a work unit as assigned to a controller
    pub fn assign(&self, work_unit_id: WorkUnitId, controller_id: ControllerId) {
        self.assigned.insert(work_unit_id.clone(), controller_id);
        self.by_id.insert(work_unit_id, WorkUnitState::Running);
    }

    /// Mark a work unit as completed
    pub async fn complete(&self, work_unit_id: WorkUnitId) {
        self.assigned.remove(&work_unit_id);
        self.by_id.insert(work_unit_id.clone(), WorkUnitState::Completed);
        self.completed.write().await.insert(work_unit_id);
    }

    /// Mark a work unit as failed
    pub fn fail(&self, work_unit_id: WorkUnitId, error: String) {
        self.assigned.remove(&work_unit_id);
        self.by_id.insert(work_unit_id, WorkUnitState::Failed { error });
    }

    /// Get work unit state
    pub fn state(&self, work_unit_id: &WorkUnitId) -> Option<WorkUnitState> {
        self.by_id.get(work_unit_id).map(|r| r.value().clone())
    }

    /// Get assigned controller for a work unit
    pub fn assigned_to(&self, work_unit_id: &WorkUnitId) -> Option<ControllerId> {
        self.assigned.get(work_unit_id).map(|r| r.value().clone())
    }

    /// Get number of pending work units
    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }

    /// Get work units assigned to a specific controller
    pub fn work_units_for_controller(&self, controller_id: &ControllerId) -> Vec<WorkUnitId> {
        self.assigned
            .iter()
            .filter(|entry| entry.value() == controller_id)
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Reassign work units from a failed controller
    pub async fn reassign_from_controller(&self, failed_controller: &ControllerId) -> Vec<WorkUnit> {
        let work_unit_ids: Vec<_> = self.work_units_for_controller(failed_controller);

        let reassigned = Vec::new();
        let _pending = self.pending.write().await;

        for id in work_unit_ids {
            self.assigned.remove(&id);
            self.by_id.insert(id.clone(), WorkUnitState::Pending);

            // We need the original work unit - this is a simplified version
            // In practice, we'd store the full work unit
            // For now, we just track the IDs
        }

        reassigned
    }
}

impl Default for WorkQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// Load balancer for work distribution
pub struct LoadBalancer {
    /// Controller loads
    loads: DashMap<ControllerId, ControllerLoad>,
    /// Imbalance threshold (0.0 - 1.0)
    imbalance_threshold: f64,
}

impl LoadBalancer {
    /// Create a new load balancer
    pub fn new(imbalance_threshold: f64) -> Self {
        Self {
            loads: DashMap::new(),
            imbalance_threshold,
        }
    }

    /// Update controller load
    pub fn update_load(&self, controller_id: ControllerId, load: ControllerLoad) {
        self.loads.insert(controller_id, load);
    }

    /// Check if rebalancing is needed
    pub fn needs_rebalancing(&self) -> bool {
        if self.loads.len() < 2 {
            return false;
        }

        let scores: Vec<f64> = self.loads.iter().map(|e| e.value().load_score()).collect();

        let max = scores.iter().cloned().fold(0.0, f64::max);
        let min = scores.iter().cloned().fold(1.0, f64::min);

        (max - min) > self.imbalance_threshold
    }

    /// Get overloaded controllers
    pub fn get_overloaded(&self) -> Vec<ControllerId> {
        let avg: f64 = self.loads.iter().map(|e| e.value().load_score()).sum::<f64>()
            / self.loads.len() as f64;

        self.loads
            .iter()
            .filter(|e| e.value().load_score() > avg + self.imbalance_threshold / 2.0)
            .map(|e| e.key().clone())
            .collect()
    }

    /// Get underloaded controllers
    pub fn get_underloaded(&self) -> Vec<ControllerId> {
        let avg: f64 = self.loads.iter().map(|e| e.value().load_score()).sum::<f64>()
            / self.loads.len() as f64;

        self.loads
            .iter()
            .filter(|e| e.value().load_score() < avg - self.imbalance_threshold / 2.0)
            .map(|e| e.key().clone())
            .collect()
    }

    /// Select best controller for a new work unit
    pub fn select_controller(&self, _work_unit: &WorkUnit) -> Option<ControllerId> {
        self.loads
            .iter()
            .filter(|e| e.value().can_accept_work())
            .min_by(|a, b| {
                let score_a = a.value().load_score();
                let score_b = b.value().load_score();
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|e| e.key().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::distributed::types::RunId;

    fn test_work_unit(priority: u32) -> WorkUnit {
        WorkUnit::new(RunId::generate(), 0, vec![]).with_priority(priority)
    }

    #[tokio::test]
    async fn test_round_robin_assigner() {
        let controllers = vec![
            ControllerId::new("ctrl-1"),
            ControllerId::new("ctrl-2"),
            ControllerId::new("ctrl-3"),
        ];
        let assigner = RoundRobinAssigner::new(controllers.clone());

        let wu = test_work_unit(50);

        let assigned1 = assigner.assign_async(&wu).await.unwrap();
        let assigned2 = assigner.assign_async(&wu).await.unwrap();
        let assigned3 = assigner.assign_async(&wu).await.unwrap();
        let assigned4 = assigner.assign_async(&wu).await.unwrap();

        // Should cycle through controllers
        assert_ne!(assigned1, assigned2);
        assert_ne!(assigned2, assigned3);
        assert_eq!(assigned1, assigned4); // Wraps around
    }

    #[tokio::test]
    async fn test_capacity_aware_assigner() {
        let assigner = CapacityAwareAssigner::new();

        let ctrl1 = ControllerId::new("ctrl-1");
        let ctrl2 = ControllerId::new("ctrl-2");

        // Use active_connections since load_score weights that at 0.4
        let mut load1 = ControllerLoad::default();
        load1.active_connections = 10;
        load1.capacity = 100;

        let mut load2 = ControllerLoad::default();
        load2.active_connections = 80;
        load2.capacity = 100;

        assigner.add_controller(ctrl1.clone(), load1);
        assigner.add_controller(ctrl2.clone(), load2);

        let wu = test_work_unit(50);
        let assigned = assigner.assign(&wu).unwrap();

        // Should pick controller with lower load (ctrl1 has 10% load, ctrl2 has 80%)
        assert_eq!(assigned, ctrl1);
    }

    #[tokio::test]
    async fn test_work_queue_priority() {
        let queue = WorkQueue::new();

        let low_priority = test_work_unit(10);
        let high_priority = test_work_unit(90);
        let medium_priority = test_work_unit(50);

        queue.enqueue(low_priority).await;
        queue.enqueue(high_priority).await;
        queue.enqueue(medium_priority).await;

        // Should dequeue in priority order
        let first = queue.dequeue().await.unwrap();
        assert_eq!(first.priority, 90);

        let second = queue.dequeue().await.unwrap();
        assert_eq!(second.priority, 50);

        let third = queue.dequeue().await.unwrap();
        assert_eq!(third.priority, 10);
    }

    #[tokio::test]
    async fn test_work_queue_dependencies() {
        let queue = WorkQueue::new();

        let dep_id = WorkUnitId::new("dep");
        let wu_with_dep = WorkUnit::new(RunId::generate(), 0, vec![])
            .with_priority(50)
            .with_dependency(dep_id.clone());

        queue.enqueue(wu_with_dep).await;

        // Should not dequeue because dependency not satisfied
        assert!(queue.dequeue().await.is_none());

        // Mark dependency as completed
        queue.complete(dep_id).await;

        // Now should dequeue
        assert!(queue.dequeue().await.is_some());
    }

    #[test]
    fn test_load_balancer_imbalance() {
        let balancer = LoadBalancer::new(0.2);

        let ctrl1 = ControllerId::new("ctrl-1");
        let ctrl2 = ControllerId::new("ctrl-2");

        let mut load1 = ControllerLoad::default();
        load1.capacity = 100;
        load1.active_connections = 80; // High load

        let mut load2 = ControllerLoad::default();
        load2.capacity = 100;
        load2.active_connections = 20; // Low load

        balancer.update_load(ctrl1.clone(), load1);
        balancer.update_load(ctrl2.clone(), load2);

        assert!(balancer.needs_rebalancing());

        let overloaded = balancer.get_overloaded();
        assert!(overloaded.contains(&ctrl1));

        let underloaded = balancer.get_underloaded();
        assert!(underloaded.contains(&ctrl2));
    }
}
