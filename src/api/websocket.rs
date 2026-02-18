//! WebSocket handlers for real-time job output.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::error::ApiError;
use super::state::AppState;
use super::types::WsMessage;

/// Query parameters for WebSocket connection
#[derive(Deserialize)]
pub struct WsParams {
    token: String,
}

/// WebSocket handler for job output streaming.
pub async fn job_ws_handler(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<Uuid>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> Result<impl IntoResponse, ApiError> {
    // Validate token
    let claims = state
        .jwt_auth
        .validate_token(&params.token)
        .map_err(|e| ApiError::Unauthorized(format!("Invalid token: {}", e)))?;

    if claims.is_expired() {
        return Err(ApiError::Unauthorized("Token has expired".to_string()));
    }

    // Verify job exists
    let _job = state
        .get_job(job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Job not found: {}", job_id)))?;

    // Subscribe to job updates
    let rx = state
        .subscribe_to_job(job_id)
        .ok_or_else(|| ApiError::Internal("Failed to subscribe to job".to_string()))?;

    info!(
        "WebSocket connection for job {} from user {}",
        job_id, claims.sub
    );

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, job_id, rx, state)))
}

/// Handle WebSocket connection.
async fn handle_socket(
    socket: WebSocket,
    job_id: Uuid,
    mut rx: broadcast::Receiver<WsMessage>,
    state: Arc<AppState>,
) {
    let (mut sender, mut receiver) = socket.split();

    // Send existing output as history
    if let Some(job) = state.get_job(job_id) {
        for line in &job.output {
            let msg = WsMessage::Output {
                job_id,
                line: line.clone(),
                stream: "stdout".to_string(),
                timestamp: job.created_at,
            };

            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json)).await.is_err() {
                    warn!("Failed to send history to WebSocket");
                    return;
                }
            }
        }
    }

    // Spawn task to handle incoming messages (ping/pong)
    let _state_clone = state.clone();
    let send_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Receive broadcast messages
                result = rx.recv() => {
                    match result {
                        Ok(msg) => {
                            match serde_json::to_string(&msg) {
                                Ok(json) => {
                                    if sender.send(Message::Text(json)).await.is_err() {
                                        break;
                                    }

                                    // Check if job is complete
                                    if let WsMessage::StatusChange { status, .. } = &msg {
                                        use crate::api::types::JobStatus;
                                        if matches!(status, JobStatus::Success | JobStatus::Failed | JobStatus::Cancelled) {
                                            debug!("Job {} completed with status {:?}", job_id, status);
                                            // Send close frame after a short delay
                                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                            let _ = sender.close().await;
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to serialize WebSocket message: {}", e);
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!("WebSocket receiver lagged by {} messages", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            debug!("Broadcast channel closed for job {}", job_id);
                            break;
                        }
                    }
                }
            }
        }
    });

    // Handle incoming WebSocket messages
    let recv_task = tokio::spawn(async move {
        while let Some(result) = receiver.next().await {
            match result {
                Ok(Message::Text(text)) => {
                    // Try to parse as WsMessage
                    if let Ok(msg) = serde_json::from_str::<WsMessage>(&text) {
                        match msg {
                            WsMessage::Ping => {
                                debug!("Received ping from client");
                                // Pong is sent in the send task
                            }
                            _ => {
                                debug!("Received message: {:?}", msg);
                            }
                        }
                    }
                }
                Ok(Message::Ping(_data)) => {
                    debug!("Received WebSocket ping");
                    // Axum/tungstenite automatically sends Pong
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received WebSocket pong");
                }
                Ok(Message::Close(_)) => {
                    debug!("Client closed WebSocket connection");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    info!("WebSocket connection closed for job {}", job_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::JobStatus;

    #[test]
    fn test_ws_message_serialization() {
        let msg = WsMessage::Output {
            job_id: Uuid::new_v4(),
            line: "Hello, World!".to_string(),
            stream: "stdout".to_string(),
            timestamp: chrono::Utc::now(),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("output"));
        assert!(json.contains("Hello, World!"));

        let parsed: WsMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            WsMessage::Output { line, .. } => {
                assert_eq!(line, "Hello, World!");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_ws_status_change_serialization() {
        let msg = WsMessage::StatusChange {
            job_id: Uuid::new_v4(),
            status: JobStatus::Running,
            message: Some("Started".to_string()),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("status_change"));
        assert!(json.contains("running"));
    }
}
