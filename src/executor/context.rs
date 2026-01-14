use std::sync::Arc;

use crate::connection::{Connection, ConnectionBuilder};

use super::{Executor, ExecutorError, ExecutorResult};

impl Executor {
    pub(super) async fn close_connections(&self) {
        let connections: Vec<_> = {
            let mut cache = self.connection_cache.write().await;
            cache.drain().map(|(_, v)| v).collect()
        };

        for conn in connections {
            let _ = conn.close().await;
        }
    }

    pub(super) async fn get_connection_for_host(
        &self,
        host: &str,
    ) -> ExecutorResult<Arc<dyn Connection + Send + Sync>> {
        let (cache_key, builder) = {
            let runtime = self.runtime.read().await;

            let ansible_host = runtime
                .get_var("ansible_host", Some(host))
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| host.to_string());

            let ansible_connection = runtime
                .get_var("ansible_connection", Some(host))
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_else(|| {
                    if ansible_host == "localhost" || ansible_host == "127.0.0.1" {
                        "local".to_string()
                    } else {
                        "ssh".to_string()
                    }
                });

            let ansible_user = runtime
                .get_var("ansible_user", Some(host))
                .and_then(|v| v.as_str().map(str::to_string));

            let ansible_port = runtime.get_var("ansible_port", Some(host)).and_then(|v| {
                v.as_u64()
                    .and_then(|p| u16::try_from(p).ok())
                    .or_else(|| v.as_str().and_then(|s| s.parse::<u16>().ok()))
            });

            let private_key = runtime
                .get_var("ansible_ssh_private_key_file", Some(host))
                .and_then(|v| v.as_str().map(str::to_string))
                .map(|p| shellexpand::tilde(&p).to_string());

            let password = runtime
                .get_var("ansible_ssh_pass", Some(host))
                .and_then(|v| v.as_str().map(str::to_string));

            let timeout = runtime
                .get_var("ansible_ssh_timeout", Some(host))
                .and_then(|v| v.as_u64());

            let conn_type = match ansible_connection.as_str() {
                "local" => "local",
                "docker" | "podman" => "docker",
                "ssh" => "ssh",
                other => {
                    return Err(ExecutorError::RuntimeError(format!(
                        "Unsupported connection type '{}' for host '{}'",
                        other, host
                    )));
                }
            };

            let cache_key = format!(
                "{}:{}:{}:{}:{}:{}",
                conn_type,
                ansible_host,
                ansible_port.unwrap_or(22),
                ansible_user.clone().unwrap_or_else(|| "root".to_string()),
                private_key.clone().unwrap_or_default(),
                password.is_some()
            );

            let mut builder = ConnectionBuilder::new(ansible_host);
            builder = builder.connection_type(conn_type);
            if let Some(port) = ansible_port {
                builder = builder.port(port);
            }
            if let Some(user) = ansible_user {
                builder = builder.user(user);
            }
            if let Some(key) = private_key {
                builder = builder.private_key(key);
            }
            if let Some(pass) = password {
                builder = builder.password(pass);
            }
            if let Some(t) = timeout {
                builder = builder.timeout(t);
            }

            (cache_key, builder)
        };

        {
            let cache = self.connection_cache.read().await;
            if let Some(conn) = cache.get(&cache_key) {
                if conn.is_alive().await {
                    return Ok(Arc::clone(conn));
                }
            }
        }

        {
            let mut cache = self.connection_cache.write().await;
            cache.remove(&cache_key);
        }

        builder
            .connect()
            .await
            .map_err(|e| ExecutorError::HostUnreachable(format!("{}: {}", host, e)))
    }

    pub(super) async fn get_python_interpreter(&self, host: &str) -> String {
        let runtime = self.runtime.read().await;
        runtime
            .get_var("ansible_python_interpreter", Some(host))
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "/usr/bin/python3".to_string())
    }

    /// Resolve host pattern to list of hosts
    pub(super) async fn resolve_hosts(&self, pattern: &str) -> ExecutorResult<Vec<String>> {
        let runtime = self.runtime.read().await;

        // Handle special patterns
        if pattern == "all" {
            return Ok(runtime.get_all_hosts());
        }

        if pattern == "localhost" {
            return Ok(vec!["localhost".to_string()]);
        }

        // Check for group name
        if let Some(hosts) = runtime.get_group_hosts(pattern) {
            return Ok(hosts);
        }

        // Check for regex pattern (starts with ~)
        if let Some(regex_pattern) = pattern.strip_prefix('~') {
            let re = regex::Regex::new(regex_pattern)
                .map_err(|e| ExecutorError::ParseError(format!("Invalid regex: {}", e)))?;

            let all_hosts = runtime.get_all_hosts();
            let matched: Vec<_> = all_hosts.into_iter().filter(|h| re.is_match(h)).collect();

            return Ok(matched);
        }

        // Treat as single host or comma-separated list
        let hosts: Vec<String> = pattern.split(',').map(|s| s.trim().to_string()).collect();
        Ok(hosts)
    }
}
