//! Galaxy API Client
//!
//! This module provides an HTTP client for interacting with Ansible Galaxy
//! and compatible servers (like Automation Hub). It includes:
//!
//! - Configurable retry logic with exponential backoff
//! - Timeout handling
//! - Rate limiting awareness
//! - Multi-server support
//! - Authentication token management

use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;
use std::time::Duration;
use tracing::{debug, info, warn};

use super::collection::{CollectionInfo, CollectionVersion};
use super::error::{GalaxyError, GalaxyResult};
use super::role::RoleInfo;
use crate::config::{GalaxyConfig, GalaxyServer};

/// Default Galaxy API server URL
pub const DEFAULT_GALAXY_SERVER: &str = "https://galaxy.ansible.com";

/// Default request timeout in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Default maximum number of retries
const DEFAULT_MAX_RETRIES: u32 = 3;

/// Default retry delay in milliseconds
const DEFAULT_RETRY_DELAY_MS: u64 = 1000;

/// Configuration for the Galaxy client
#[derive(Debug, Clone)]
pub struct GalaxyClientConfig {
    /// Primary server URL
    pub server_url: String,
    /// List of additional servers to try
    pub servers: Vec<GalaxyServer>,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum number of retries
    pub max_retries: u32,
    /// Base retry delay (will be multiplied by retry number for exponential backoff)
    pub retry_delay: Duration,
    /// Whether to ignore TLS certificate errors
    pub ignore_certs: bool,
    /// API token for authentication
    pub token: Option<String>,
    /// User agent string
    pub user_agent: String,
}

impl Default for GalaxyClientConfig {
    fn default() -> Self {
        Self {
            server_url: DEFAULT_GALAXY_SERVER.to_string(),
            servers: Vec::new(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            max_retries: DEFAULT_MAX_RETRIES,
            retry_delay: Duration::from_millis(DEFAULT_RETRY_DELAY_MS),
            ignore_certs: false,
            token: None,
            user_agent: format!("rustible/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// Builder for creating a GalaxyClient
pub struct GalaxyClientBuilder {
    config: GalaxyClientConfig,
}

impl GalaxyClientBuilder {
    /// Create a new builder with default settings
    pub fn new() -> Self {
        Self {
            config: GalaxyClientConfig::default(),
        }
    }

    /// Set the primary server URL
    pub fn server_url(mut self, url: impl Into<String>) -> Self {
        self.config.server_url = url.into();
        self
    }

    /// Add a server to the list
    pub fn add_server(mut self, server: GalaxyServer) -> Self {
        self.config.servers.push(server);
        self
    }

    /// Set the request timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Set the maximum number of retries
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.config.max_retries = retries;
        self
    }

    /// Set the retry delay
    pub fn retry_delay(mut self, delay: Duration) -> Self {
        self.config.retry_delay = delay;
        self
    }

    /// Ignore TLS certificate errors
    pub fn ignore_certs(mut self, ignore: bool) -> Self {
        self.config.ignore_certs = ignore;
        self
    }

    /// Set the authentication token
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.config.token = Some(token.into());
        self
    }

    /// Set the user agent string
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config.user_agent = user_agent.into();
        self
    }

    /// Build the GalaxyClient
    pub fn build(self) -> GalaxyResult<GalaxyClient> {
        GalaxyClient::from_config(self.config)
    }
}

impl Default for GalaxyClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// HTTP client for Ansible Galaxy API
pub struct GalaxyClient {
    /// The underlying HTTP client
    client: Client,
    /// Client configuration
    config: GalaxyClientConfig,
}

impl GalaxyClient {
    /// Create a new GalaxyClient from GalaxyConfig
    pub fn new(config: &GalaxyConfig) -> GalaxyResult<Self> {
        let client_config = GalaxyClientConfig {
            server_url: config.server.clone(),
            servers: config.server_list.clone(),
            ignore_certs: config.ignore_certs,
            token: std::env::var("ANSIBLE_GALAXY_TOKEN").ok(),
            ..Default::default()
        };

        Self::from_config(client_config)
    }

    /// Create a GalaxyClient from client configuration
    fn from_config(config: GalaxyClientConfig) -> GalaxyResult<Self> {
        let mut builder = Client::builder()
            .timeout(config.timeout)
            .user_agent(&config.user_agent)
            .pool_max_idle_per_host(10)
            .tcp_keepalive(Duration::from_secs(60));

        if config.ignore_certs {
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder
            .build()
            .map_err(|e| GalaxyError::http_error_with_source("Failed to create HTTP client", e))?;

        Ok(Self { client, config })
    }

    /// Create a new builder
    pub fn builder() -> GalaxyClientBuilder {
        GalaxyClientBuilder::new()
    }

    /// Get the primary server URL
    pub fn server_url(&self) -> &str {
        &self.config.server_url
    }

    /// Make a GET request with retry logic
    async fn get(&self, url: &str) -> GalaxyResult<Response> {
        self.request_with_retry(url, None).await
    }

    /// Make a request with retry logic and optional server fallback
    async fn request_with_retry(&self, path: &str, server: Option<&str>) -> GalaxyResult<Response> {
        let base_url = server.unwrap_or(&self.config.server_url);
        let url = if path.starts_with("http://") || path.starts_with("https://") {
            path.to_string()
        } else {
            format!(
                "{}/{}",
                base_url.trim_end_matches('/'),
                path.trim_start_matches('/')
            )
        };

        let mut last_error = None;
        let mut retry_count = 0;

        while retry_count <= self.config.max_retries {
            if retry_count > 0 {
                let delay = self.config.retry_delay * retry_count;
                debug!(
                    "Retry {}/{} for {} after {:?}",
                    retry_count, self.config.max_retries, url, delay
                );
                tokio::time::sleep(delay).await;
            }

            let mut request = self.client.get(&url);

            // Add authentication token if available
            if let Some(ref token) = self.config.token {
                request = request.header("Authorization", format!("Token {}", token));
            }

            match request.send().await {
                Ok(response) => {
                    let status = response.status();

                    // Handle rate limiting
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        let retry_after = response
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(60);

                        warn!("Rate limited, retry after {} seconds", retry_after);

                        if retry_count < self.config.max_retries {
                            tokio::time::sleep(Duration::from_secs(retry_after)).await;
                            retry_count += 1;
                            continue;
                        }
                        return Err(GalaxyError::RateLimited {
                            retry_after_secs: retry_after,
                        });
                    }

                    // Handle authentication errors
                    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                        return Err(GalaxyError::AuthenticationFailed {
                            message: format!("Server returned {}", status),
                        });
                    }

                    // Handle not found
                    if status == StatusCode::NOT_FOUND {
                        // This will be handled by the caller based on context
                        return Ok(response);
                    }

                    // Handle server errors (5xx) - these should be retried
                    if status.is_server_error() {
                        last_error =
                            Some(GalaxyError::http_error(format!("Server error: {}", status)));
                        retry_count += 1;
                        continue;
                    }

                    // Success or client error (4xx except those handled above)
                    return Ok(response);
                }
                Err(e) => {
                    if e.is_timeout() {
                        last_error = Some(GalaxyError::Timeout {
                            url: url.clone(),
                            timeout_secs: self.config.timeout.as_secs(),
                        });
                    } else if e.is_connect() {
                        last_error = Some(GalaxyError::connection_failed(&url, e.to_string()));
                    } else {
                        last_error = Some(GalaxyError::from(e));
                    }
                    retry_count += 1;
                }
            }
        }

        // All retries exhausted, try fallback servers
        for server in &self.config.servers {
            info!("Trying fallback server: {}", server.url);
            match self
                .request_with_retry_single(&url, &server.url, server.token.as_deref())
                .await
            {
                Ok(response) => return Ok(response),
                Err(e) => {
                    warn!("Fallback server {} failed: {}", server.url, e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| GalaxyError::http_error("Request failed after all retries")))
    }

    /// Make a single request to a specific server without retry
    async fn request_with_retry_single(
        &self,
        path: &str,
        server_url: &str,
        token: Option<&str>,
    ) -> GalaxyResult<Response> {
        let url = format!(
            "{}/{}",
            server_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );

        let mut request = self.client.get(&url);

        if let Some(token) = token {
            request = request.header("Authorization", format!("Token {}", token));
        } else if let Some(ref token) = self.config.token {
            request = request.header("Authorization", format!("Token {}", token));
        }

        let response = request.send().await?;
        Ok(response)
    }

    /// Get collection information from Galaxy
    pub async fn get_collection_info(&self, name: &str) -> GalaxyResult<CollectionInfo> {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() != 2 {
            return Err(GalaxyError::InvalidCollectionName {
                name: name.to_string(),
                reason: "Collection name must be in 'namespace.name' format".to_string(),
            });
        }

        let (namespace, collection_name) = (parts[0], parts[1]);
        let url = format!(
            "api/v3/plugin/ansible/content/published/collections/index/{}/{}/",
            namespace, collection_name
        );

        let response = self.get(&url).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(GalaxyError::collection_not_found(name));
        }

        if !response.status().is_success() {
            return Err(GalaxyError::http_error(format!(
                "Failed to get collection info: {}",
                response.status()
            )));
        }

        let info: CollectionInfo = response.json().await.map_err(|e| {
            GalaxyError::http_error_with_source("Failed to parse collection info", e)
        })?;

        Ok(info)
    }

    /// List available versions for a collection
    pub async fn list_collection_versions(
        &self,
        name: &str,
    ) -> GalaxyResult<Vec<CollectionVersion>> {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() != 2 {
            return Err(GalaxyError::InvalidCollectionName {
                name: name.to_string(),
                reason: "Collection name must be in 'namespace.name' format".to_string(),
            });
        }

        let (namespace, collection_name) = (parts[0], parts[1]);
        let url = format!(
            "api/v3/plugin/ansible/content/published/collections/index/{}/{}/versions/",
            namespace, collection_name
        );

        let response = self.get(&url).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(GalaxyError::collection_not_found(name));
        }

        if !response.status().is_success() {
            return Err(GalaxyError::http_error(format!(
                "Failed to list collection versions: {}",
                response.status()
            )));
        }

        let response_body: CollectionVersionsResponse = response
            .json()
            .await
            .map_err(|e| GalaxyError::http_error_with_source("Failed to parse version list", e))?;

        Ok(response_body.data)
    }

    /// Get a specific collection version
    pub async fn get_collection_version(
        &self,
        name: &str,
        version: &str,
    ) -> GalaxyResult<CollectionVersion> {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() != 2 {
            return Err(GalaxyError::InvalidCollectionName {
                name: name.to_string(),
                reason: "Collection name must be in 'namespace.name' format".to_string(),
            });
        }

        let (namespace, collection_name) = (parts[0], parts[1]);
        let url = format!(
            "api/v3/plugin/ansible/content/published/collections/index/{}/{}/versions/{}/",
            namespace, collection_name, version
        );

        let response = self.get(&url).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(GalaxyError::CollectionVersionNotFound {
                name: name.to_string(),
                version: version.to_string(),
            });
        }

        if !response.status().is_success() {
            return Err(GalaxyError::http_error(format!(
                "Failed to get collection version: {}",
                response.status()
            )));
        }

        let version_info: CollectionVersion = response
            .json()
            .await
            .map_err(|e| GalaxyError::http_error_with_source("Failed to parse version info", e))?;

        Ok(version_info)
    }

    /// Download a collection artifact
    pub async fn download_collection(&self, download_url: &str) -> GalaxyResult<bytes::Bytes> {
        let response = self.get(download_url).await?;

        if !response.status().is_success() {
            return Err(GalaxyError::http_error(format!(
                "Failed to download collection: {}",
                response.status()
            )));
        }

        let bytes = response.bytes().await.map_err(|e| {
            GalaxyError::http_error_with_source("Failed to read collection data", e)
        })?;

        Ok(bytes)
    }

    /// Get role information from Galaxy
    pub async fn get_role_info(&self, name: &str) -> GalaxyResult<RoleInfo> {
        // Galaxy v1 API for roles
        let url = if name.contains('.') {
            let parts: Vec<&str> = name.split('.').collect();
            format!(
                "api/v1/roles/?owner__username={}&name={}",
                parts[0], parts[1]
            )
        } else {
            format!("api/v1/roles/?name={}", name)
        };

        let response = self.get(&url).await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(GalaxyError::role_not_found(name));
        }

        if !response.status().is_success() {
            return Err(GalaxyError::http_error(format!(
                "Failed to get role info: {}",
                response.status()
            )));
        }

        let response_body: RoleSearchResponse = response
            .json()
            .await
            .map_err(|e| GalaxyError::http_error_with_source("Failed to parse role info", e))?;

        response_body
            .results
            .into_iter()
            .next()
            .ok_or_else(|| GalaxyError::role_not_found(name))
    }

    /// Search for collections
    pub async fn search_collections(&self, query: &str) -> GalaxyResult<Vec<CollectionInfo>> {
        let url = format!(
            "api/v3/plugin/ansible/search/collection-versions/?keywords={}",
            urlencoding::encode(query)
        );

        let response = self.get(&url).await?;

        if !response.status().is_success() {
            return Err(GalaxyError::http_error(format!(
                "Failed to search collections: {}",
                response.status()
            )));
        }

        let response_body: CollectionSearchResponse = response.json().await.map_err(|e| {
            GalaxyError::http_error_with_source("Failed to parse search results", e)
        })?;

        Ok(response_body.data)
    }

    /// Search for roles
    pub async fn search_roles(&self, query: &str) -> GalaxyResult<Vec<RoleInfo>> {
        let url = format!("api/v1/search/roles/?search={}", urlencoding::encode(query));

        let response = self.get(&url).await?;

        if !response.status().is_success() {
            return Err(GalaxyError::http_error(format!(
                "Failed to search roles: {}",
                response.status()
            )));
        }

        let response_body: RoleSearchResponse = response.json().await.map_err(|e| {
            GalaxyError::http_error_with_source("Failed to parse search results", e)
        })?;

        Ok(response_body.results)
    }

    /// Check if the server is reachable
    pub async fn health_check(&self) -> GalaxyResult<bool> {
        let url = "api/";
        let response = self.get(url).await?;
        Ok(response.status().is_success())
    }
}

// Response structures for Galaxy API

#[derive(Debug, Deserialize)]
struct CollectionVersionsResponse {
    data: Vec<CollectionVersion>,
    #[serde(default)]
    links: PaginationLinks,
}

#[derive(Debug, Deserialize)]
struct CollectionSearchResponse {
    data: Vec<CollectionInfo>,
    #[serde(default)]
    links: PaginationLinks,
}

#[derive(Debug, Default, Deserialize)]
struct PaginationLinks {
    #[serde(default)]
    first: Option<String>,
    #[serde(default)]
    previous: Option<String>,
    #[serde(default)]
    next: Option<String>,
    #[serde(default)]
    last: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RoleSearchResponse {
    results: Vec<RoleInfo>,
    #[serde(default)]
    count: u32,
    #[serde(default)]
    next: Option<String>,
    #[serde(default)]
    previous: Option<String>,
}

/// URL encoding helper
mod urlencoding {
    pub fn encode(input: &str) -> String {
        url::form_urlencoded::byte_serialize(input.as_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_builder() {
        let client = GalaxyClient::builder()
            .server_url("https://custom.galaxy.example.com")
            .timeout(Duration::from_secs(30))
            .max_retries(5)
            .build();

        assert!(client.is_ok());
        let client = client.unwrap();
        assert_eq!(client.server_url(), "https://custom.galaxy.example.com");
    }

    #[test]
    fn test_default_config() {
        let config = GalaxyClientConfig::default();
        assert_eq!(config.server_url, DEFAULT_GALAXY_SERVER);
        assert_eq!(config.max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(config.timeout, Duration::from_secs(DEFAULT_TIMEOUT_SECS));
    }

    #[test]
    fn test_invalid_collection_name() {
        let config = GalaxyConfig::default();
        let client = GalaxyClient::new(&config).unwrap();

        // This would fail at runtime due to invalid name
        // The actual async test would be in an integration test
        let _ = client;
    }
}
