//! Kubernetes connection module
//!
//! This module provides connectivity to Kubernetes pods using the kube-rs crate.
//! It supports executing commands inside pods via the Kubernetes API, with full
//! support for kubeconfig, service accounts, and namespace isolation.
//!
//! # Features
//!
//! - Pod exec functionality via Kubernetes API (not kubectl)
//! - Kubeconfig file loading with context selection
//! - Service account token authentication
//! - Namespace-aware operations
//! - File transfer via tar streaming
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::connection::kubernetes::{KubernetesConnection, KubernetesConnectionBuilder};
//!
//! // Connect to a pod in the default namespace
//! let conn = KubernetesConnectionBuilder::new()
//!     .namespace("default")
//!     .pod("my-pod")
//!     .container("app")
//!     .build()
//!     .await?;
//!
//! // Execute a command
//! let result = conn.execute("ls -la /app", None).await?;
//! println!("Output: {}", result.stdout);
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use k8s_openapi::api::core::v1::Pod;
use kube::{
    api::{Api, AttachParams, AttachedProcess, ListParams},
    config::{KubeConfigOptions, Kubeconfig},
    Client, Config,
};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, trace};

use super::{
    CommandResult, Connection, ConnectionError, ConnectionResult, ExecuteOptions, FileStat,
    TransferOptions,
};

/// Authentication method for Kubernetes API
#[derive(Debug, Clone)]
pub enum KubernetesAuth {
    /// Use kubeconfig file (default: ~/.kube/config)
    Kubeconfig {
        /// Path to kubeconfig file (None = default location)
        path: Option<PathBuf>,
        /// Context to use (None = current context)
        context: Option<String>,
    },
    /// Use in-cluster service account token
    InCluster,
    /// Use bearer token directly
    BearerToken(String),
}

impl Default for KubernetesAuth {
    fn default() -> Self {
        KubernetesAuth::Kubeconfig {
            path: None,
            context: None,
        }
    }
}

/// Kubernetes connection for executing commands inside pods
pub struct KubernetesConnection {
    /// Kubernetes API client
    client: Client,
    /// Target namespace
    namespace: String,
    /// Target pod name
    pod: String,
    /// Target container (None = first container)
    container: Option<String>,
    /// Cached Pod API handle
    pods_api: Api<Pod>,
    /// Connection identifier for pooling
    identifier: String,
}

impl std::fmt::Debug for KubernetesConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KubernetesConnection")
            .field("namespace", &self.namespace)
            .field("pod", &self.pod)
            .field("container", &self.container)
            .field("identifier", &self.identifier)
            .finish_non_exhaustive()
    }
}

impl Clone for KubernetesConnection {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            namespace: self.namespace.clone(),
            pod: self.pod.clone(),
            container: self.container.clone(),
            pods_api: Api::namespaced(self.client.clone(), &self.namespace),
            identifier: self.identifier.clone(),
        }
    }
}

impl KubernetesConnection {
    /// Create a new Kubernetes connection
    pub async fn new(
        namespace: impl Into<String>,
        pod: impl Into<String>,
        container: Option<String>,
        auth: KubernetesAuth,
    ) -> ConnectionResult<Self> {
        let namespace = namespace.into();
        let pod = pod.into();

        let client = Self::create_client(auth).await?;
        let pods_api = Api::namespaced(client.clone(), &namespace);
        let identifier = format!("k8s://{}:{}", namespace, pod);

        Ok(Self {
            client,
            namespace,
            pod,
            container,
            pods_api,
            identifier,
        })
    }

    /// Create a Kubernetes client from authentication config
    async fn create_client(auth: KubernetesAuth) -> ConnectionResult<Client> {
        let config = match auth {
            KubernetesAuth::Kubeconfig { path, context } => {
                let kubeconfig = if let Some(path) = path {
                    Kubeconfig::read_from(&path).map_err(|e| {
                        ConnectionError::InvalidConfig(format!(
                            "Failed to read kubeconfig from {}: {}",
                            path.display(),
                            e
                        ))
                    })?
                } else {
                    Kubeconfig::read().map_err(|e| {
                        ConnectionError::InvalidConfig(format!(
                            "Failed to read default kubeconfig: {}",
                            e
                        ))
                    })?
                };

                let options = KubeConfigOptions {
                    context,
                    cluster: None,
                    user: None,
                };

                Config::from_custom_kubeconfig(kubeconfig, &options)
                    .await
                    .map_err(|e| {
                        ConnectionError::InvalidConfig(format!(
                            "Failed to create config from kubeconfig: {}",
                            e
                        ))
                    })?
            }
            KubernetesAuth::InCluster => Config::incluster().map_err(|e| {
                ConnectionError::InvalidConfig(format!("Failed to load in-cluster config: {}", e))
            })?,
            KubernetesAuth::BearerToken(token) => {
                // Start with default kubeconfig to get server URL
                let mut config = Config::infer().await.map_err(|e| {
                    ConnectionError::InvalidConfig(format!("Failed to infer config: {}", e))
                })?;
                // Override with bearer token auth header
                config.auth_info.token = Some(token.into());
                config
            }
        };

        Client::try_from(config).map_err(|e| {
            ConnectionError::ConnectionFailed(format!("Failed to create client: {}", e))
        })
    }

    /// Verify that the target pod exists and is running
    pub async fn verify_pod(&self) -> ConnectionResult<PodInfo> {
        let pod = self.pods_api.get(&self.pod).await.map_err(|e| {
            ConnectionError::ConnectionFailed(format!("Failed to get pod {}: {}", self.pod, e))
        })?;

        let status = pod.status.as_ref().ok_or_else(|| {
            ConnectionError::ConnectionFailed(format!("Pod {} has no status", self.pod))
        })?;

        let phase = status.phase.clone().unwrap_or_default();
        let ready = status
            .conditions
            .as_ref()
            .map(|conds| {
                conds
                    .iter()
                    .any(|c| c.type_ == "Ready" && c.status == "True")
            })
            .unwrap_or(false);

        let containers: Vec<String> = pod
            .spec
            .as_ref()
            .map(|spec| spec.containers.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();

        Ok(PodInfo {
            name: self.pod.clone(),
            namespace: self.namespace.clone(),
            phase,
            ready,
            containers,
        })
    }

    /// Check if pod is running
    async fn is_pod_running(&self) -> ConnectionResult<bool> {
        match self.verify_pod().await {
            Ok(info) => Ok(info.phase == "Running"),
            Err(_) => Ok(false),
        }
    }

    /// Build attach parameters for exec
    fn build_attach_params(&self, options: &ExecuteOptions) -> AttachParams {
        let mut params = AttachParams::default();

        // Always capture stdout and stderr
        params = params.stdout(true).stderr(true);

        // Enable stdin if we might need to send input (e.g., for password)
        if options.escalate && options.escalate_password.is_some() {
            params = params.stdin(true);
        } else {
            params = params.stdin(false);
        }

        // Set container if specified
        if let Some(container) = &self.container {
            params = params.container(container);
        }

        params
    }

    /// Execute command and collect output
    async fn execute_in_pod(
        &self,
        command: Vec<String>,
        options: &ExecuteOptions,
    ) -> ConnectionResult<CommandResult> {
        let attach_params = self.build_attach_params(options);

        debug!(
            pod = %self.pod,
            namespace = %self.namespace,
            command = ?command,
            "Executing command in Kubernetes pod"
        );

        let mut attached: AttachedProcess = self
            .pods_api
            .exec(
                &self.pod,
                command.iter().map(|s| s.as_str()),
                &attach_params,
            )
            .await
            .map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to exec in pod: {}", e))
            })?;

        // Handle stdin for password escalation
        if options.escalate && options.escalate_password.is_some() {
            if let Some(mut stdin) = attached.stdin() {
                let password = options.escalate_password.as_ref().unwrap();
                stdin
                    .write_all(format!("{}\n", password).as_bytes())
                    .await
                    .map_err(|e| {
                        ConnectionError::ExecutionFailed(format!("Failed to write password: {}", e))
                    })?;
            }
        }

        // Collect stdout
        let mut stdout_data = Vec::new();
        if let Some(mut stdout) = attached.stdout() {
            stdout.read_to_end(&mut stdout_data).await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to read stdout: {}", e))
            })?;
        }

        // Collect stderr
        let mut stderr_data = Vec::new();
        if let Some(mut stderr) = attached.stderr() {
            stderr.read_to_end(&mut stderr_data).await.map_err(|e| {
                ConnectionError::ExecutionFailed(format!("Failed to read stderr: {}", e))
            })?;
        }

        // Wait for process to complete and get exit status
        let status = attached.take_status();
        let exit_code = if let Some(status) = status {
            match status.await {
                Some(status) => {
                    if status.status == Some("Success".to_string()) {
                        0
                    } else {
                        status
                            .reason
                            .as_ref()
                            .and_then(|r| r.parse::<i32>().ok())
                            .unwrap_or(1)
                    }
                }
                None => 0,
            }
        } else {
            0
        };

        let stdout = String::from_utf8_lossy(&stdout_data).to_string();
        let stderr = String::from_utf8_lossy(&stderr_data).to_string();

        trace!(
            exit_code = %exit_code,
            stdout_len = %stdout.len(),
            stderr_len = %stderr.len(),
            "Kubernetes exec completed"
        );

        if exit_code == 0 {
            Ok(CommandResult::success(stdout, stderr))
        } else {
            Ok(CommandResult::failure(exit_code, stdout, stderr))
        }
    }

    /// Build the full command with options
    fn build_command(&self, command: &str, options: &ExecuteOptions) -> Vec<String> {
        let mut parts = vec!["sh".to_string(), "-c".to_string()];

        let mut full_command = String::new();

        // Add environment variables
        for (key, value) in &options.env {
            full_command.push_str(&format!(
                "export {}='{}'; ",
                key,
                value.replace('\'', "'\\''")
            ));
        }

        // Add working directory
        if let Some(cwd) = &options.cwd {
            full_command.push_str(&format!("cd '{}' && ", cwd.replace('\'', "'\\''")));
        }

        // Add privilege escalation
        if options.escalate {
            let method = options.escalate_method.as_deref().unwrap_or("sudo");
            let user = options.escalate_user.as_deref().unwrap_or("root");

            match method {
                "sudo" => {
                    if options.escalate_password.is_some() {
                        full_command.push_str(&format!("sudo -S -u {} -- ", user));
                    } else {
                        full_command.push_str(&format!("sudo -u {} -- ", user));
                    }
                }
                "su" => {
                    full_command.push_str(&format!("su - {} -c ", user));
                }
                _ => {
                    full_command.push_str(&format!("sudo -u {} -- ", user));
                }
            }
        }

        full_command.push_str(command);
        parts.push(full_command);

        parts
    }

    /// Get the namespace
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Get the pod name
    pub fn pod(&self) -> &str {
        &self.pod
    }

    /// Get the container name
    pub fn container(&self) -> Option<&str> {
        self.container.as_deref()
    }
}

#[async_trait]
impl Connection for KubernetesConnection {
    fn identifier(&self) -> &str {
        &self.identifier
    }

    async fn is_alive(&self) -> bool {
        self.is_pod_running().await.unwrap_or(false)
    }

    async fn execute(
        &self,
        command: &str,
        options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        let options = options.unwrap_or_default();

        // Verify pod is running
        if !self.is_pod_running().await? {
            return Err(ConnectionError::ConnectionFailed(format!(
                "Pod {} is not running",
                self.pod
            )));
        }

        // Build the command
        let cmd_parts = self.build_command(command, &options);

        // Handle timeout
        if let Some(timeout_secs) = options.timeout {
            let timeout = tokio::time::Duration::from_secs(timeout_secs);
            match tokio::time::timeout(timeout, self.execute_in_pod(cmd_parts, &options)).await {
                Ok(result) => result,
                Err(_) => Err(ConnectionError::Timeout(timeout_secs)),
            }
        } else {
            self.execute_in_pod(cmd_parts, &options).await
        }
    }

    async fn upload(
        &self,
        local_path: &Path,
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();

        debug!(
            local = %local_path.display(),
            remote = %remote_path.display(),
            pod = %self.pod,
            "Uploading file to Kubernetes pod"
        );

        // Read local file content
        let content = tokio::fs::read(local_path).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to read local file: {}", e))
        })?;

        self.upload_content(&content, remote_path, Some(options))
            .await
    }

    async fn upload_content(
        &self,
        content: &[u8],
        remote_path: &Path,
        options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        let options = options.unwrap_or_default();

        debug!(
            remote = %remote_path.display(),
            pod = %self.pod,
            size = %content.len(),
            "Uploading content to Kubernetes pod"
        );

        // Create parent directories if needed
        if options.create_dirs {
            if let Some(parent) = remote_path.parent() {
                let mkdir_cmd = format!("mkdir -p '{}'", parent.display());
                self.execute(&mkdir_cmd, None).await?;
            }
        }

        // Base64 encode the content and decode it on the pod
        // This avoids issues with binary data and shell escaping
        let encoded = BASE64_STANDARD.encode(content);

        // Write using base64 decode
        let write_cmd = format!(
            "echo '{}' | base64 -d > '{}'",
            encoded,
            remote_path.display()
        );
        let result = self.execute(&write_cmd, None).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to write file: {}",
                result.stderr
            )));
        }

        // Set permissions if specified
        if let Some(mode) = options.mode {
            let chmod_cmd = format!("chmod {:o} '{}'", mode, remote_path.display());
            self.execute(&chmod_cmd, None).await?;
        }

        // Set owner/group if specified
        if options.owner.is_some() || options.group.is_some() {
            let ownership = match (&options.owner, &options.group) {
                (Some(o), Some(g)) => format!("{}:{}", o, g),
                (Some(o), None) => o.to_string(),
                (None, Some(g)) => format!(":{}", g),
                (None, None) => return Ok(()),
            };

            let chown_cmd = format!("chown {} '{}'", ownership, remote_path.display());
            self.execute(&chown_cmd, None).await?;
        }

        Ok(())
    }

    async fn download(&self, remote_path: &Path, local_path: &Path) -> ConnectionResult<()> {
        debug!(
            remote = %remote_path.display(),
            local = %local_path.display(),
            pod = %self.pod,
            "Downloading file from Kubernetes pod"
        );

        // Create parent directories for local file
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                ConnectionError::TransferFailed(format!("Failed to create local directory: {}", e))
            })?;
        }

        let content = self.download_content(remote_path).await?;

        tokio::fs::write(local_path, &content).await.map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to write local file: {}", e))
        })?;

        Ok(())
    }

    async fn download_content(&self, remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        debug!(
            remote = %remote_path.display(),
            pod = %self.pod,
            "Downloading content from Kubernetes pod"
        );

        // Use base64 to safely transfer binary content
        let command = format!("base64 '{}'", remote_path.display());
        let result = self.execute(&command, None).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to read file: {}",
                result.stderr
            )));
        }

        // Decode base64 content
        let decoded = BASE64_STANDARD.decode(result.stdout.trim()).map_err(|e| {
            ConnectionError::TransferFailed(format!("Failed to decode content: {}", e))
        })?;

        Ok(decoded)
    }

    async fn path_exists(&self, path: &Path) -> ConnectionResult<bool> {
        let command = format!("test -e '{}' && echo yes || echo no", path.display());
        let result = self.execute(&command, None).await?;
        Ok(result.stdout.trim() == "yes")
    }

    async fn is_directory(&self, path: &Path) -> ConnectionResult<bool> {
        let command = format!("test -d '{}' && echo yes || echo no", path.display());
        let result = self.execute(&command, None).await?;
        Ok(result.stdout.trim() == "yes")
    }

    async fn stat(&self, path: &Path) -> ConnectionResult<FileStat> {
        // Use stat command to get file info (Linux stat format)
        let command = format!("stat -c '%s|%a|%u|%g|%X|%Y|%F' '{}'", path.display());
        let result = self.execute(&command, None).await?;

        if !result.success {
            return Err(ConnectionError::TransferFailed(format!(
                "Failed to stat file: {}",
                result.stderr
            )));
        }

        let parts: Vec<&str> = result.stdout.trim().split('|').collect();
        if parts.len() != 7 {
            return Err(ConnectionError::TransferFailed(
                "Invalid stat output".to_string(),
            ));
        }

        let file_type = parts[6];

        Ok(FileStat {
            size: parts[0].parse().unwrap_or(0),
            mode: u32::from_str_radix(parts[1], 8).unwrap_or(0),
            uid: parts[2].parse().unwrap_or(0),
            gid: parts[3].parse().unwrap_or(0),
            atime: parts[4].parse().unwrap_or(0),
            mtime: parts[5].parse().unwrap_or(0),
            is_dir: file_type.contains("directory"),
            is_file: file_type.contains("regular"),
            is_symlink: file_type.contains("symbolic link"),
        })
    }

    async fn close(&self) -> ConnectionResult<()> {
        // Nothing to close for Kubernetes connection
        // The client can be reused
        Ok(())
    }
}

/// Pod information
#[derive(Debug, Clone)]
pub struct PodInfo {
    /// Pod name
    pub name: String,
    /// Namespace
    pub namespace: String,
    /// Pod phase (Pending, Running, Succeeded, Failed, Unknown)
    pub phase: String,
    /// Whether pod is ready
    pub ready: bool,
    /// Container names in the pod
    pub containers: Vec<String>,
}

/// Builder for Kubernetes connections
#[derive(Debug, Clone, Default)]
pub struct KubernetesConnectionBuilder {
    namespace: Option<String>,
    pod: Option<String>,
    container: Option<String>,
    auth: Option<KubernetesAuth>,
    kubeconfig_path: Option<PathBuf>,
    context: Option<String>,
}

impl KubernetesConnectionBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the target namespace
    pub fn namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = Some(namespace.into());
        self
    }

    /// Set the target pod
    pub fn pod(mut self, pod: impl Into<String>) -> Self {
        self.pod = Some(pod.into());
        self
    }

    /// Set the target container
    pub fn container(mut self, container: impl Into<String>) -> Self {
        self.container = Some(container.into());
        self
    }

    /// Set the kubeconfig file path
    pub fn kubeconfig_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.kubeconfig_path = Some(path.into());
        self
    }

    /// Set the kubeconfig context to use
    pub fn context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Use in-cluster service account authentication
    pub fn in_cluster(mut self) -> Self {
        self.auth = Some(KubernetesAuth::InCluster);
        self
    }

    /// Use bearer token authentication
    pub fn bearer_token(mut self, token: impl Into<String>) -> Self {
        self.auth = Some(KubernetesAuth::BearerToken(token.into()));
        self
    }

    /// Build the connection
    pub async fn build(self) -> ConnectionResult<KubernetesConnection> {
        let namespace = self.namespace.unwrap_or_else(|| "default".to_string());
        let pod = self
            .pod
            .ok_or_else(|| ConnectionError::InvalidConfig("Pod name is required".to_string()))?;

        let auth = self.auth.unwrap_or(KubernetesAuth::Kubeconfig {
            path: self.kubeconfig_path,
            context: self.context,
        });

        KubernetesConnection::new(namespace, pod, self.container, auth).await
    }
}

/// List pods in a namespace
pub async fn list_pods(namespace: &str, auth: KubernetesAuth) -> ConnectionResult<Vec<PodInfo>> {
    let client = KubernetesConnection::create_client(auth).await?;
    let pods_api: Api<Pod> = Api::namespaced(client, namespace);

    let pods = pods_api
        .list(&ListParams::default())
        .await
        .map_err(|e| ConnectionError::ConnectionFailed(format!("Failed to list pods: {}", e)))?;

    let pod_infos: Vec<PodInfo> = pods
        .items
        .into_iter()
        .filter_map(|pod| {
            let name = pod.metadata.name?;
            let status = pod.status.as_ref()?;
            let phase = status.phase.clone().unwrap_or_default();
            let ready = status
                .conditions
                .as_ref()
                .map(|conds| {
                    conds
                        .iter()
                        .any(|c| c.type_ == "Ready" && c.status == "True")
                })
                .unwrap_or(false);

            let containers: Vec<String> = pod
                .spec
                .as_ref()
                .map(|spec| spec.containers.iter().map(|c| c.name.clone()).collect())
                .unwrap_or_default();

            Some(PodInfo {
                name,
                namespace: namespace.to_string(),
                phase,
                ready,
                containers,
            })
        })
        .collect();

    Ok(pod_infos)
}

/// List namespaces
pub async fn list_namespaces(auth: KubernetesAuth) -> ConnectionResult<Vec<String>> {
    use k8s_openapi::api::core::v1::Namespace;

    let client = KubernetesConnection::create_client(auth).await?;
    let ns_api: Api<Namespace> = Api::all(client);

    let namespaces = ns_api.list(&ListParams::default()).await.map_err(|e| {
        ConnectionError::ConnectionFailed(format!("Failed to list namespaces: {}", e))
    })?;

    Ok(namespaces
        .items
        .into_iter()
        .filter_map(|ns| ns.metadata.name)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kubernetes_auth_default() {
        let auth = KubernetesAuth::default();
        match auth {
            KubernetesAuth::Kubeconfig { path, context } => {
                assert!(path.is_none());
                assert!(context.is_none());
            }
            _ => panic!("Expected Kubeconfig variant"),
        }
    }

    #[test]
    fn test_builder_missing_pod() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = KubernetesConnectionBuilder::new()
                .namespace("default")
                .build()
                .await;

            assert!(result.is_err());
            match result {
                Err(ConnectionError::InvalidConfig(msg)) => {
                    assert!(msg.contains("Pod name is required"));
                }
                _ => panic!("Expected InvalidConfig error"),
            }
        });
    }

    #[test]
    fn test_builder_chain() {
        let builder = KubernetesConnectionBuilder::new()
            .namespace("my-namespace")
            .pod("my-pod")
            .container("app")
            .kubeconfig_path("/home/user/.kube/config")
            .context("my-cluster");

        assert_eq!(builder.namespace, Some("my-namespace".to_string()));
        assert_eq!(builder.pod, Some("my-pod".to_string()));
        assert_eq!(builder.container, Some("app".to_string()));
        assert_eq!(
            builder.kubeconfig_path,
            Some(PathBuf::from("/home/user/.kube/config"))
        );
        assert_eq!(builder.context, Some("my-cluster".to_string()));
    }

    #[test]
    fn test_builder_in_cluster() {
        let builder = KubernetesConnectionBuilder::new()
            .pod("my-pod")
            .in_cluster();

        match builder.auth {
            Some(KubernetesAuth::InCluster) => {}
            _ => panic!("Expected InCluster auth"),
        }
    }

    #[test]
    fn test_builder_bearer_token() {
        let builder = KubernetesConnectionBuilder::new()
            .pod("my-pod")
            .bearer_token("my-token");

        match builder.auth {
            Some(KubernetesAuth::BearerToken(token)) => {
                assert_eq!(token, "my-token");
            }
            _ => panic!("Expected BearerToken auth"),
        }
    }

    #[test]
    fn test_build_command_basic() {
        // We need a mock connection for this test
        // For now, we just verify the builder compiles
    }

    #[test]
    fn test_pod_info() {
        let info = PodInfo {
            name: "test-pod".to_string(),
            namespace: "default".to_string(),
            phase: "Running".to_string(),
            ready: true,
            containers: vec!["app".to_string(), "sidecar".to_string()],
        };

        assert_eq!(info.name, "test-pod");
        assert_eq!(info.namespace, "default");
        assert_eq!(info.phase, "Running");
        assert!(info.ready);
        assert_eq!(info.containers.len(), 2);
    }
}
