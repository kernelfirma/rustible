//! Cluster management for distributed execution
//!
//! This module handles:
//! - Peer discovery and connection management
//! - Inter-controller communication
//! - Cluster state synchronization

use super::raft::{HeartbeatRequest, HeartbeatResponse, VoteRequest, VoteResponse};
use super::types::{
    ClusterConfig, ControllerHealth, ControllerId, ControllerInfo, WorkUnit, WorkUnitId,
    WorkUnitState,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{interval, MissedTickBehavior};

/// Cluster manager for peer communication
pub struct ClusterManager {
    /// Configuration
    config: ClusterConfig,
    /// Connected peers
    peers: Arc<RwLock<HashMap<ControllerId, PeerConnection>>>,
    /// Peer info cache
    peer_info: Arc<RwLock<HashMap<ControllerId, ControllerInfo>>>,
    /// Message sender for outgoing messages
    message_tx: Arc<RwLock<mpsc::Sender<OutgoingMessage>>>,
    /// Running state
    running: Arc<RwLock<bool>>,
    /// Last observed leader from heartbeat traffic
    known_leader: Arc<RwLock<Option<ControllerId>>>,
    /// Detached background tasks owned by the manager lifecycle
    background_tasks: Arc<RwLock<Vec<JoinHandle<()>>>>,
}

/// Connection to a peer controller
#[derive(Debug)]
pub struct PeerConnection {
    /// Peer's controller ID
    pub id: ControllerId,
    /// Peer's address
    pub address: SocketAddr,
    /// Connection state
    pub state: PeerConnectionState,
    /// Last successful communication
    pub last_seen: Instant,
    /// Pending messages
    pending_messages: usize,
}

/// Peer connection state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerConnectionState {
    /// Not connected
    Disconnected,
    /// Attempting to connect
    Connecting,
    /// Connected and healthy
    Connected,
    /// Connection failed, will retry
    Failed { reason: String },
}

/// Messages between controllers
#[derive(Debug, Clone)]
pub enum PeerMessage {
    /// Vote request for leader election
    VoteRequest(VoteRequest),
    /// Vote response
    VoteResponse(VoteResponse),
    /// Heartbeat from leader
    Heartbeat(HeartbeatRequest),
    /// Heartbeat response
    HeartbeatResponse(HeartbeatResponse),
    /// Work unit assignment
    WorkUnitAssign(WorkUnit),
    /// Work unit status update
    WorkUnitStatus {
        id: WorkUnitId,
        state: WorkUnitState,
    },
    /// Work unit result
    WorkUnitResult {
        id: WorkUnitId,
        results: serde_json::Value,
    },
    /// Controller info exchange
    ControllerInfo(ControllerInfo),
    /// Ping for health check
    Ping,
    /// Pong response
    Pong,
}

/// Outgoing message wrapper
struct OutgoingMessage {
    /// Target controller
    target: ControllerId,
    /// Message to send
    message: PeerMessage,
    /// Response channel (if expecting response)
    response_tx: Option<mpsc::Sender<PeerMessage>>,
}

/// Cluster state snapshot
#[derive(Debug, Clone)]
pub struct ClusterState {
    /// All known controllers
    pub controllers: HashMap<ControllerId, ControllerInfo>,
    /// Current leader
    pub leader: Option<ControllerId>,
    /// Cluster is healthy (has quorum)
    pub healthy: bool,
    /// Total capacity across cluster
    pub total_capacity: usize,
    /// Current load across cluster
    pub total_load: usize,
}

impl ClusterManager {
    /// Create a new cluster manager
    pub async fn new(config: ClusterConfig) -> Result<Self, ClusterError> {
        let (message_tx, _message_rx) = mpsc::channel(1000);

        let mut peers = HashMap::new();

        // Initialize peer connections for configured peers
        for (i, addr) in config.peers.iter().enumerate() {
            let peer_id = ControllerId::new(format!("peer-{}", i));
            peers.insert(
                peer_id.clone(),
                PeerConnection {
                    id: peer_id,
                    address: *addr,
                    state: PeerConnectionState::Disconnected,
                    last_seen: Instant::now(),
                    pending_messages: 0,
                },
            );
        }

        Ok(Self {
            config,
            peers: Arc::new(RwLock::new(peers)),
            peer_info: Arc::new(RwLock::new(HashMap::new())),
            message_tx: Arc::new(RwLock::new(message_tx)),
            running: Arc::new(RwLock::new(false)),
            known_leader: Arc::new(RwLock::new(None)),
            background_tasks: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Start the cluster manager
    pub async fn start(&self) -> Result<(), ClusterError> {
        let mut running = self.running.write().await;
        if *running {
            return Err(ClusterError::AlreadyRunning);
        }
        *running = true;
        drop(running);

        tracing::info!(
            "Cluster manager starting for controller {}",
            self.config.controller_id
        );

        let now = Instant::now();

        // Initialize peer connectivity and baseline health telemetry.
        let mut peers = self.peers.write().await;
        let mut peer_info = self.peer_info.write().await;
        for peer in peers.values_mut() {
            peer.state = PeerConnectionState::Connected;
            peer.last_seen = now;
            peer.pending_messages = 0;

            let info = peer_info
                .entry(peer.id.clone())
                .or_insert_with(|| ControllerInfo::new(peer.id.clone(), peer.address));
            info.address = peer.address;
            info.health = ControllerHealth::Healthy;
            info.last_heartbeat = Some(now);
        }
        drop(peer_info);
        drop(peers);

        {
            let mut leader = self.known_leader.write().await;
            if leader.is_none() {
                *leader = Some(self.config.controller_id.clone());
            }
        }

        // Fresh message channel per lifecycle start to avoid stale receiver state.
        let (message_tx, message_rx) = mpsc::channel(1000);
        *self.message_tx.write().await = message_tx;

        let message_task = Self::spawn_message_drain_loop(
            Arc::clone(&self.peers),
            Arc::clone(&self.peer_info),
            Arc::clone(&self.known_leader),
            message_rx,
        );

        let health_task = Self::spawn_health_update_loop(
            Arc::clone(&self.peers),
            Arc::clone(&self.peer_info),
            Arc::clone(&self.running),
            self.config.heartbeat_interval(),
        );

        let mut background_tasks = self.background_tasks.write().await;
        background_tasks.push(message_task);
        background_tasks.push(health_task);

        Ok(())
    }

    /// Stop the cluster manager
    pub async fn stop(&self) -> Result<(), ClusterError> {
        let mut running = self.running.write().await;
        if !*running {
            return Ok(());
        }
        *running = false;
        drop(running);

        tracing::info!(
            "Cluster manager stopping for controller {}",
            self.config.controller_id
        );

        {
            let mut tasks = self.background_tasks.write().await;
            for task in tasks.drain(..) {
                task.abort();
            }
        }

        // Close all peer connections
        let mut peers = self.peers.write().await;
        for peer in peers.values_mut() {
            peer.state = PeerConnectionState::Disconnected;
            peer.pending_messages = 0;
        }
        drop(peers);

        *self.known_leader.write().await = None;

        Ok(())
    }

    fn spawn_message_drain_loop(
        peers: Arc<RwLock<HashMap<ControllerId, PeerConnection>>>,
        peer_info: Arc<RwLock<HashMap<ControllerId, ControllerInfo>>>,
        known_leader: Arc<RwLock<Option<ControllerId>>>,
        mut message_rx: mpsc::Receiver<OutgoingMessage>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(outgoing) = message_rx.recv().await {
                Self::process_outgoing_message(&peers, &peer_info, &known_leader, outgoing).await;
            }
        })
    }

    fn spawn_health_update_loop(
        peers: Arc<RwLock<HashMap<ControllerId, PeerConnection>>>,
        peer_info: Arc<RwLock<HashMap<ControllerId, ControllerInfo>>>,
        running: Arc<RwLock<bool>>,
        heartbeat_interval: Duration,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let check_interval = heartbeat_interval.max(Duration::from_millis(10));
            let stale_after = check_interval.saturating_mul(3);

            let mut ticker = interval(check_interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                ticker.tick().await;

                if !*running.read().await {
                    break;
                }

                let now = Instant::now();
                let snapshots = {
                    let mut peers_guard = peers.write().await;
                    let mut snapshots = Vec::with_capacity(peers_guard.len());

                    for (id, peer) in peers_guard.iter_mut() {
                        if matches!(peer.state, PeerConnectionState::Connected)
                            && now.saturating_duration_since(peer.last_seen) > stale_after
                        {
                            peer.state = PeerConnectionState::Failed {
                                reason: "health timeout".to_string(),
                            };
                        }

                        snapshots.push((
                            id.clone(),
                            peer.address,
                            peer.state.clone(),
                            peer.last_seen,
                        ));
                    }

                    snapshots
                };

                let mut peer_info_guard = peer_info.write().await;
                for (id, address, state, last_seen) in snapshots {
                    let info = peer_info_guard
                        .entry(id.clone())
                        .or_insert_with(|| ControllerInfo::new(id.clone(), address));
                    info.address = address;
                    info.last_heartbeat = Some(last_seen);
                    info.health = match state {
                        PeerConnectionState::Connected => ControllerHealth::Healthy,
                        PeerConnectionState::Connecting => ControllerHealth::Degraded,
                        PeerConnectionState::Disconnected | PeerConnectionState::Failed { .. } => {
                            ControllerHealth::Down
                        }
                    };
                }
            }
        })
    }

    async fn process_outgoing_message(
        peers: &Arc<RwLock<HashMap<ControllerId, PeerConnection>>>,
        peer_info: &Arc<RwLock<HashMap<ControllerId, ControllerInfo>>>,
        known_leader: &Arc<RwLock<Option<ControllerId>>>,
        outgoing: OutgoingMessage,
    ) {
        let now = Instant::now();
        let OutgoingMessage {
            target,
            message,
            response_tx,
        } = outgoing;

        let mut peer_address = None;
        {
            let mut peers_guard = peers.write().await;
            if let Some(peer) = peers_guard.get_mut(&target) {
                peer.pending_messages = peer.pending_messages.saturating_sub(1);
                peer.last_seen = now;
                peer.state = PeerConnectionState::Connected;
                peer_address = Some(peer.address);
            }
        }

        if let Some(address) = peer_address {
            let mut peer_info_guard = peer_info.write().await;
            let info = peer_info_guard
                .entry(target.clone())
                .or_insert_with(|| ControllerInfo::new(target.clone(), address));
            info.address = address;
            info.health = ControllerHealth::Healthy;
            info.last_heartbeat = Some(now);
        }

        match message {
            PeerMessage::Heartbeat(request) => {
                *known_leader.write().await = Some(request.leader_id.clone());

                if let Some(response_tx) = response_tx {
                    let _ = response_tx
                        .send(PeerMessage::HeartbeatResponse(HeartbeatResponse {
                            term: request.term,
                            success: true,
                        }))
                        .await;
                }
            }
            PeerMessage::Ping => {
                if let Some(response_tx) = response_tx {
                    let _ = response_tx.send(PeerMessage::Pong).await;
                }
            }
            PeerMessage::ControllerInfo(info) => {
                peer_info.write().await.insert(target, info);
            }
            _ => {}
        }
    }

    /// Get connected peers and their info
    pub async fn connected_peers(&self) -> HashMap<ControllerId, ControllerInfo> {
        self.peer_info.read().await.clone()
    }

    /// Get peer connection state
    pub async fn peer_state(&self, id: &ControllerId) -> Option<PeerConnectionState> {
        self.peers.read().await.get(id).map(|p| p.state.clone())
    }

    /// Send a work unit to a specific controller
    pub async fn send_work_unit(
        &self,
        target: &ControllerId,
        work_unit: WorkUnit,
    ) -> Result<(), ClusterError> {
        self.send_message(target, PeerMessage::WorkUnitAssign(work_unit))
            .await
    }

    /// Send a vote request to all peers
    pub async fn broadcast_vote_request(&self, request: VoteRequest) -> Result<(), ClusterError> {
        let peer_ids: Vec<ControllerId> = self.peers.read().await.keys().cloned().collect();
        for peer_id in peer_ids {
            self.send_message(&peer_id, PeerMessage::VoteRequest(request.clone()))
                .await?;
        }
        Ok(())
    }

    /// Send a heartbeat to all peers
    pub async fn broadcast_heartbeat(&self, request: HeartbeatRequest) -> Result<(), ClusterError> {
        let peer_ids: Vec<ControllerId> = self.peers.read().await.keys().cloned().collect();
        for peer_id in peer_ids {
            self.send_message(&peer_id, PeerMessage::Heartbeat(request.clone()))
                .await?;
        }
        Ok(())
    }

    /// Send a message to a specific peer
    async fn send_message(
        &self,
        target: &ControllerId,
        message: PeerMessage,
    ) -> Result<(), ClusterError> {
        // Check if peer exists and is connected
        {
            let mut peers = self.peers.write().await;
            match peers.get_mut(target) {
                Some(peer) if peer.state == PeerConnectionState::Connected => {
                    peer.pending_messages += 1;
                }
                Some(_) => return Err(ClusterError::PeerNotConnected(target.clone())),
                None => return Err(ClusterError::PeerNotFound(target.clone())),
            }
        }

        // Queue message for sending
        let sender = self.message_tx.read().await.clone();
        if sender
            .send(OutgoingMessage {
                target: target.clone(),
                message,
                response_tx: None,
            })
            .await
            .is_err()
        {
            if let Some(peer) = self.peers.write().await.get_mut(target) {
                peer.pending_messages = peer.pending_messages.saturating_sub(1);
            }
            return Err(ClusterError::ChannelClosed);
        }

        Ok(())
    }

    /// Get cluster state snapshot
    pub async fn cluster_state(&self) -> ClusterState {
        let peer_info = self.peer_info.read().await;
        let controllers = peer_info.clone();

        let mut total_capacity: usize = self.config.capacity as usize;

        for info in controllers.values() {
            total_capacity += info.capacity as usize;
        }

        let healthy_count = controllers
            .values()
            .filter(|i| matches!(i.health, ControllerHealth::Healthy))
            .count();
        drop(peer_info);

        let quorum_size = self.config.peers.len().div_ceil(2) + 1;
        let total_load = self
            .peers
            .read()
            .await
            .values()
            .map(|peer| peer.pending_messages)
            .sum();

        let leader = match self.known_leader.read().await.clone() {
            Some(leader) => Some(leader),
            None if *self.running.read().await => Some(self.config.controller_id.clone()),
            None => None,
        };

        ClusterState {
            controllers,
            leader,
            healthy: healthy_count + 1 >= quorum_size, // +1 for self
            total_capacity,
            total_load,
        }
    }

    /// Add a new peer dynamically
    pub async fn add_peer(
        &self,
        id: ControllerId,
        address: SocketAddr,
    ) -> Result<(), ClusterError> {
        let mut peers = self.peers.write().await;

        if peers.contains_key(&id) {
            return Err(ClusterError::PeerAlreadyExists(id));
        }

        peers.insert(
            id.clone(),
            PeerConnection {
                id,
                address,
                state: PeerConnectionState::Disconnected,
                last_seen: Instant::now(),
                pending_messages: 0,
            },
        );

        Ok(())
    }

    /// Remove a peer
    pub async fn remove_peer(&self, id: &ControllerId) -> Result<(), ClusterError> {
        let mut peers = self.peers.write().await;
        let mut peer_info = self.peer_info.write().await;

        if peers.remove(id).is_none() {
            return Err(ClusterError::PeerNotFound(id.clone()));
        }

        peer_info.remove(id);
        Ok(())
    }

    /// Update peer info
    pub async fn update_peer_info(&self, id: ControllerId, info: ControllerInfo) {
        let mut peer_info = self.peer_info.write().await;
        peer_info.insert(id, info);
    }

    /// Mark peer as connected
    pub async fn mark_peer_connected(&self, id: &ControllerId) {
        let mut peers = self.peers.write().await;
        let mut connected_address = None;
        if let Some(peer) = peers.get_mut(id) {
            peer.state = PeerConnectionState::Connected;
            peer.last_seen = Instant::now();
            connected_address = Some(peer.address);
        }

        if let Some(address) = connected_address {
            let mut peer_info = self.peer_info.write().await;
            let info = peer_info
                .entry(id.clone())
                .or_insert_with(|| ControllerInfo::new(id.clone(), address));
            info.address = address;
            info.health = ControllerHealth::Healthy;
            info.last_heartbeat = Some(Instant::now());
        }
    }

    /// Mark peer as disconnected
    pub async fn mark_peer_disconnected(&self, id: &ControllerId, reason: String) {
        let mut peers = self.peers.write().await;
        let mut address = None;
        if let Some(peer) = peers.get_mut(id) {
            peer.state = PeerConnectionState::Failed { reason };
            address = Some(peer.address);
        }

        if let Some(address) = address {
            let mut peer_info = self.peer_info.write().await;
            let info = peer_info
                .entry(id.clone())
                .or_insert_with(|| ControllerInfo::new(id.clone(), address));
            info.address = address;
            info.health = ControllerHealth::Down;
        }
    }
}

/// Cluster errors
#[derive(Debug, thiserror::Error)]
pub enum ClusterError {
    #[error("Cluster manager already running")]
    AlreadyRunning,
    #[error("Peer not found: {0}")]
    PeerNotFound(ControllerId),
    #[error("Peer not connected: {0}")]
    PeerNotConnected(ControllerId),
    #[error("Peer already exists: {0}")]
    PeerAlreadyExists(ControllerId),
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Communication timeout")]
    Timeout,
    #[error("Channel closed")]
    ChannelClosed,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    fn test_config() -> ClusterConfig {
        ClusterConfig {
            cluster_id: "test".to_string(),
            controller_id: ControllerId::new("test-ctrl"),
            bind_address: "127.0.0.1:9000".parse().unwrap(),
            peers: vec!["127.0.0.1:9001".parse().unwrap()],
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
            region: None,
            capacity: 500,
        }
    }

    #[tokio::test]
    async fn test_cluster_manager_creation() {
        let config = test_config();
        let manager = ClusterManager::new(config).await.unwrap();

        let peers = manager.peers.read().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers.get(&ControllerId::new("peer-0")).unwrap().state,
            PeerConnectionState::Disconnected
        );
    }

    #[tokio::test]
    async fn test_add_remove_peer() {
        let config = test_config();
        let manager = ClusterManager::new(config).await.unwrap();

        let peer_id = ControllerId::new("new-peer");
        let addr: SocketAddr = "127.0.0.1:9002".parse().unwrap();

        manager.add_peer(peer_id.clone(), addr).await.unwrap();

        {
            let peers = manager.peers.read().await;
            assert!(peers.contains_key(&peer_id));
        }

        manager.remove_peer(&peer_id).await.unwrap();

        {
            let peers = manager.peers.read().await;
            assert!(!peers.contains_key(&peer_id));
        }
    }

    #[tokio::test]
    async fn test_start_initializes_connectivity_and_health_state() {
        let config = test_config();
        let manager = ClusterManager::new(config.clone()).await.unwrap();
        let peer_id = ControllerId::new("peer-0");

        manager.start().await.unwrap();

        assert_eq!(
            manager.peer_state(&peer_id).await,
            Some(PeerConnectionState::Connected)
        );

        let peers = manager.connected_peers().await;
        let peer = peers.get(&peer_id).unwrap();
        assert_eq!(peer.health, ControllerHealth::Healthy);

        let state = manager.cluster_state().await;
        assert_eq!(state.leader, Some(config.controller_id.clone()));
        assert!(state.healthy);

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_message_drain_loop_updates_leader_from_heartbeat() {
        let manager = ClusterManager::new(test_config()).await.unwrap();
        manager.start().await.unwrap();

        let announced_leader = ControllerId::new("leader-1");
        manager
            .broadcast_heartbeat(HeartbeatRequest {
                term: 7,
                leader_id: announced_leader.clone(),
            })
            .await
            .unwrap();

        timeout(Duration::from_millis(500), async {
            loop {
                let leader = manager.cluster_state().await.leader;
                if leader.as_ref() == Some(&announced_leader) {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("leader update from heartbeat should be observed");

        let pending_total: usize = manager
            .peers
            .read()
            .await
            .values()
            .map(|peer| peer.pending_messages)
            .sum();
        assert_eq!(pending_total, 0);

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_cluster_state_reports_pending_message_load() {
        let config = test_config();
        let manager = ClusterManager::new(config.clone()).await.unwrap();
        let peer_id = ControllerId::new("peer-0");
        manager.start().await.unwrap();

        {
            let mut peers = manager.peers.write().await;
            peers.get_mut(&peer_id).unwrap().pending_messages = 4;
        }

        let state = manager.cluster_state().await;
        assert_eq!(state.total_capacity, (config.capacity as usize) * 2);
        assert_eq!(state.total_load, 4);
        assert_eq!(state.leader, Some(config.controller_id));

        manager.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_health_loop_marks_failed_peers_down() {
        let manager = ClusterManager::new(test_config()).await.unwrap();
        let peer_id = ControllerId::new("peer-0");
        manager.start().await.unwrap();

        {
            let mut peers = manager.peers.write().await;
            peers.get_mut(&peer_id).unwrap().state = PeerConnectionState::Failed {
                reason: "injected failure".to_string(),
            };
        }

        {
            let mut peer_info = manager.peer_info.write().await;
            peer_info.get_mut(&peer_id).unwrap().health = ControllerHealth::Healthy;
        }

        timeout(Duration::from_millis(500), async {
            loop {
                let peers = manager.connected_peers().await;
                if peers
                    .get(&peer_id)
                    .map(|info| info.health == ControllerHealth::Down)
                    .unwrap_or(false)
                {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("health loop should reconcile failed peer to down");

        manager.stop().await.unwrap();
    }
}
