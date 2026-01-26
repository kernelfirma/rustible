//! Raft consensus implementation for leader election
//!
//! This module implements a simplified Raft protocol focused on:
//! - Leader election with randomized timeouts
//! - Heartbeat-based leadership maintenance
//! - Term-based consistency
//!
//! Full log replication is not implemented - we use this primarily
//! for leader election in the distributed controller cluster.

use super::types::{ClusterConfig, ControllerId, ControllerRole};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;

/// Raft node state
#[derive(Debug)]
pub struct RaftState {
    /// Current term
    current_term: AtomicU64,
    /// Controller that received vote in current term
    voted_for: RwLock<Option<ControllerId>>,
    /// Current role
    role: RwLock<ControllerRole>,
    /// Known leader (if any)
    leader_id: RwLock<Option<ControllerId>>,
    /// Election timeout
    election_timeout: RwLock<Instant>,
    /// Last heartbeat received (for followers)
    last_heartbeat: RwLock<Instant>,
    /// Votes received in current election
    votes_received: RwLock<HashMap<ControllerId, bool>>,
    /// Cluster configuration
    config: ClusterConfig,
}

impl RaftState {
    /// Create new Raft state
    pub fn new(config: ClusterConfig) -> Self {
        let now = Instant::now();
        Self {
            current_term: AtomicU64::new(0),
            voted_for: RwLock::new(None),
            role: RwLock::new(ControllerRole::Follower),
            leader_id: RwLock::new(None),
            election_timeout: RwLock::new(now + config.random_election_timeout()),
            last_heartbeat: RwLock::new(now),
            votes_received: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Get current term
    pub fn current_term(&self) -> u64 {
        self.current_term.load(Ordering::SeqCst)
    }

    /// Get current role
    pub async fn role(&self) -> ControllerRole {
        *self.role.read().await
    }

    /// Get leader ID
    pub async fn leader_id(&self) -> Option<ControllerId> {
        self.leader_id.read().await.clone()
    }

    /// Check if this node is the leader
    pub async fn is_leader(&self) -> bool {
        *self.role.read().await == ControllerRole::Leader
    }

    /// Get this controller's ID
    pub fn controller_id(&self) -> &ControllerId {
        &self.config.controller_id
    }

    /// Transition to follower state
    pub async fn become_follower(&self, term: u64) {
        self.current_term.store(term, Ordering::SeqCst);
        *self.role.write().await = ControllerRole::Follower;
        *self.voted_for.write().await = None;
        self.reset_election_timeout().await;
    }

    /// Transition to candidate state
    pub async fn become_candidate(&self) {
        let new_term = self.current_term.fetch_add(1, Ordering::SeqCst) + 1;
        *self.role.write().await = ControllerRole::Candidate;
        *self.voted_for.write().await = Some(self.config.controller_id.clone());
        *self.votes_received.write().await = HashMap::new();

        // Vote for self
        self.votes_received
            .write()
            .await
            .insert(self.config.controller_id.clone(), true);

        self.reset_election_timeout().await;

        tracing::info!(
            "Controller {} became candidate for term {}",
            self.config.controller_id,
            new_term
        );
    }

    /// Transition to leader state
    pub async fn become_leader(&self) {
        *self.role.write().await = ControllerRole::Leader;
        *self.leader_id.write().await = Some(self.config.controller_id.clone());

        tracing::info!(
            "Controller {} became leader for term {}",
            self.config.controller_id,
            self.current_term()
        );
    }

    /// Reset election timeout with random jitter
    pub async fn reset_election_timeout(&self) {
        let timeout = self.config.random_election_timeout();
        *self.election_timeout.write().await = Instant::now() + timeout;
    }

    /// Check if election timeout has elapsed
    pub async fn election_timeout_elapsed(&self) -> bool {
        Instant::now() >= *self.election_timeout.read().await
    }

    /// Update last heartbeat time
    pub async fn update_heartbeat(&self) {
        *self.last_heartbeat.write().await = Instant::now();
        self.reset_election_timeout().await;
    }

    /// Get quorum size (majority of cluster)
    pub fn quorum_size(&self) -> usize {
        (self.config.peers.len() + 1) / 2 + 1
    }

    /// Record a vote
    pub async fn record_vote(&self, from: ControllerId, granted: bool) {
        self.votes_received.write().await.insert(from, granted);
    }

    /// Check if we have enough votes to become leader
    pub async fn has_quorum(&self) -> bool {
        let votes = self.votes_received.read().await;
        let granted = votes.values().filter(|&&v| v).count();
        granted >= self.quorum_size()
    }

    /// Handle vote request
    pub async fn handle_vote_request(
        &self,
        candidate_id: &ControllerId,
        candidate_term: u64,
    ) -> VoteResponse {
        let current_term = self.current_term();

        // If candidate's term is outdated, reject
        if candidate_term < current_term {
            return VoteResponse {
                term: current_term,
                vote_granted: false,
            };
        }

        // If candidate's term is newer, step down
        if candidate_term > current_term {
            self.become_follower(candidate_term).await;
        }

        // Check if we can vote for this candidate
        let voted_for = self.voted_for.read().await;
        let can_vote = voted_for.is_none() || voted_for.as_ref() == Some(candidate_id);

        if can_vote {
            drop(voted_for);
            *self.voted_for.write().await = Some(candidate_id.clone());
            self.reset_election_timeout().await;

            tracing::debug!(
                "Controller {} granted vote to {} for term {}",
                self.config.controller_id,
                candidate_id,
                candidate_term
            );

            VoteResponse {
                term: self.current_term(),
                vote_granted: true,
            }
        } else {
            VoteResponse {
                term: self.current_term(),
                vote_granted: false,
            }
        }
    }

    /// Handle heartbeat from leader
    pub async fn handle_heartbeat(
        &self,
        leader_id: &ControllerId,
        leader_term: u64,
    ) -> HeartbeatResponse {
        let current_term = self.current_term();

        // If leader's term is outdated, reject
        if leader_term < current_term {
            return HeartbeatResponse {
                term: current_term,
                success: false,
            };
        }

        // If leader's term is newer or equal, accept
        if leader_term >= current_term {
            if leader_term > current_term {
                self.become_follower(leader_term).await;
            }
            *self.leader_id.write().await = Some(leader_id.clone());
            self.update_heartbeat().await;

            // If we were a candidate, step down
            if *self.role.read().await == ControllerRole::Candidate {
                *self.role.write().await = ControllerRole::Follower;
            }
        }

        HeartbeatResponse {
            term: self.current_term(),
            success: true,
        }
    }
}

/// Vote request message
#[derive(Debug, Clone)]
pub struct VoteRequest {
    /// Candidate's term
    pub term: u64,
    /// Candidate requesting vote
    pub candidate_id: ControllerId,
}

/// Vote response message
#[derive(Debug, Clone)]
pub struct VoteResponse {
    /// Current term for candidate to update itself
    pub term: u64,
    /// True if vote was granted
    pub vote_granted: bool,
}

/// Heartbeat/AppendEntries request
#[derive(Debug, Clone)]
pub struct HeartbeatRequest {
    /// Leader's term
    pub term: u64,
    /// Leader ID
    pub leader_id: ControllerId,
}

/// Heartbeat response
#[derive(Debug, Clone)]
pub struct HeartbeatResponse {
    /// Current term
    pub term: u64,
    /// Success flag
    pub success: bool,
}

/// Raft event for the state machine
#[derive(Debug)]
pub enum RaftEvent {
    /// Election timeout elapsed
    ElectionTimeout,
    /// Received vote request
    VoteRequest {
        request: VoteRequest,
        response_tx: tokio::sync::oneshot::Sender<VoteResponse>,
    },
    /// Received vote response
    VoteResponse {
        from: ControllerId,
        response: VoteResponse,
    },
    /// Received heartbeat
    Heartbeat {
        request: HeartbeatRequest,
        response_tx: tokio::sync::oneshot::Sender<HeartbeatResponse>,
    },
    /// Send heartbeats (leader only)
    SendHeartbeats,
    /// Shutdown
    Shutdown,
}

/// Raft node that runs the consensus protocol
pub struct RaftNode {
    /// Raft state
    state: Arc<RaftState>,
    /// Event sender
    event_tx: mpsc::Sender<RaftEvent>,
    /// Event receiver
    event_rx: Option<mpsc::Receiver<RaftEvent>>,
}

impl RaftNode {
    /// Create a new Raft node
    pub fn new(config: ClusterConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(100);
        let state = Arc::new(RaftState::new(config));

        Self {
            state,
            event_tx,
            event_rx: Some(event_rx),
        }
    }

    /// Get the Raft state
    pub fn state(&self) -> Arc<RaftState> {
        Arc::clone(&self.state)
    }

    /// Get event sender for external events
    pub fn event_sender(&self) -> mpsc::Sender<RaftEvent> {
        self.event_tx.clone()
    }

    /// Run the Raft state machine
    pub async fn run(mut self) -> Result<(), RaftError> {
        let mut event_rx = self.event_rx.take().ok_or(RaftError::AlreadyRunning)?;
        let state = Arc::clone(&self.state);
        let event_tx = self.event_tx.clone();

        // Spawn election timeout checker
        let state_clone = Arc::clone(&state);
        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(10));
            loop {
                interval.tick().await;
                if state_clone.election_timeout_elapsed().await {
                    let role = state_clone.role().await;
                    if role != ControllerRole::Leader {
                        let _ = event_tx_clone.send(RaftEvent::ElectionTimeout).await;
                    }
                }
            }
        });

        // Spawn heartbeat sender (for leader)
        let state_clone = Arc::clone(&state);
        let event_tx_clone = event_tx.clone();
        tokio::spawn(async move {
            let heartbeat_interval = state_clone.config.heartbeat_interval();
            let mut interval = interval(heartbeat_interval);
            loop {
                interval.tick().await;
                if state_clone.is_leader().await {
                    let _ = event_tx_clone.send(RaftEvent::SendHeartbeats).await;
                }
            }
        });

        // Main event loop
        while let Some(event) = event_rx.recv().await {
            match event {
                RaftEvent::ElectionTimeout => {
                    self.handle_election_timeout().await?;
                }
                RaftEvent::VoteRequest {
                    request,
                    response_tx,
                } => {
                    let response = state
                        .handle_vote_request(&request.candidate_id, request.term)
                        .await;
                    let _ = response_tx.send(response);
                }
                RaftEvent::VoteResponse { from, response } => {
                    self.handle_vote_response(from, response).await?;
                }
                RaftEvent::Heartbeat {
                    request,
                    response_tx,
                } => {
                    let response = state
                        .handle_heartbeat(&request.leader_id, request.term)
                        .await;
                    let _ = response_tx.send(response);
                }
                RaftEvent::SendHeartbeats => {
                    // In a real implementation, this would send heartbeats to all peers
                    // For now, we just log it
                    tracing::trace!(
                        "Leader {} sending heartbeats",
                        state.controller_id()
                    );
                }
                RaftEvent::Shutdown => {
                    tracing::info!("Raft node shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle election timeout
    async fn handle_election_timeout(&self) -> Result<(), RaftError> {
        let state = &self.state;

        // Become candidate
        state.become_candidate().await;

        // In a real implementation, we would send vote requests to all peers here
        // For now, if we're the only node, become leader immediately
        if state.config.peers.is_empty() {
            state.become_leader().await;
        }

        Ok(())
    }

    /// Handle vote response
    async fn handle_vote_response(
        &self,
        from: ControllerId,
        response: VoteResponse,
    ) -> Result<(), RaftError> {
        let state = &self.state;

        // Only process if we're still a candidate
        if state.role().await != ControllerRole::Candidate {
            return Ok(());
        }

        // If response term is higher, step down
        if response.term > state.current_term() {
            state.become_follower(response.term).await;
            return Ok(());
        }

        // Record the vote
        state.record_vote(from, response.vote_granted).await;

        // Check if we have quorum
        if response.vote_granted && state.has_quorum().await {
            state.become_leader().await;
        }

        Ok(())
    }
}

/// Raft errors
#[derive(Debug, thiserror::Error)]
pub enum RaftError {
    #[error("Raft node is already running")]
    AlreadyRunning,
    #[error("Communication error: {0}")]
    Communication(String),
    #[error("Invalid state transition")]
    InvalidStateTransition,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ClusterConfig {
        ClusterConfig {
            cluster_id: "test".to_string(),
            controller_id: ControllerId::new("test-ctrl"),
            bind_address: "127.0.0.1:9000".parse().unwrap(),
            peers: vec![],
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            heartbeat_interval_ms: 50,
            region: None,
            capacity: 500,
        }
    }

    #[tokio::test]
    async fn test_initial_state() {
        let config = test_config();
        let state = RaftState::new(config);

        assert_eq!(state.current_term(), 0);
        assert_eq!(state.role().await, ControllerRole::Follower);
        assert!(state.leader_id().await.is_none());
    }

    #[tokio::test]
    async fn test_become_candidate() {
        let config = test_config();
        let state = RaftState::new(config);

        state.become_candidate().await;

        assert_eq!(state.current_term(), 1);
        assert_eq!(state.role().await, ControllerRole::Candidate);
    }

    #[tokio::test]
    async fn test_become_leader() {
        let config = test_config();
        let state = RaftState::new(config.clone());

        state.become_candidate().await;
        state.become_leader().await;

        assert_eq!(state.role().await, ControllerRole::Leader);
        assert_eq!(state.leader_id().await, Some(config.controller_id));
    }

    #[tokio::test]
    async fn test_vote_request_handling() {
        let config = test_config();
        let state = RaftState::new(config);

        let candidate = ControllerId::new("candidate-1");
        let response = state.handle_vote_request(&candidate, 1).await;

        assert!(response.vote_granted);
        assert_eq!(response.term, 1);
    }

    #[tokio::test]
    async fn test_heartbeat_handling() {
        let config = test_config();
        let state = RaftState::new(config);

        let leader = ControllerId::new("leader-1");
        let response = state.handle_heartbeat(&leader, 1).await;

        assert!(response.success);
        assert_eq!(state.leader_id().await, Some(leader));
    }

    #[tokio::test]
    async fn test_step_down_on_higher_term() {
        let config = test_config();
        let state = RaftState::new(config);

        state.become_candidate().await;
        assert_eq!(state.role().await, ControllerRole::Candidate);

        let leader = ControllerId::new("leader-1");
        let _ = state.handle_heartbeat(&leader, 10).await;

        assert_eq!(state.role().await, ControllerRole::Follower);
        assert_eq!(state.current_term(), 10);
    }
}
