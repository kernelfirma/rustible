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
    // If it's an absolute path, check if it exists
    let playbook_path = Path::new(playbook);
    if playbook_path.is_absolute() && playbook_path.exists() {
        return Ok(playbook.to_string());
    }

    // Search in configured paths
    for base_path in search_paths {
        let full_path = Path::new(base_path).join(playbook);
        if full_path.exists() {
            return Ok(full_path.to_string_lossy().to_string());
        }

        // Try with .yml extension
        let with_yml = full_path.with_extension("yml");
        if with_yml.exists() {
            return Ok(with_yml.to_string_lossy().to_string());
        }

        // Try with .yaml extension
        let with_yaml = full_path.with_extension("yaml");
        if with_yaml.exists() {
            return Ok(with_yaml.to_string_lossy().to_string());
        }
    }

    Err(ApiError::NotFound(format!(
        "Playbook not found: {}",
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
    let playbook = match crate::playbook::Playbook::from_file(&playbook_path).await {
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
            playbook.play_count(),
            playbook.task_count()
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

    // TODO: Actually execute the playbook using the Executor
    // For now, simulate execution
    state.append_job_output(job_id, "Execution started...".to_string(), "stdout");

    // Simulate some work
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    state.append_job_output(job_id, "Gathering facts...".to_string(), "stdout");
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    for (_i, play) in playbook.plays.iter().enumerate() {
        state.append_job_output(job_id, format!("PLAY [{}] ***", play.name), "stdout");

        for task in play.all_tasks() {
            state.append_job_output(job_id, format!("TASK [{}] ***", task.name), "stdout");

            // Simulate task execution
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            state.append_job_output(job_id, format!("ok: [localhost]"), "stdout");
        }
    }

    // Set final stats
    let stats = JobStats {
        hosts: inventory.as_ref().map(|i| i.host_count()).unwrap_or(1),
        ok: playbook.task_count(),
        changed: 0,
        failed: 0,
        skipped: 0,
        unreachable: 0,
    };
    state.set_job_stats(job_id, stats);

    state.append_job_output(job_id, "Playbook execution completed".to_string(), "stdout");
    state.update_job_status(job_id, JobStatus::Success);
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
