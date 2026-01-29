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
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};

/// Cluster manager for peer communication
pub struct ClusterManager {
    /// Configuration
    config: ClusterConfig,
    /// Connected peers
    peers: RwLock<HashMap<ControllerId, PeerConnection>>,
    /// Peer info cache
    peer_info: RwLock<HashMap<ControllerId, ControllerInfo>>,
    /// Message sender for outgoing messages
    message_tx: mpsc::Sender<OutgoingMessage>,
    /// Running state
    running: RwLock<bool>,
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
            let peer_id = ControllerId::new(&format!("peer-{}", i));
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
            peers: RwLock::new(peers),
            peer_info: RwLock::new(HashMap::new()),
            message_tx,
            running: RwLock::new(false),
        })
    }

    /// Start the cluster manager
    pub async fn start(&self) -> Result<(), ClusterError> {
        let mut running = self.running.write().await;
        if *running {
            return Err(ClusterError::AlreadyRunning);
        }
        *running = true;

        tracing::info!(
            "Cluster manager starting for controller {}",
            self.config.controller_id
        );

        // TODO: Start connection loops for each peer
        // TODO: Start message processing loop
        // TODO: Start health check loop

        Ok(())
    }

    /// Stop the cluster manager
    pub async fn stop(&self) -> Result<(), ClusterError> {
        let mut running = self.running.write().await;
        if !*running {
            return Ok(());
        }
        *running = false;

        tracing::info!(
            "Cluster manager stopping for controller {}",
            self.config.controller_id
        );

        // Close all peer connections
        let mut peers = self.peers.write().await;
        for peer in peers.values_mut() {
            peer.state = PeerConnectionState::Disconnected;
        }

        Ok(())
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
        let peers = self.peers.read().await;
        for peer_id in peers.keys() {
            self.send_message(peer_id, PeerMessage::VoteRequest(request.clone()))
                .await?;
        }
        Ok(())
    }

    /// Send a heartbeat to all peers
    pub async fn broadcast_heartbeat(&self, request: HeartbeatRequest) -> Result<(), ClusterError> {
        let peers = self.peers.read().await;
        for peer_id in peers.keys() {
            self.send_message(peer_id, PeerMessage::Heartbeat(request.clone()))
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
            let peers = self.peers.read().await;
            match peers.get(target) {
                Some(peer) if peer.state == PeerConnectionState::Connected => {}
                Some(_) => return Err(ClusterError::PeerNotConnected(target.clone())),
                None => return Err(ClusterError::PeerNotFound(target.clone())),
            }
        }

        // Queue message for sending
        self.message_tx
            .send(OutgoingMessage {
                target: target.clone(),
                message,
                response_tx: None,
            })
            .await
            .map_err(|_| ClusterError::ChannelClosed)?;

        Ok(())
    }

    /// Get cluster state snapshot
    pub async fn cluster_state(&self) -> ClusterState {
        let peer_info = self.peer_info.read().await;

        let mut total_capacity: usize = self.config.capacity as usize;

        for info in peer_info.values() {
            total_capacity += info.capacity as usize;
        }

        let healthy_count = peer_info
            .values()
            .filter(|i| matches!(i.health, ControllerHealth::Healthy))
            .count();

        let quorum_size = (self.config.peers.len() + 1) / 2 + 1;

        ClusterState {
            controllers: peer_info.clone(),
            leader: None,                              // TODO: Get from Raft state
            healthy: healthy_count + 1 >= quorum_size, // +1 for self
            total_capacity,
            total_load: 0, // TODO: Aggregate from peer load reports
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
        if let Some(peer) = peers.get_mut(id) {
            peer.state = PeerConnectionState::Connected;
            peer.last_seen = Instant::now();
        }
    }

    /// Mark peer as disconnected
    pub async fn mark_peer_disconnected(&self, id: &ControllerId, reason: String) {
        let mut peers = self.peers.write().await;
        if let Some(peer) = peers.get_mut(id) {
            peer.state = PeerConnectionState::Failed { reason };
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
    async fn test_cluster_state() {
        let config = test_config();
        let manager = ClusterManager::new(config.clone()).await.unwrap();

        let state = manager.cluster_state().await;
        assert_eq!(state.total_capacity, config.capacity as usize);
    }
}
