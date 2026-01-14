use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use tracing::{debug, warn};

use super::config::{default_identity_files, expand_path, ConnectionConfig, HostConfig, RetryConfig};
use super::{ConnectionError, ConnectionResult};

#[derive(Debug, Clone)]
pub struct ResolvedConnectionParams {
    pub host_config: HostConfig,
    pub retry_config: RetryConfig,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub timeout: Duration,
    pub identifier: String,
}

pub fn resolve_connection_params(
    host: &str,
    port: u16,
    user: &str,
    host_config: Option<HostConfig>,
    global_config: &ConnectionConfig,
) -> ResolvedConnectionParams {
    let host_config = host_config.unwrap_or_else(|| global_config.get_host_merged(host));
    let retry_config = host_config.retry_config();

    let actual_host = host_config.hostname.as_deref().unwrap_or(host).to_string();
    let actual_port = host_config.port.unwrap_or(port);
    let actual_user = host_config.user.as_deref().unwrap_or(user).to_string();
    let timeout = host_config.timeout_duration();

    let identifier = format!("{}@{}:{}", actual_user, actual_host, actual_port);

    ResolvedConnectionParams {
        host_config,
        retry_config,
        host: actual_host,
        port: actual_port,
        user: actual_user,
        timeout,
        identifier,
    }
}

pub fn identity_file_candidates(
    host_config: &HostConfig,
    global_config: &ConnectionConfig,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();

    let mut push_unique = |path: PathBuf| {
        if seen.insert(path.clone()) {
            candidates.push(path);
        }
    };

    if let Some(identity_file) = &host_config.identity_file {
        push_unique(expand_path(identity_file));
    }

    for identity_file in &global_config.defaults.identity_files {
        push_unique(expand_path(identity_file));
    }

    for key_path in default_identity_files() {
        push_unique(key_path);
    }

    candidates
}

pub fn user_known_hosts_path(host_config: &HostConfig) -> Option<PathBuf> {
    host_config
        .user_known_hosts_file
        .as_ref()
        .map(PathBuf::from)
}

pub async fn connect_with_retry_async<T, F, Fut>(
    retry_config: &RetryConfig,
    operation: &str,
    mut attempt: F,
) -> ConnectionResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = ConnectionResult<T>>,
{
    let mut last_error = None;

    for attempt_idx in 0..=retry_config.max_retries {
        if attempt_idx > 0 {
            let delay = retry_config.delay_for_attempt(attempt_idx - 1);
            debug!(
                attempt = %attempt_idx,
                delay = ?delay,
                operation = %operation,
                "Retrying connection"
            );
            tokio::time::sleep(delay).await;
        }

        match attempt().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                warn!(
                    attempt = %attempt_idx,
                    error = %e,
                    operation = %operation,
                    "Connection attempt failed"
                );
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ConnectionError::ConnectionFailed("Unknown connection error".to_string())
    }))
}

pub fn connect_with_retry_blocking<T, F>(
    retry_config: &RetryConfig,
    operation: &str,
    mut attempt: F,
) -> ConnectionResult<T>
where
    F: FnMut() -> ConnectionResult<T>,
{
    let mut last_error = None;

    for attempt_idx in 0..=retry_config.max_retries {
        if attempt_idx > 0 {
            let delay = retry_config.delay_for_attempt(attempt_idx - 1);
            debug!(
                attempt = %attempt_idx,
                delay = ?delay,
                operation = %operation,
                "Retrying connection"
            );
            std::thread::sleep(delay);
        }

        match attempt() {
            Ok(result) => return Ok(result),
            Err(e) => {
                warn!(
                    attempt = %attempt_idx,
                    error = %e,
                    operation = %operation,
                    "Connection attempt failed"
                );
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        ConnectionError::ConnectionFailed("Unknown connection error".to_string())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_file_candidates_order_and_dedupe() {
        let mut host_config = HostConfig::default();
        host_config.identity_file = Some("/tmp/test_key".to_string());

        let mut config = ConnectionConfig::default();
        config.defaults.identity_files = vec![
            "/tmp/test_key".to_string(),
            "/tmp/other_key".to_string(),
        ];

        let candidates = identity_file_candidates(&host_config, &config);
        assert!(candidates.len() >= 2);

        let expected_first = expand_path("/tmp/test_key");
        let expected_second = expand_path("/tmp/other_key");

        assert_eq!(candidates[0], expected_first);
        assert_eq!(candidates[1], expected_second);

        let count = candidates.iter().filter(|p| **p == expected_first).count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_resolve_connection_params_overrides() {
        let mut host_config = HostConfig::default();
        host_config.hostname = Some("example.com".to_string());
        host_config.port = Some(2222);
        host_config.user = Some("admin".to_string());
        host_config.connect_timeout = Some(30);

        let config = ConnectionConfig::default();
        let resolved = resolve_connection_params("host", 22, "root", Some(host_config), &config);

        assert_eq!(resolved.host, "example.com");
        assert_eq!(resolved.port, 2222);
        assert_eq!(resolved.user, "admin");
        assert_eq!(resolved.timeout, Duration::from_secs(30));
        assert_eq!(resolved.identifier, "admin@example.com:2222");
    }
}
