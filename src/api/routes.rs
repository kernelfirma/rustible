//! API route configuration.

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;

use super::handlers;
use super::state::AppState;
use super::websocket;

/// Create the main API router with all routes.
pub fn api_routes(state: Arc<AppState>) -> Router {
    Router::new()
        // Public routes (no auth required)
        .nest("/api/v1", public_routes())
        // Protected routes (auth required)
        .nest("/api/v1", protected_routes())
        // WebSocket routes
        .nest("/api/v1/ws", websocket_routes())
        // Add state
        .with_state(state)
}

/// Public routes that don't require authentication.
fn public_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Health check
        .route("/health", get(handlers::health_check))
        // API info
        .route("/", get(handlers::api_info))
        .route("/info", get(handlers::api_info))
        // Authentication
        .route("/auth/login", post(handlers::login))
}

/// Protected routes that require authentication.
fn protected_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Auth endpoints
        .route("/auth/refresh", post(handlers::refresh_token))
        .route("/auth/me", get(handlers::me))
        // Playbook endpoints
        .route("/playbooks", get(handlers::list_playbooks))
        .route("/playbooks/execute", post(handlers::execute_playbook))
        // Job endpoints
        .route("/jobs", get(handlers::list_jobs))
        .route("/jobs/:id", get(handlers::get_job))
        .route("/jobs/:id/output", get(handlers::get_job_output))
        .route("/jobs/:id/cancel", post(handlers::cancel_job))
        // Inventory endpoints
        .route("/inventory", get(handlers::get_inventory_summary))
        .route("/inventory/reload", post(handlers::reload_inventory))
        .route("/inventory/hosts", get(handlers::list_hosts))
        .route("/inventory/hosts/:name", get(handlers::get_host))
        .route("/inventory/groups", get(handlers::list_groups))
        .route("/inventory/groups/:name", get(handlers::get_group))
}

/// WebSocket routes for real-time updates.
fn websocket_routes() -> Router<Arc<AppState>> {
    Router::new().route("/jobs/:id", get(websocket::job_ws_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiConfig;

    #[test]
    fn test_router_creation() {
        let config = ApiConfig::default();
        let state = Arc::new(AppState::new(config));
        let _router = api_routes(state);
        // Just verify it doesn't panic
    }
}
