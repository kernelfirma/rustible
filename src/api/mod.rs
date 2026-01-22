//! REST API server for Rustible.
//!
//! This module provides a REST API for Rustible, enabling programmatic access
//! to playbook execution, inventory management, and job monitoring.
//!
//! # Features
//!
//! - **Playbook Execution**: Submit playbooks for execution via HTTP
//! - **Inventory Management**: Query hosts, groups, and variables
//! - **Job Management**: Monitor job status and history
//! - **Real-time Output**: WebSocket support for live execution output
//! - **Authentication**: JWT-based authentication
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! use rustible::prelude::*;
//! use rustible::api::{ApiServer, ApiConfig};
//!
//! #[tokio::main]
//! async fn main() {
//!     let config = ApiConfig::default();
//!     let server = ApiServer::new(config);
//!     server.run().await.unwrap();
//! }
//! ```

pub mod auth;
pub mod error;
pub mod handlers;
pub mod routes;
pub mod state;
pub mod types;
pub mod websocket;

// AWX/Tower API compatibility module (Issue #87)
pub mod awx;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;
use uuid::Uuid;

pub use auth::{AuthConfig, Claims, JwtAuth};
pub use error::{ApiError, ApiResult};
pub use state::AppState;
pub use types::*;

/// Configuration for the API server.
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// Address to bind the server to
    pub bind_address: SocketAddr,
    /// JWT secret key for authentication
    pub jwt_secret: String,
    /// JWT token expiration in seconds
    pub token_expiration_secs: u64,
    /// Whether to enable CORS
    pub enable_cors: bool,
    /// Maximum request body size in bytes
    pub max_body_size: usize,
    /// Path to inventory file/directory
    pub inventory_path: Option<String>,
    /// Playbook search paths
    pub playbook_paths: Vec<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:8080".parse().unwrap(),
            jwt_secret: Uuid::new_v4().to_string(),
            token_expiration_secs: 3600, // 1 hour
            enable_cors: true,
            max_body_size: 10 * 1024 * 1024, // 10MB
            inventory_path: None,
            playbook_paths: vec!["./playbooks".to_string()],
        }
    }
}

impl ApiConfig {
    /// Create a new API configuration with the specified bind address.
    pub fn with_address(mut self, addr: SocketAddr) -> Self {
        self.bind_address = addr;
        self
    }

    /// Set the JWT secret key.
    pub fn with_jwt_secret(mut self, secret: impl Into<String>) -> Self {
        self.jwt_secret = secret.into();
        self
    }

    /// Set the inventory path.
    pub fn with_inventory(mut self, path: impl Into<String>) -> Self {
        self.inventory_path = Some(path.into());
        self
    }

    /// Add a playbook search path.
    pub fn with_playbook_path(mut self, path: impl Into<String>) -> Self {
        self.playbook_paths.push(path.into());
        self
    }
}

/// The main API server.
pub struct ApiServer {
    config: ApiConfig,
    state: Arc<AppState>,
}

impl ApiServer {
    /// Create a new API server with the given configuration.
    pub fn new(config: ApiConfig) -> Self {
        let state = Arc::new(AppState::new(config.clone()));
        Self { config, state }
    }

    /// Create a new API server with existing state.
    pub fn with_state(config: ApiConfig, state: Arc<AppState>) -> Self {
        Self { config, state }
    }

    /// Build the router with all routes.
    pub fn router(&self) -> Router {
        let mut app = Router::new().merge(routes::api_routes(self.state.clone()));

        // Add CORS layer if enabled
        if self.config.enable_cors {
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any);
            app = app.layer(cors);
        }

        // Add tracing layer
        app = app.layer(TraceLayer::new_for_http());

        app
    }

    /// Run the API server.
    pub async fn run(self) -> Result<(), std::io::Error> {
        let addr = self.config.bind_address;
        let router = self.router();

        info!("Starting Rustible API server on {}", addr);

        let listener = TcpListener::bind(addr).await?;
        axum::serve(listener, router).await
    }

    /// Run the server with graceful shutdown support.
    pub async fn run_with_shutdown(
        self,
        shutdown: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> Result<(), std::io::Error> {
        let addr = self.config.bind_address;
        let router = self.router();

        info!("Starting Rustible API server on {}", addr);

        let listener = TcpListener::bind(addr).await?;
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown)
            .await
    }

    /// Get a reference to the application state.
    pub fn state(&self) -> Arc<AppState> {
        self.state.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_config_default() {
        let config = ApiConfig::default();
        assert_eq!(config.bind_address.port(), 8080);
        assert!(config.enable_cors);
    }

    #[test]
    fn test_api_config_builder() {
        let config = ApiConfig::default()
            .with_address("0.0.0.0:3000".parse().unwrap())
            .with_jwt_secret("my-secret")
            .with_inventory("/etc/rustible/inventory");

        assert_eq!(config.bind_address.port(), 3000);
        assert_eq!(config.jwt_secret, "my-secret");
        assert_eq!(
            config.inventory_path,
            Some("/etc/rustible/inventory".to_string())
        );
    }

    #[test]
    fn test_default_config_has_random_secret() {
        let config1 = ApiConfig::default();
        let config2 = ApiConfig::default();

        // Should not be the old hardcoded value
        assert_ne!(
            config1.jwt_secret,
            "rustible-secret-key-change-in-production"
        );
        // Should be random (different each time)
        assert_ne!(config1.jwt_secret, config2.jwt_secret);
    }
}
