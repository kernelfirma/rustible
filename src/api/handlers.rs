//! API route handlers.

use std::path::Path;
use std::sync::Arc;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use tracing::{error, info};
use uuid::Uuid;

use super::auth::{AuthenticatedUser, Claims};
use super::error::{ApiError, ApiResult};
use super::state::AppState;
use super::types::*;
use crate::executor::task::TaskStatus;
use crate::executor::{ExecutionEvent, ExecutionStrategy, Executor, ExecutorConfig};
use crate::inventory::{Host, Inventory};

// ============================================================================
// Health & Info Handlers
// ============================================================================

/// Health check endpoint.
pub async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(HealthResponse {
        status: "healthy".to_string(),
        version: crate::version().to_string(),
        uptime_secs: state.uptime_secs(),
        active_jobs: state.active_job_count(),
    })
}

/// API info endpoint.
pub async fn api_info() -> impl IntoResponse {
    Json(ApiInfoResponse {
        name: "Rustible API".to_string(),
        version: crate::version().to_string(),
        endpoints: vec![
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/v1/health".to_string(),
                description: "Health check".to_string(),
            },
            EndpointInfo {
                method: "POST".to_string(),
                path: "/api/v1/auth/login".to_string(),
                description: "Authenticate and get JWT token".to_string(),
            },
            EndpointInfo {
                method: "POST".to_string(),
                path: "/api/v1/playbooks/execute".to_string(),
                description: "Execute a playbook".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/v1/jobs".to_string(),
                description: "List all jobs".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/v1/jobs/:id".to_string(),
                description: "Get job details".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/v1/inventory".to_string(),
                description: "Get inventory summary".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/v1/inventory/hosts".to_string(),
                description: "List all hosts".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/v1/inventory/groups".to_string(),
                description: "List all groups".to_string(),
            },
            EndpointInfo {
                method: "WS".to_string(),
                path: "/api/v1/ws/jobs/:id".to_string(),
                description: "WebSocket for real-time job output".to_string(),
            },
        ],
    })
}

// ============================================================================
// Authentication Handlers
// ============================================================================

/// Login and get JWT token.
pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> ApiResult<Json<LoginResponse>> {
    // Verify credentials
    let roles = state
        .verify_credentials(&req.username, &req.password)
        .ok_or_else(|| ApiError::Unauthorized("Invalid credentials".to_string()))?;

    // Generate token with claims
    let mut claims = Claims::new(
        &req.username,
        state.config.token_expiration_secs,
        "rustible",
    );
    claims.roles = roles;

    let token = state
        .jwt_auth
        .generate_token_with_claims(&claims)
        .map_err(|e| ApiError::Internal(format!("Failed to generate token: {}", e)))?;

    info!("User '{}' logged in successfully", req.username);

    Ok(Json(LoginResponse {
        token,
        token_type: "Bearer".to_string(),
        expires_in: state.config.token_expiration_secs,
    }))
}

/// Refresh JWT token.
pub async fn refresh_token(
    State(state): State<Arc<AppState>>,
    user: AuthenticatedUser,
) -> ApiResult<Json<LoginResponse>> {
    let token = state
        .jwt_auth
        .refresh_token(&user.claims)
        .map_err(|e| ApiError::Internal(format!("Failed to refresh token: {}", e)))?;

    Ok(Json(LoginResponse {
        token,
        token_type: "Bearer".to_string(),
        expires_in: state.config.token_expiration_secs,
    }))
}

/// Get current user info.
pub async fn me(user: AuthenticatedUser) -> impl IntoResponse {
    Json(serde_json::json!({
        "username": user.claims.sub,
        "roles": user.claims.roles,
        "issued_at": user.claims.iat,
        "expires_at": user.claims.exp,
    }))
}

// ============================================================================
// Playbook Handlers
// ============================================================================

/// Execute a playbook.
pub async fn execute_playbook(
    State(state): State<Arc<AppState>>,
    user: AuthenticatedUser,
    Json(req): Json<PlaybookExecuteRequest>,
) -> ApiResult<Json<PlaybookExecuteResponse>> {
    // Validate playbook exists
    let playbook_path = find_playbook(&state.config.playbook_paths, &req.playbook)?;

    info!(
        "User '{}' executing playbook: {}",
        user.claims.sub, playbook_path
    );

    // Create job
    let job_id = state.create_job(
        playbook_path.clone(),
        req.inventory.clone(),
        Some(user.claims.sub.clone()),
        req.extra_vars.clone(),
    );

    // Spawn async task to run the playbook
    let state_clone = state.clone();
    let req_clone = req;
    tokio::spawn(async move {
        run_playbook_job(state_clone, job_id, playbook_path, req_clone).await;
    });

    // Build WebSocket URL
    let ws_url = format!("/api/v1/ws/jobs/{}", job_id);

    Ok(Json(PlaybookExecuteResponse {
        job_id,
        status: JobStatus::Pending,
        message: "Playbook execution started".to_string(),
        websocket_url: Some(ws_url),
    }))
}

/// List available playbooks.
pub async fn list_playbooks(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
) -> ApiResult<Json<PlaybookListResponse>> {
    let mut playbooks = Vec::new();

    for search_path in &state.config.playbook_paths {
        let path = Path::new(search_path);
        if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if let Some(ext) = entry_path.extension() {
                        if ext == "yml" || ext == "yaml" {
                            if let Some(name) = entry_path.file_stem() {
                                let modified = entry
                                    .metadata()
                                    .ok()
                                    .and_then(|m| m.modified().ok())
                                    .map(DateTime::<Utc>::from);

                                // Try to parse to get play count
                                let plays = std::fs::read_to_string(&entry_path)
                                    .ok()
                                    .and_then(|content| {
                                        serde_yaml::from_str::<Vec<serde_yaml::Value>>(&content)
                                            .ok()
                                    })
                                    .map(|v| v.len())
                                    .unwrap_or(0);

                                playbooks.push(PlaybookInfo {
                                    name: name.to_string_lossy().to_string(),
                                    path: entry_path.to_string_lossy().to_string(),
                                    plays,
                                    modified,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Json(PlaybookListResponse { playbooks }))
}

/// Find a playbook file in the search paths.
fn find_playbook(search_paths: &[String], playbook: &str) -> ApiResult<String> {
    let playbook_path = Path::new(playbook);

    // Reject path traversal components in the user-supplied playbook name
    // before using it in any filesystem operations. This prevents path
    // traversal attacks (e.g. "../../etc/passwd") regardless of whether the
    // path is absolute or relative.
    for component in playbook_path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(ApiError::Forbidden(
                "Path traversal (\"..\" components) not allowed in playbook path".to_string(),
            ));
        }
    }

    // Helper to validate if an existing path is within allowed search paths
    let validate_path = |path: &Path| -> ApiResult<()> {
        let canonical_path = path
            .canonicalize()
            .map_err(|e| ApiError::Internal(format!("Failed to resolve path: {}", e)))?;

        for base in search_paths {
            // Canonicalize base path to handle symlinks and relative paths
            if let Ok(canonical_base) = Path::new(base).canonicalize() {
                if canonical_path.starts_with(&canonical_base) {
                    return Ok(());
                }
            }
        }
        Err(ApiError::Forbidden(format!(
            "Access denied to playbook outside search paths: {}",
            path.display()
        )))
    };

    // If it's an absolute path, reconstruct it from a trusted base + relative
    // suffix so that only sanitised paths reach filesystem operations.
    if playbook_path.is_absolute() {
        for base in search_paths {
            if let Ok(canonical_base) = Path::new(base).canonicalize() {
                if let Ok(relative) = playbook_path.strip_prefix(&canonical_base) {
                    // Reconstruct from trusted base to avoid tainted path in fs ops
                    let safe_path = canonical_base.join(relative);
                    if safe_path.exists() {
                        validate_path(&safe_path)?;
                        return Ok(safe_path.to_string_lossy().to_string());
                    }
                }
            }
        }
        return Err(ApiError::NotFound(format!(
            "Playbook not found or access denied: {}",
            playbook
        )));
    }

    // Search in configured paths
    for base_path in search_paths {
        let base = Path::new(base_path);

        let candidates = [
            base.join(playbook),
            base.join(playbook).with_extension("yml"),
            base.join(playbook).with_extension("yaml"),
        ];

        for full_path in candidates {
            if full_path.exists() {
                validate_path(&full_path)?;
                return Ok(full_path.to_string_lossy().to_string());
            }
        }
    }

    Err(ApiError::NotFound(format!(
        "Playbook not found or access denied: {}",
        playbook
    )))
}

/// Internal function to run a playbook job.
async fn run_playbook_job(
    state: Arc<AppState>,
    job_id: Uuid,
    playbook_path: String,
    req: PlaybookExecuteRequest,
) {
    // Update status to running
    state.update_job_status(job_id, JobStatus::Running);
    state.append_job_output(
        job_id,
        format!("Starting playbook: {}", playbook_path),
        "stdout",
    );

    // Load inventory if specified
    let inventory = if let Some(inv_path) = &req.inventory {
        match Inventory::load(inv_path) {
            Ok(inv) => Some(inv),
            Err(e) => {
                let error_msg = format!("Failed to load inventory: {}", e);
                error!("{}", error_msg);
                state.append_job_output(job_id, error_msg.clone(), "stderr");
                state.set_job_error(job_id, error_msg);
                state.update_job_status(job_id, JobStatus::Failed);
                return;
            }
        }
    } else {
        state.get_inventory().map(|i| (*i).clone())
    };

    // Parse playbook
    let playbook = match crate::executor::playbook::Playbook::load(&playbook_path) {
        Ok(pb) => pb,
        Err(e) => {
            let error_msg = format!("Failed to parse playbook: {}", e);
            error!("{}", error_msg);
            state.append_job_output(job_id, error_msg.clone(), "stderr");
            state.set_job_error(job_id, error_msg);
            state.update_job_status(job_id, JobStatus::Failed);
            return;
        }
    };

    // Log playbook info
    state.append_job_output(
        job_id,
        format!(
            "Playbook: {} plays, {} tasks",
            playbook.plays.len(),
            playbook
                .plays
                .iter()
                .map(|p| p.pre_tasks.len() + p.tasks.len() + p.post_tasks.len())
                .sum::<usize>()
        ),
        "stdout",
    );

    if let Some(ref inv) = inventory {
        state.append_job_output(
            job_id,
            format!(
                "Inventory: {} hosts, {} groups",
                inv.host_count(),
                inv.group_count()
            ),
            "stdout",
        );
    }

    // Create executor config
    let executor_config = ExecutorConfig {
        forks: req.forks.unwrap_or(5),
        check_mode: req.check,
        diff_mode: req.diff,
        verbosity: req.verbosity,
        strategy: ExecutionStrategy::Linear,
        task_timeout: 300,
        gather_facts: true,
        extra_vars: req.extra_vars,
        auto_rollback: false,
        forward_agent: false,
        pipelining: true,
        r#become: false,
        become_method: "sudo".to_string(),
        become_user: "root".to_string(),
        become_password: None,
        distributed: false,
        workers: 1,
        distribution_strategy: "adaptive".to_string(),
    };

    // Create runtime context
    use crate::executor::runtime::RuntimeContext;
    let runtime = if let Some(inv) = inventory.as_ref() {
        RuntimeContext::from_inventory(inv)
    } else {
        RuntimeContext::new()
    };

    // Setup event callback
    let job_id_clone = job_id;
    let state_clone = state.clone();
    let callback = Arc::new(move |event: ExecutionEvent| {
        let msg = match event {
            ExecutionEvent::PlaybookStart(name) => {
                format!("Starting playbook: {}", name)
            }
            ExecutionEvent::PlayStart(name) => {
                format!("PLAY [{}] ***", name)
            }
            ExecutionEvent::TaskStart { task, host } => {
                if let Some(host) = host {
                    format!("TASK [{}] on {} ***", task, host)
                } else {
                    format!("TASK [{}] ***", task)
                }
            }
            ExecutionEvent::HostTaskComplete(host, _task, result) => {
                let status_str = match result.status {
                    TaskStatus::Ok => "ok",
                    TaskStatus::Changed => "changed",
                    TaskStatus::Failed => "failed",
                    TaskStatus::Skipped => "skipping",
                    TaskStatus::Unreachable => "unreachable",
                    _ => "unknown", // Handle potential future statuses
                };
                if let Some(msg) = &result.msg {
                    format!("{}: [{}] => {}", status_str, host, msg)
                } else {
                    format!("{}: [{}]", status_str, host)
                }
            }
            ExecutionEvent::PlaybookFinish(_) => "Playbook execution completed".to_string(),
            ExecutionEvent::Log(msg) => msg,
        };
        state_clone.append_job_output(job_id_clone, msg, "stdout");
    });

    // Create executor with callback
    let executor = Executor::with_runtime(executor_config, runtime).with_event_callback(callback);

    // Execute playbook
    match executor.run_playbook(&playbook).await {
        Ok(results) => {
            // Calculate stats
            let summary = Executor::summarize_results(&results);
            let job_stats = JobStats {
                hosts: results.len(),
                ok: summary.ok,
                changed: summary.changed,
                failed: summary.failed,
                skipped: summary.skipped,
                unreachable: summary.unreachable,
            };

            state.set_job_stats(job_id, job_stats);

            let has_failures = summary.failed > 0 || summary.unreachable > 0;
            if has_failures {
                state.update_job_status(job_id, JobStatus::Failed);
            } else {
                state.update_job_status(job_id, JobStatus::Success);
            }
        }
        Err(e) => {
            let error_msg = format!("Playbook execution failed: {}", e);
            error!("{}", error_msg);
            state.append_job_output(job_id, error_msg.clone(), "stderr");
            state.set_job_error(job_id, error_msg);
            state.update_job_status(job_id, JobStatus::Failed);
        }
    }
}

// ============================================================================
// Job Handlers
// ============================================================================

/// List jobs with optional filtering.
pub async fn list_jobs(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
    Query(query): Query<JobListQuery>,
) -> impl IntoResponse {
    let (jobs, total) = state.list_jobs(query.status, query.page, query.per_page);

    Json(JobListResponse {
        jobs,
        total,
        page: query.page,
        per_page: query.per_page,
    })
}

/// Get job details.
pub async fn get_job(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
    AxumPath(job_id): AxumPath<Uuid>,
) -> ApiResult<Json<JobDetails>> {
    let job = state
        .get_job(job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Job not found: {}", job_id)))?;

    Ok(Json(JobDetails {
        info: job.to_info(),
        stats: job.stats.clone(),
        output: Some(job.full_output()),
        error: job.error.clone(),
    }))
}

/// Get job output.
pub async fn get_job_output(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
    AxumPath(job_id): AxumPath<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let job = state
        .get_job(job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Job not found: {}", job_id)))?;

    Ok(job.full_output())
}

/// Cancel a job.
pub async fn cancel_job(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
    AxumPath(job_id): AxumPath<Uuid>,
) -> ApiResult<impl IntoResponse> {
    if state.cancel_job(job_id) {
        Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "message": "Job cancelled",
                "job_id": job_id
            })),
        ))
    } else {
        Err(ApiError::Conflict("Job cannot be cancelled".to_string()))
    }
}

// ============================================================================
// Inventory Handlers
// ============================================================================

/// Get inventory summary.
pub async fn get_inventory_summary(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
) -> ApiResult<Json<InventorySummaryResponse>> {
    let inventory = state
        .get_inventory()
        .or_else(|| state.load_inventory().ok())
        .ok_or_else(|| ApiError::NotFound("No inventory loaded".to_string()))?;

    Ok(Json(InventorySummaryResponse {
        host_count: inventory.host_count(),
        group_count: inventory.group_count(),
        hosts: inventory.host_names().cloned().collect(),
        groups: inventory.group_names().cloned().collect(),
        source: state.config.inventory_path.clone(),
    }))
}

/// List all hosts.
pub async fn list_hosts(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
) -> ApiResult<Json<HostListResponse>> {
    let inventory = state
        .get_inventory()
        .or_else(|| state.load_inventory().ok())
        .ok_or_else(|| ApiError::NotFound("No inventory loaded".to_string()))?;

    let hosts: Vec<HostResponse> = inventory.hosts().map(host_to_response).collect();

    let total = hosts.len();
    Ok(Json(HostListResponse { hosts, total }))
}

/// Get a specific host.
pub async fn get_host(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
    AxumPath(host_name): AxumPath<String>,
) -> ApiResult<Json<HostResponse>> {
    let inventory = state
        .get_inventory()
        .or_else(|| state.load_inventory().ok())
        .ok_or_else(|| ApiError::NotFound("No inventory loaded".to_string()))?;

    let host = inventory
        .get_host(&host_name)
        .ok_or_else(|| ApiError::NotFound(format!("Host not found: {}", host_name)))?;

    Ok(Json(host_to_response(host)))
}

/// List all groups.
pub async fn list_groups(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
) -> ApiResult<Json<GroupListResponse>> {
    let inventory = state
        .get_inventory()
        .or_else(|| state.load_inventory().ok())
        .ok_or_else(|| ApiError::NotFound("No inventory loaded".to_string()))?;

    let groups: Vec<GroupResponse> = inventory
        .groups()
        .map(|g| GroupResponse {
            name: g.name.clone(),
            hosts: g.hosts.iter().cloned().collect(),
            children: g.children.iter().cloned().collect(),
            parents: g.parents.iter().cloned().collect(),
            vars: g
                .vars
                .iter()
                .map(|(k, v)| {
                    (
                        k.clone(),
                        serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
                    )
                })
                .collect(),
        })
        .collect();

    let total = groups.len();
    Ok(Json(GroupListResponse { groups, total }))
}

/// Get a specific group.
pub async fn get_group(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
    AxumPath(group_name): AxumPath<String>,
) -> ApiResult<Json<GroupResponse>> {
    let inventory = state
        .get_inventory()
        .or_else(|| state.load_inventory().ok())
        .ok_or_else(|| ApiError::NotFound("No inventory loaded".to_string()))?;

    let group = inventory
        .get_group(&group_name)
        .ok_or_else(|| ApiError::NotFound(format!("Group not found: {}", group_name)))?;

    Ok(Json(GroupResponse {
        name: group.name.clone(),
        hosts: group.hosts.iter().cloned().collect(),
        children: group.children.iter().cloned().collect(),
        parents: group.parents.iter().cloned().collect(),
        vars: group
            .vars
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
                )
            })
            .collect(),
    }))
}

/// Reload inventory from disk.
pub async fn reload_inventory(
    State(state): State<Arc<AppState>>,
    _user: AuthenticatedUser,
) -> ApiResult<Json<InventorySummaryResponse>> {
    let inventory = state
        .load_inventory()
        .map_err(|e| ApiError::Inventory(format!("Failed to reload inventory: {}", e)))?;

    Ok(Json(InventorySummaryResponse {
        host_count: inventory.host_count(),
        group_count: inventory.group_count(),
        hosts: inventory.host_names().cloned().collect(),
        groups: inventory.group_names().cloned().collect(),
        source: state.config.inventory_path.clone(),
    }))
}

/// Convert Host to HostResponse.
fn host_to_response(host: &Host) -> HostResponse {
    let connection_type = match host.connection.connection {
        crate::inventory::ConnectionType::Ssh => "ssh",
        crate::inventory::ConnectionType::Local => "local",
        crate::inventory::ConnectionType::Docker => "docker",
        crate::inventory::ConnectionType::Podman => "podman",
        crate::inventory::ConnectionType::Winrm => "winrm",
    };

    HostResponse {
        name: host.name.clone(),
        ansible_host: host.ansible_host.clone(),
        groups: host.groups.iter().cloned().collect(),
        vars: host
            .vars
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
                )
            })
            .collect(),
        connection: connection_type.to_string(),
        port: host.connection.ssh.port,
        user: host.connection.ssh.user.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_find_playbook_valid_paths() {
        let temp_dir = TempDir::new().unwrap();
        let search_path = temp_dir.path().join("playbooks");
        fs::create_dir(&search_path).unwrap();

        // Create a valid playbook
        let valid_playbook = search_path.join("site.yml");
        fs::write(&valid_playbook, "- hosts: all").unwrap();

        let search_paths = vec![search_path.to_str().unwrap().to_string()];

        // Test finding by relative path
        let result = find_playbook(&search_paths, "site.yml");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            valid_playbook.to_string_lossy().to_string()
        );

        // Test finding by absolute path
        let result = find_playbook(&search_paths, valid_playbook.to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_playbook_path_traversal() {
        let temp_dir = TempDir::new().unwrap();
        let search_path = temp_dir.path().join("playbooks");
        fs::create_dir(&search_path).unwrap();

        // Create a secret file outside search path
        let secret_file = temp_dir.path().join("secret.yml");
        fs::write(&secret_file, "secret data").unwrap();

        let search_paths = vec![search_path.to_str().unwrap().to_string()];

        // Test traversal via relative path
        // playbooks/../secret.yml
        let traversal = "../secret.yml";
        let result = find_playbook(&search_paths, traversal);

        // Should fail with Forbidden or NotFound (depending on how join works vs exists)
        // ../secret.yml joined with /tmp/.../playbooks becomes /tmp/.../playbooks/../secret.yml -> /tmp/.../secret.yml
        // It exists. But validation should forbid it.
        assert!(matches!(result, Err(ApiError::Forbidden(_))));

        // Test absolute path outside search path
        let result = find_playbook(&search_paths, secret_file.to_str().unwrap());
        assert!(matches!(result, Err(ApiError::Forbidden(_))));
    }

    #[test]
    fn test_find_playbook_symlink_traversal() {
        // This test requires unix symlinks
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;

            let temp_dir = TempDir::new().unwrap();
            let search_path = temp_dir.path().join("playbooks");
            fs::create_dir(&search_path).unwrap();

            let secret_file = temp_dir.path().join("secret.yml");
            fs::write(&secret_file, "secret").unwrap();

            // Create a symlink inside search path pointing outside
            // playbooks/link.yml -> ../secret.yml
            let link_path = search_path.join("link.yml");
            symlink(&secret_file, &link_path).unwrap();

            let search_paths = vec![search_path.to_str().unwrap().to_string()];

            // Trying to access the symlink
            let result = find_playbook(&search_paths, "link.yml");

            // Canonicalization resolves the link to outside search path
            assert!(matches!(result, Err(ApiError::Forbidden(_))));
        }
    }
}
