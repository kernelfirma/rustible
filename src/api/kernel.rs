//! Internal kernel deployment workflow helpers.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use chrono::Utc;
use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::fs;
use tokio::process::Command;
use tracing::warn;
use uuid::Uuid;

use super::error::{ApiError, ApiResult};
use super::state::AppState;
use super::types::{
    JobStats, JobStatus, KernelBootloader, KernelDeploymentActionKind,
    KernelDeploymentActionRequired, KernelDeploymentArtifact, KernelDeploymentBmc,
    KernelDeploymentBmcActionHint, KernelDeploymentBmcProvider, KernelDeploymentHost,
    KernelDeploymentProgress, KernelDeploymentRebootPolicy, KernelDeploymentRequest,
    KernelDeploymentResponse, KernelDeploymentResumeRequest, KernelDeploymentStage,
    KernelDeploymentStatusResponse, KernelSecureBootMode,
};
use crate::connection::{
    CommandResult, Connection, ConnectionBuilder, ExecuteOptions, TransferOptions,
};

const HOST_REBOOT_WAIT_SECS: u64 = 180;
const HOST_CONNECT_TIMEOUT_SECS: u64 = 30;
const MIN_BOOT_SPACE_KIB: u64 = 262_144;

pub fn validate_request(req: &KernelDeploymentRequest) -> ApiResult<()> {
    if req.hosts.is_empty() {
        return Err(ApiError::BadRequest(
            "at least one host is required".to_string(),
        ));
    }

    if req.artifact.url.trim().is_empty()
        || req.artifact.sha256.trim().is_empty()
        || req.artifact.package_name.trim().is_empty()
        || req.artifact.expected_kernel_release.trim().is_empty()
        || req.artifact.signature_url.trim().is_empty()
        || req.artifact.public_key_url.trim().is_empty()
        || req.artifact.public_key_fingerprint.trim().is_empty()
    {
        return Err(ApiError::BadRequest(
            "artifact url, sha256, package_name, expected_kernel_release, signature_url, public_key_url, and public_key_fingerprint are required"
                .to_string(),
        ));
    }

    for host in &req.hosts {
        if host.name.trim().is_empty()
            || host.address.trim().is_empty()
            || host.username.trim().is_empty()
        {
            return Err(ApiError::BadRequest(
                "each host requires name, address, and username".to_string(),
            ));
        }
    }

    Ok(())
}

pub fn initial_response(
    job_id: Uuid,
    progress: KernelDeploymentProgress,
) -> Json<KernelDeploymentResponse> {
    Json(KernelDeploymentResponse {
        job_id,
        status: JobStatus::Pending,
        message: format!(
            "Kernel deployment queued for {} host(s)",
            progress.hosts.len()
        ),
        websocket_url: Some(format!("/api/v1/ws/jobs/{}", job_id)),
        deployment: progress,
    })
}

pub async fn get_status(
    state: &Arc<AppState>,
    job_id: Uuid,
) -> ApiResult<Json<KernelDeploymentStatusResponse>> {
    let job = state
        .get_job(job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Job not found: {}", job_id)))?;
    let deployment = state
        .kernel_job_progress(job_id)
        .await
        .ok_or_else(|| ApiError::NotFound(format!("Kernel deployment not found: {}", job_id)))?;

    Ok(Json(KernelDeploymentStatusResponse {
        job_id,
        status: job.status,
        deployment,
        error: job.error,
    }))
}

pub async fn resume_job(
    state: Arc<AppState>,
    job_id: Uuid,
    req: KernelDeploymentResumeRequest,
) -> ApiResult<Json<KernelDeploymentStatusResponse>> {
    let runtime = state
        .get_kernel_job_runtime(job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Kernel deployment not found: {}", job_id)))?;

    let deployment = runtime
        .clear_action_required(&req.action_id)
        .await
        .map_err(|err| ApiError::Conflict(err.to_string()))?;

    state.update_job_status(job_id, JobStatus::Running);
    state.append_job_output(
        job_id,
        format!("Resuming kernel deployment action {}", req.action_id),
        "stdout",
    );
    runtime.resume_notify.notify_waiters();

    let job = state
        .get_job(job_id)
        .ok_or_else(|| ApiError::NotFound(format!("Job not found: {}", job_id)))?;

    Ok(Json(KernelDeploymentStatusResponse {
        job_id,
        status: job.status,
        deployment,
        error: job.error,
    }))
}

pub async fn run_job(state: Arc<AppState>, job_id: Uuid, req: KernelDeploymentRequest) {
    state.update_job_status(job_id, JobStatus::Running);
    log_job(
        &state,
        job_id,
        "Starting internal kernel deployment workflow",
    );

    let staged_artifact = match stage_artifact(&state, job_id, &req.artifact).await {
        Ok(staged) => staged,
        Err(err) => {
            fail_job(&state, job_id, err.to_string()).await;
            return;
        }
    };

    let runtime = match state.get_kernel_job_runtime(job_id) {
        Some(runtime) => runtime,
        None => {
            fail_job(
                &state,
                job_id,
                "kernel deployment runtime state was not initialized".to_string(),
            )
            .await;
            return;
        }
    };

    let mut stats = JobStats {
        hosts: req.hosts.len(),
        ..Default::default()
    };

    for host in &req.hosts {
        match deploy_to_host(
            &state,
            &runtime,
            job_id,
            host,
            &staged_artifact,
            req.reboot_policy,
        )
        .await
        {
            Ok(HostDeploymentOutcome::Changed) => stats.changed += 1,
            Ok(HostDeploymentOutcome::Ok) => stats.ok += 1,
            Err(HostDeploymentError::Unreachable(message)) => {
                stats.unreachable += 1;
                log_job(&state, job_id, &format!("[{}] {}", host.name, message));
            }
            Err(HostDeploymentError::Failed(message)) => {
                stats.failed += 1;
                log_job(&state, job_id, &format!("[{}] {}", host.name, message));
            }
        }
    }

    state.set_job_stats(job_id, stats.clone());

    if stats.failed > 0 || stats.unreachable > 0 {
        runtime.set_stage(KernelDeploymentStage::Failed, None).await;
        let summary = format!(
            "Kernel deployment finished with failures: {} failed, {} unreachable",
            stats.failed, stats.unreachable
        );
        state.set_job_error(job_id, summary.clone());
        state.update_job_status(job_id, JobStatus::Failed);
        log_job(&state, job_id, &summary);
    } else {
        runtime
            .set_stage(KernelDeploymentStage::Succeeded, None)
            .await;
        state.update_job_status(job_id, JobStatus::Success);
        log_job(&state, job_id, "Kernel deployment completed successfully");
    }
}

#[derive(Debug)]
enum HostDeploymentOutcome {
    Ok,
    Changed,
}

#[derive(Debug)]
enum HostDeploymentError {
    Failed(String),
    Unreachable(String),
}

#[derive(Debug)]
struct StagedArtifact {
    _workspace: TempDir,
    package_path: PathBuf,
    package_sha256: String,
    package_name: String,
    expected_kernel_release: String,
    cert_path: Option<PathBuf>,
    cert_fingerprint: Option<String>,
}

#[derive(Debug)]
struct PreflightFacts {
    distro: String,
    distro_version: String,
    arch: String,
    current_kernel: String,
    current_kernel_package: Option<String>,
    boot_space_kib: u64,
    secure_boot_enabled: bool,
    bootloader: KernelBootloader,
}

#[derive(Debug)]
struct BootPlan {
    bootloader: KernelBootloader,
    entry_id: String,
}

async fn stage_artifact(
    state: &Arc<AppState>,
    job_id: Uuid,
    artifact: &KernelDeploymentArtifact,
) -> ApiResult<StagedArtifact> {
    let workspace = TempDir::new().map_err(|err| {
        ApiError::Internal(format!("failed to create staging directory: {}", err))
    })?;
    let package_path = workspace.path().join("kernel.deb");
    let signature_path = workspace.path().join("kernel.sig");
    let public_key_path = workspace.path().join("cosign.pub");

    fetch_to_path(&artifact.url, &package_path).await?;
    fetch_to_path(&artifact.signature_url, &signature_path).await?;
    fetch_to_path(&artifact.public_key_url, &public_key_path).await?;

    let public_key_fingerprint = sha256_file(&public_key_path).await?;
    ensure_expected_fingerprint(&public_key_fingerprint, &artifact.public_key_fingerprint)
        .map_err(ApiError::ValidationError)?;

    verify_sha256(&package_path, &artifact.sha256).await?;
    verify_cosign_blob(&package_path, &signature_path, &public_key_path).await?;
    log_job(
        state,
        job_id,
        "Artifact signature and SHA-256 verified on the control node",
    );

    let (cert_path, cert_fingerprint) = match &artifact.secure_boot_cert_url {
        Some(url) => {
            let cert_path = workspace.path().join("mok.der");
            fetch_to_path(url, &cert_path).await?;
            let fingerprint = sha256_file(&cert_path).await?;
            if let Some(expected) = &artifact.secure_boot_cert_fingerprint {
                ensure_expected_fingerprint(&fingerprint, expected)
                    .map_err(ApiError::ValidationError)?;
            }
            (Some(cert_path), Some(fingerprint))
        }
        None => (None, None),
    };

    Ok(StagedArtifact {
        _workspace: workspace,
        package_path,
        package_sha256: artifact.sha256.to_lowercase(),
        package_name: artifact.package_name.clone(),
        expected_kernel_release: artifact.expected_kernel_release.clone(),
        cert_path,
        cert_fingerprint,
    })
}

async fn deploy_to_host(
    state: &Arc<AppState>,
    runtime: &Arc<crate::api::state::KernelJobRuntime>,
    job_id: Uuid,
    host: &KernelDeploymentHost,
    artifact: &StagedArtifact,
    reboot_policy: KernelDeploymentRebootPolicy,
) -> Result<HostDeploymentOutcome, HostDeploymentError> {
    runtime
        .set_stage(KernelDeploymentStage::Preflight, Some(host.name.clone()))
        .await;
    log_job(state, job_id, &format!("[{}] Connecting", host.name));

    let connection = build_connection_for_host(host)
        .await
        .map_err(|err| HostDeploymentError::Unreachable(err.to_string()))?;

    let facts = collect_preflight_facts(connection.as_ref(), host)
        .await
        .map_err(HostDeploymentError::Failed)?;
    log_job(
        state,
        job_id,
        &format!(
            "[{}] Preflight: distro={} {} arch={} bootloader={:?} current_kernel={} package={}",
            host.name,
            facts.distro,
            facts.distro_version,
            facts.arch,
            facts.bootloader,
            facts.current_kernel,
            facts.current_kernel_package.as_deref().unwrap_or("unknown")
        ),
    );

    if facts.distro != "ubuntu" && facts.distro != "debian" {
        return Err(HostDeploymentError::Failed(format!(
            "unsupported distro '{}'; only ubuntu and debian are supported",
            facts.distro
        )));
    }
    if facts.arch != "x86_64" && facts.arch != "amd64" {
        return Err(HostDeploymentError::Failed(format!(
            "unsupported architecture '{}'; only x86_64 is supported",
            facts.arch
        )));
    }
    if facts.boot_space_kib < MIN_BOOT_SPACE_KIB {
        return Err(HostDeploymentError::Failed(format!(
            "insufficient /boot space: {} KiB available",
            facts.boot_space_kib
        )));
    }

    let remote_package_path = PathBuf::from(format!(
        "/var/tmp/rustible-kernel-{}-{}.deb",
        job_id, host.name
    ));
    upload_verified_artifact(connection.as_ref(), artifact, &remote_package_path)
        .await
        .map_err(HostDeploymentError::Failed)?;

    let remote_cert_path = if let Some(cert_path) = artifact.cert_path.as_ref() {
        let remote_cert_path = PathBuf::from(format!(
            "/var/tmp/rustible-mok-{}-{}.der",
            job_id, host.name
        ));
        upload_file(connection.as_ref(), cert_path, &remote_cert_path)
            .await
            .map_err(HostDeploymentError::Failed)?;
        Some(remote_cert_path)
    } else {
        None
    };

    handle_secure_boot_preflight(
        state,
        runtime,
        job_id,
        host,
        connection.as_ref(),
        &facts,
        artifact,
        remote_cert_path.as_deref(),
    )
    .await?;

    runtime
        .set_stage(KernelDeploymentStage::Installing, Some(host.name.clone()))
        .await;
    run_command(
        connection.as_ref(),
        &format!(
            "DEBIAN_FRONTEND=noninteractive apt-get install -y {pkg} || (dpkg -i {pkg} && DEBIAN_FRONTEND=noninteractive apt-get -f install -y)",
            pkg = shell_quote(remote_package_path.to_string_lossy().as_ref())
        ),
        Some(build_execute_options(host, 900)),
    )
    .await
    .map_err(|err| HostDeploymentError::Failed(err.to_string()))?;

    run_command(
        connection.as_ref(),
        "bash -lc 'command -v update-initramfs >/dev/null 2>&1 && update-initramfs -u || true; command -v update-grub >/dev/null 2>&1 && update-grub || true; command -v bootctl >/dev/null 2>&1 && bootctl update || true'",
        Some(build_execute_options(host, 300)),
    )
    .await
    .map_err(|err| HostDeploymentError::Failed(err.to_string()))?;

    if reboot_policy == KernelDeploymentRebootPolicy::Skip {
        let running_kernel = run_command(
            connection.as_ref(),
            "uname -r",
            Some(build_execute_options(host, 60)),
        )
        .await
        .map_err(|err| HostDeploymentError::Failed(err.to_string()))?
        .stdout
        .trim()
        .to_string();

        if running_kernel != artifact.expected_kernel_release {
            return Err(HostDeploymentError::Failed(format!(
                "kernel package installed but reboot_policy=skip and running kernel is still {}",
                running_kernel
            )));
        }

        return Ok(HostDeploymentOutcome::Changed);
    }

    let boot_plan = plan_boot_entry(
        connection.as_ref(),
        host,
        facts.bootloader,
        &artifact.expected_kernel_release,
    )
    .await
    .map_err(HostDeploymentError::Failed)?;

    configure_one_shot_boot(connection.as_ref(), host, &boot_plan)
        .await
        .map_err(HostDeploymentError::Failed)?;

    runtime
        .set_stage(KernelDeploymentStage::Rebooting, Some(host.name.clone()))
        .await;
    reboot_host(connection.as_ref(), host)
        .await
        .map_err(HostDeploymentError::Failed)?;

    tokio::time::sleep(Duration::from_secs(5)).await;
    let reconnected = match wait_for_reboot(host, HOST_REBOOT_WAIT_SECS).await {
        Ok(conn) => conn,
        Err(err) => {
            rollback_after_failed_test_boot(
                state,
                runtime,
                job_id,
                host,
                artifact,
                &facts.current_kernel,
                Some(&boot_plan),
            )
            .await?;
            return Err(err);
        }
    };

    runtime
        .set_stage(KernelDeploymentStage::Verifying, Some(host.name.clone()))
        .await;
    let running_kernel = run_command(
        reconnected.as_ref(),
        "uname -r",
        Some(build_execute_options(host, 120)),
    )
    .await
    .map_err(|err| HostDeploymentError::Failed(err.to_string()))?
    .stdout
    .trim()
    .to_string();

    if running_kernel != artifact.expected_kernel_release {
        rollback_after_failed_test_boot(
            state,
            runtime,
            job_id,
            host,
            artifact,
            &facts.current_kernel,
            Some(&boot_plan),
        )
        .await?;
        return Err(HostDeploymentError::Failed(format!(
            "expected kernel {}, got {} after reboot",
            artifact.expected_kernel_release, running_kernel
        )));
    }

    runtime
        .set_stage(KernelDeploymentStage::Committing, Some(host.name.clone()))
        .await;
    commit_boot_entry(reconnected.as_ref(), host, &boot_plan)
        .await
        .map_err(HostDeploymentError::Failed)?;

    log_job(
        state,
        job_id,
        &format!(
            "[{}] Verified and committed kernel {}",
            host.name, running_kernel
        ),
    );

    Ok(HostDeploymentOutcome::Changed)
}

async fn collect_preflight_facts(
    connection: &dyn Connection,
    host: &KernelDeploymentHost,
) -> Result<PreflightFacts, String> {
    let exec_options = build_execute_options(host, 120);

    let current_kernel = run_command(connection, "uname -r", Some(exec_options.clone()))
        .await
        .map_err(|err| err.to_string())?
        .stdout
        .trim()
        .to_string();

    let distro = run_command(
        connection,
        r#"bash -lc '. /etc/os-release >/dev/null 2>&1 && printf "%s\n" "${ID:-unknown}"'"#,
        Some(exec_options.clone()),
    )
    .await
    .map_err(|err| err.to_string())?
    .stdout
    .trim()
    .to_lowercase();

    let distro_version = run_command(
        connection,
        r#"bash -lc '. /etc/os-release >/dev/null 2>&1 && printf "%s\n" "${VERSION_ID:-unknown}"'"#,
        Some(exec_options.clone()),
    )
    .await
    .map_err(|err| err.to_string())?
    .stdout
    .trim()
    .to_string();

    let arch = run_command(
        connection,
        r#"bash -lc 'dpkg --print-architecture 2>/dev/null || uname -m'"#,
        Some(exec_options.clone()),
    )
    .await
    .map_err(|err| err.to_string())?
    .stdout
    .trim()
    .to_lowercase();

    let boot_space_kib = run_command(
        connection,
        r#"bash -lc '(df -Pk /boot 2>/dev/null || df -Pk /) | awk "NR==2 {print \$4}"'"#,
        Some(exec_options.clone()),
    )
    .await
    .map_err(|err| err.to_string())?
    .stdout
    .trim()
    .parse::<u64>()
    .unwrap_or_default();

    let secure_boot_state = run_command(
        connection,
        r#"bash -lc 'command -v mokutil >/dev/null 2>&1 && mokutil --sb-state || echo unavailable'"#,
        Some(exec_options.clone()),
    )
    .await
    .map_err(|err| err.to_string())?
    .stdout;
    let secure_boot_enabled = parse_secure_boot_state(&secure_boot_state);

    let bootloader = detect_bootloader(connection, host, Some(exec_options.clone())).await?;

    let current_kernel_package = run_command(
        connection,
        r#"bash -lc 'dpkg-query -S /boot/vmlinuz-$(uname -r) 2>/dev/null | head -n1 | cut -d: -f1 || true'"#,
        Some(exec_options),
    )
    .await
    .map_err(|err| err.to_string())?
    .stdout
    .trim()
    .to_string();

    Ok(PreflightFacts {
        distro,
        distro_version,
        arch,
        current_kernel,
        current_kernel_package: if current_kernel_package.is_empty() {
            None
        } else {
            Some(current_kernel_package)
        },
        boot_space_kib,
        secure_boot_enabled,
        bootloader,
    })
}

async fn handle_secure_boot_preflight(
    state: &Arc<AppState>,
    runtime: &Arc<crate::api::state::KernelJobRuntime>,
    job_id: Uuid,
    host: &KernelDeploymentHost,
    connection: &dyn Connection,
    facts: &PreflightFacts,
    artifact: &StagedArtifact,
    remote_cert_path: Option<&Path>,
) -> Result<(), HostDeploymentError> {
    if !facts.secure_boot_enabled {
        return Ok(());
    }

    let remote_cert_path = remote_cert_path.ok_or_else(|| {
        HostDeploymentError::Failed(
            "secure boot is enabled but no secure boot certificate was supplied".to_string(),
        )
    })?;

    let test_key = run_command(
        connection,
        &format!(
            "mokutil --test-key {}",
            shell_quote(remote_cert_path.to_string_lossy().as_ref())
        ),
        Some(build_execute_options(host, 60)),
    )
    .await;
    let already_enrolled = test_key.is_ok();
    if already_enrolled {
        log_job(
            state,
            job_id,
            &format!("[{}] Secure Boot certificate already enrolled", host.name),
        );
        return Ok(());
    }

    match host.secure_boot_mode {
        KernelSecureBootMode::Disabled => Err(HostDeploymentError::Failed(
            "secure boot is enabled on the host but the host policy is disabled".to_string(),
        )),
        KernelSecureBootMode::PreEnrolled => Err(HostDeploymentError::Failed(
            "secure boot certificate is not enrolled on the host".to_string(),
        )),
        KernelSecureBootMode::ConsoleBmc => {
            let bmc = host.bmc.as_ref().ok_or_else(|| {
                HostDeploymentError::Failed(
                    "secure boot certificate enrollment requires BMC metadata".to_string(),
                )
            })?;

            if bmc.username.as_deref().unwrap_or_default().is_empty()
                || bmc.password.as_deref().unwrap_or_default().is_empty()
            {
                return Err(HostDeploymentError::Failed(
                    "secure boot certificate enrollment requires BMC credentials".to_string(),
                ));
            }

            run_command(
                connection,
                &format!(
                    "mokutil --root-pw --import {}",
                    shell_quote(remote_cert_path.to_string_lossy().as_ref())
                ),
                Some(build_execute_options(host, 120)),
            )
            .await
            .map_err(|err| {
                HostDeploymentError::Failed(format!(
                    "failed to stage secure boot certificate enrollment: {}",
                    err
                ))
            })?;

            power_cycle_bmc(bmc).await.map_err(|err| {
                HostDeploymentError::Failed(format!(
                    "failed to power-cycle BMC for secure boot enrollment: {}",
                    err
                ))
            })?;

            let action_id = format!("{}:{}", host.name, Utc::now().timestamp());
            let instructions = vec![
                format!(
                    "Open the {} console at {} and complete the MOK enrollment flow.",
                    bmc.provider_string(),
                    bmc.endpoint
                ),
                "Choose 'Enroll MOK', approve the staged certificate, and continue booting.".to_string(),
                "After the host is back online, call the resume endpoint with the returned action_id.".to_string(),
            ];
            let action = KernelDeploymentActionRequired {
                action_id: action_id.clone(),
                kind: KernelDeploymentActionKind::SecureBootEnrollment,
                host: host.name.clone(),
                message: format!(
                    "Secure Boot certificate enrollment is required on {}",
                    host.name
                ),
                instructions,
                bmc: Some(KernelDeploymentBmcActionHint {
                    provider: bmc.provider,
                    endpoint: bmc.endpoint.clone(),
                }),
            };
            runtime.set_action_required(action.clone()).await;
            state.update_job_status(job_id, JobStatus::ActionRequired);
            log_job(
                state,
                job_id,
                &format!(
                    "[{}] Waiting for operator to complete Secure Boot enrollment (action_id={})",
                    host.name, action_id
                ),
            );

            runtime.resume_notify.notified().await;
            if matches!(
                state.get_job(job_id).map(|job| job.status),
                Some(JobStatus::Cancelled)
            ) {
                return Err(HostDeploymentError::Failed("job was cancelled".to_string()));
            }

            let reconnected = wait_for_reboot(host, HOST_REBOOT_WAIT_SECS).await?;
            let post_resume = run_command(
                reconnected.as_ref(),
                &format!(
                    "mokutil --test-key {}",
                    shell_quote(remote_cert_path.to_string_lossy().as_ref())
                ),
                Some(build_execute_options(host, 60)),
            )
            .await;

            if post_resume.is_err() {
                return Err(HostDeploymentError::Failed(format!(
                    "secure boot certificate enrollment was not detected after resume (fingerprint={})",
                    artifact.cert_fingerprint.as_deref().unwrap_or("unknown")
                )));
            }

            state.update_job_status(job_id, JobStatus::Running);
            Ok(())
        }
    }
}

async fn plan_boot_entry(
    connection: &dyn Connection,
    host: &KernelDeploymentHost,
    bootloader: KernelBootloader,
    expected_kernel_release: &str,
) -> Result<BootPlan, String> {
    let exec_options = build_execute_options(host, 60);
    let entry_id = match bootloader {
        KernelBootloader::Grub => run_command(
            connection,
            &format!(
                r#"bash -lc 'awk -F"'"'"'" '/menuentry / && index($0, {release}) {{print $2; exit}}' /boot/grub/grub.cfg /boot/grub2/grub.cfg 2>/dev/null'"#,
                release = shell_quote(expected_kernel_release)
            ),
            Some(exec_options),
        )
        .await
        .map_err(|err| err.to_string())?
        .stdout
        .trim()
        .to_string(),
        KernelBootloader::SystemdBoot => run_command(
            connection,
            &format!(
                r#"bash -lc 'for base in /boot/loader/entries /boot/efi/loader/entries /efi/loader/entries; do for f in "$base"/*.conf; do [ -f "$f" ] || continue; if grep -qi -- {release} "$f"; then basename "$f" .conf; exit 0; fi; done; done'"#,
                release = shell_quote(expected_kernel_release)
            ),
            Some(exec_options),
        )
        .await
        .map_err(|err| err.to_string())?
        .stdout
        .trim()
        .to_string(),
    };

    if entry_id.is_empty() {
        return Err(format!(
            "failed to locate boot entry for kernel {}",
            expected_kernel_release
        ));
    }

    Ok(BootPlan {
        bootloader,
        entry_id,
    })
}

async fn configure_one_shot_boot(
    connection: &dyn Connection,
    host: &KernelDeploymentHost,
    boot_plan: &BootPlan,
) -> Result<(), String> {
    let command = match boot_plan.bootloader {
        KernelBootloader::Grub => format!(
            "bash -lc 'grub-reboot {entry} || grub2-reboot {entry}'",
            entry = shell_quote(&boot_plan.entry_id)
        ),
        KernelBootloader::SystemdBoot => {
            format!("bootctl set-oneshot {}", shell_quote(&boot_plan.entry_id))
        }
    };

    run_command(connection, &command, Some(build_execute_options(host, 120)))
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

async fn commit_boot_entry(
    connection: &dyn Connection,
    host: &KernelDeploymentHost,
    boot_plan: &BootPlan,
) -> Result<(), String> {
    let command = match boot_plan.bootloader {
        KernelBootloader::Grub => format!(
            "bash -lc 'grub-set-default {entry} || grub2-set-default {entry}'",
            entry = shell_quote(&boot_plan.entry_id)
        ),
        KernelBootloader::SystemdBoot => {
            format!("bootctl set-default {}", shell_quote(&boot_plan.entry_id))
        }
    };

    run_command(connection, &command, Some(build_execute_options(host, 120)))
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

async fn rollback_after_failed_test_boot(
    state: &Arc<AppState>,
    runtime: &Arc<crate::api::state::KernelJobRuntime>,
    job_id: Uuid,
    host: &KernelDeploymentHost,
    artifact: &StagedArtifact,
    previous_kernel: &str,
    boot_plan: Option<&BootPlan>,
) -> Result<(), HostDeploymentError> {
    runtime
        .set_stage(KernelDeploymentStage::RollingBack, Some(host.name.clone()))
        .await;
    log_job(
        state,
        job_id,
        &format!("[{}] Rolling back to previous kernel", host.name),
    );

    if let Some(bmc) = host.bmc.as_ref() {
        if let Err(err) = power_cycle_bmc(bmc).await {
            warn!(
                "failed to power-cycle BMC during rollback for {}: {}",
                host.name, err
            );
        }
    }

    let recovered = wait_for_reboot(host, HOST_REBOOT_WAIT_SECS).await?;
    let running_kernel = run_command(
        recovered.as_ref(),
        "uname -r",
        Some(build_execute_options(host, 120)),
    )
    .await
    .map_err(|err| HostDeploymentError::Failed(err.to_string()))?
    .stdout
    .trim()
    .to_string();

    if running_kernel != previous_kernel {
        return Err(HostDeploymentError::Failed(format!(
            "rollback did not restore the previous kernel; expected {}, got {}",
            previous_kernel, running_kernel
        )));
    }

    if let Some(plan) = boot_plan {
        if matches!(plan.bootloader, KernelBootloader::Grub) {
            let _ = run_command(
                recovered.as_ref(),
                "bash -lc 'grub-editenv - unset next_entry || true'",
                Some(build_execute_options(host, 60)),
            )
            .await;
        }
    }

    run_command(
        recovered.as_ref(),
        &format!(
            "DEBIAN_FRONTEND=noninteractive apt-get remove -y {pkg} || true; command -v update-initramfs >/dev/null 2>&1 && update-initramfs -u || true; command -v update-grub >/dev/null 2>&1 && update-grub || true; command -v bootctl >/dev/null 2>&1 && bootctl update || true",
            pkg = shell_quote(&artifact.package_name)
        ),
        Some(build_execute_options(host, 300)),
    )
    .await
    .map_err(|err| HostDeploymentError::Failed(err.to_string()))?;

    Ok(())
}

async fn upload_verified_artifact(
    connection: &dyn Connection,
    artifact: &StagedArtifact,
    remote_package_path: &Path,
) -> Result<(), String> {
    upload_file(connection, &artifact.package_path, remote_package_path).await?;
    let checksum = run_command(
        connection,
        &format!(
            "sha256sum {} | awk '{{print $1}}'",
            shell_quote(remote_package_path.to_string_lossy().as_ref())
        ),
        None,
    )
    .await
    .map_err(|err| err.to_string())?
    .stdout
    .trim()
    .to_lowercase();

    if checksum != artifact.package_sha256 {
        return Err(format!(
            "copied package checksum mismatch: expected {}, got {}",
            artifact.package_sha256, checksum
        ));
    }

    Ok(())
}

async fn upload_file(
    connection: &dyn Connection,
    local_path: &Path,
    remote_path: &Path,
) -> Result<(), String> {
    connection
        .upload(
            local_path,
            remote_path,
            Some(TransferOptions::new().with_create_dirs()),
        )
        .await
        .map_err(|err| err.to_string())
}

async fn detect_bootloader(
    connection: &dyn Connection,
    host: &KernelDeploymentHost,
    options: Option<ExecuteOptions>,
) -> Result<KernelBootloader, String> {
    let observed = run_command(
        connection,
        r#"bash -lc 'if command -v bootctl >/dev/null 2>&1 && bootctl status >/dev/null 2>&1; then echo systemd_boot; elif command -v grub-reboot >/dev/null 2>&1 || command -v grub2-reboot >/dev/null 2>&1; then echo grub; else echo unknown; fi'"#,
        options,
    )
    .await
    .map_err(|err| err.to_string())?
    .stdout
    .trim()
    .to_string();

    resolve_bootloader(host.bootloader_hint, observed.as_str()).ok_or_else(|| {
        format!(
            "failed to determine bootloader for {} (hint={:?}, observed={})",
            host.name, host.bootloader_hint, observed
        )
    })
}

async fn reboot_host(
    connection: &dyn Connection,
    host: &KernelDeploymentHost,
) -> Result<(), String> {
    run_command(
        connection,
        "bash -lc 'nohup sh -c \"sleep 2; systemctl reboot || reboot\" >/dev/null 2>&1 &'",
        Some(build_execute_options(host, 30)),
    )
    .await
    .map(|_| ())
    .map_err(|err| err.to_string())
}

async fn wait_for_reboot(
    host: &KernelDeploymentHost,
    timeout_secs: u64,
) -> Result<Arc<dyn Connection + Send + Sync>, HostDeploymentError> {
    let started = tokio::time::Instant::now();
    let mut last_error = String::new();

    while started.elapsed() < Duration::from_secs(timeout_secs) {
        match build_connection_for_host(host).await {
            Ok(connection) => {
                match connection
                    .execute(
                        "uname -r",
                        Some(build_execute_options(host, HOST_CONNECT_TIMEOUT_SECS)),
                    )
                    .await
                {
                    Ok(CommandResult { success: true, .. }) => return Ok(connection),
                    Ok(result) => last_error = result.combined_output(),
                    Err(err) => last_error = err.to_string(),
                }
            }
            Err(err) => last_error = err.to_string(),
        }

        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    Err(HostDeploymentError::Unreachable(format!(
        "host did not come back after reboot: {}",
        last_error
    )))
}

async fn build_connection_for_host(
    host: &KernelDeploymentHost,
) -> Result<Arc<dyn Connection + Send + Sync>, crate::connection::ConnectionError> {
    let mut builder = ConnectionBuilder::new(host.address.clone())
        .port(host.port)
        .user(host.username.clone())
        .timeout(HOST_CONNECT_TIMEOUT_SECS);

    if let Some(password) = &host.password {
        builder = builder.password(password.clone());
    }

    let _key_dir;
    if let Some(private_key) = host.private_key.as_deref() {
        let key_dir = TempDir::new().map_err(|err| {
            crate::connection::ConnectionError::InvalidConfig(format!(
                "failed to create temporary key directory: {}",
                err
            ))
        })?;
        let key_path = key_dir.path().join("id_key");
        std::fs::write(&key_path, private_key).map_err(|err| {
            crate::connection::ConnectionError::InvalidConfig(format!(
                "failed to write temporary private key: {}",
                err
            ))
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&key_path, perms).map_err(|err| {
                crate::connection::ConnectionError::InvalidConfig(format!(
                    "failed to secure temporary private key: {}",
                    err
                ))
            })?;
        }
        builder = builder.private_key(key_path.to_string_lossy().to_string());
        _key_dir = Some(key_dir);
    } else {
        _key_dir = None::<TempDir>;
    }

    builder.connect().await
}

fn build_execute_options(host: &KernelDeploymentHost, timeout_secs: u64) -> ExecuteOptions {
    let mut options = ExecuteOptions::new().with_timeout(timeout_secs);
    if host.sudo_enabled {
        options = options
            .with_escalation(Some("root".to_string()))
            .with_escalate_method("sudo".to_string());
        if let Some(password) = &host.password {
            options = options.with_escalate_password(password.clone());
        }
    }
    options
}

async fn run_command(
    connection: &dyn Connection,
    command: &str,
    options: Option<ExecuteOptions>,
) -> Result<CommandResult, ApiError> {
    let result = connection
        .execute(command, options)
        .await
        .map_err(|err| ApiError::JobExecution(err.to_string()))?;
    if result.success {
        Ok(result)
    } else {
        Err(ApiError::JobExecution(result.combined_output()))
    }
}

async fn fetch_to_path(url: &str, destination: &Path) -> ApiResult<()> {
    if let Some(path) = url.strip_prefix("file://") {
        fs::copy(path, destination).await?;
        return Ok(());
    }

    if url.starts_with('/') {
        fs::copy(url, destination).await?;
        return Ok(());
    }

    let client = Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| ApiError::Internal(format!("failed to download {}: {}", url, err)))?
        .error_for_status()
        .map_err(|err| ApiError::Internal(format!("failed to download {}: {}", url, err)))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|err| ApiError::Internal(format!("failed to read {}: {}", url, err)))?;
    fs::write(destination, &bytes).await?;
    Ok(())
}

async fn verify_sha256(path: &Path, expected: &str) -> ApiResult<()> {
    let observed = sha256_file(path).await?;
    ensure_expected_fingerprint(&observed, expected)
        .map_err(|err| ApiError::ValidationError(format!("checksum mismatch: {}", err)))
}

async fn sha256_file(path: &Path) -> ApiResult<String> {
    let bytes = fs::read(path).await?;
    let digest = Sha256::digest(bytes);
    Ok(format!("{:x}", digest))
}

async fn verify_cosign_blob(
    package_path: &Path,
    signature_path: &Path,
    public_key_path: &Path,
) -> ApiResult<()> {
    let output = Command::new("cosign")
        .arg("verify-blob")
        .arg("--key")
        .arg(public_key_path)
        .arg("--signature")
        .arg(signature_path)
        .arg(package_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| ApiError::Internal(format!("failed to execute cosign: {}", err)))?;

    if output.status.success() {
        return Ok(());
    }

    Err(ApiError::ValidationError(format!(
        "cosign verify-blob failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

fn ensure_expected_fingerprint(observed: &str, expected: &str) -> Result<(), String> {
    let normalized_observed = normalize_sha256(observed);
    let normalized_expected = normalize_sha256(expected);

    if normalized_observed == normalized_expected {
        Ok(())
    } else {
        Err(format!(
            "expected {}, got {}",
            normalized_expected, normalized_observed
        ))
    }
}

fn normalize_sha256(value: &str) -> String {
    value
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(value.trim())
        .to_lowercase()
}

fn parse_secure_boot_state(output: &str) -> bool {
    let lower = output.to_lowercase();
    lower.contains("secureboot enabled") || lower.contains("secure boot enabled")
}

fn resolve_bootloader(hint: Option<KernelBootloader>, observed: &str) -> Option<KernelBootloader> {
    let observed = match observed.trim() {
        "grub" => Some(KernelBootloader::Grub),
        "systemd_boot" => Some(KernelBootloader::SystemdBoot),
        _ => None,
    };

    match (hint, observed) {
        (Some(KernelBootloader::Grub), Some(KernelBootloader::SystemdBoot)) => {
            Some(KernelBootloader::Grub)
        }
        (Some(KernelBootloader::SystemdBoot), Some(KernelBootloader::Grub)) => {
            Some(KernelBootloader::SystemdBoot)
        }
        (Some(hint), _) => Some(hint),
        (None, observed) => observed,
    }
}

async fn power_cycle_bmc(bmc: &KernelDeploymentBmc) -> Result<(), String> {
    match bmc.provider {
        KernelDeploymentBmcProvider::Redfish => {
            if let Err(err) = redfish_power_cycle(bmc).await {
                warn!("redfish power cycle failed for {}: {}", bmc.endpoint, err);
                ipmi_power_cycle(bmc).await
            } else {
                Ok(())
            }
        }
        KernelDeploymentBmcProvider::Ipmi => ipmi_power_cycle(bmc).await,
    }
}

async fn redfish_power_cycle(bmc: &KernelDeploymentBmc) -> Result<(), String> {
    let username = bmc
        .username
        .as_deref()
        .ok_or_else(|| "missing BMC username".to_string())?;
    let password = bmc
        .password
        .as_deref()
        .ok_or_else(|| "missing BMC password".to_string())?;

    let endpoint = bmc.endpoint.trim_end_matches('/');
    let client = Client::builder()
        .danger_accept_invalid_certs(!bmc.verify_tls)
        .build()
        .map_err(|err| format!("failed to create Redfish client: {}", err))?;

    let systems_url = format!("{}/redfish/v1/Systems", endpoint);
    let systems = client
        .get(&systems_url)
        .basic_auth(username, Some(password))
        .send()
        .await
        .map_err(|err| format!("failed to query Redfish systems: {}", err))?
        .error_for_status()
        .map_err(|err| format!("failed to query Redfish systems: {}", err))?;

    let payload: serde_json::Value = systems
        .json()
        .await
        .map_err(|err| format!("failed to parse Redfish systems response: {}", err))?;
    let system_path = payload["Members"]
        .as_array()
        .and_then(|members| members.first())
        .and_then(|member| member["@odata.id"].as_str())
        .unwrap_or("/redfish/v1/Systems/1");
    let reset_url = format!(
        "{}/Actions/ComputerSystem.Reset",
        endpoint.to_string() + system_path
    );

    client
        .post(&reset_url)
        .basic_auth(username, Some(password))
        .json(&json!({ "ResetType": "PowerCycle" }))
        .send()
        .await
        .map_err(|err| format!("failed to invoke Redfish reset: {}", err))?
        .error_for_status()
        .map_err(|err| format!("failed to invoke Redfish reset: {}", err))?;
    Ok(())
}

async fn ipmi_power_cycle(bmc: &KernelDeploymentBmc) -> Result<(), String> {
    let username = bmc
        .username
        .as_deref()
        .ok_or_else(|| "missing BMC username".to_string())?;
    let password = bmc
        .password
        .as_deref()
        .ok_or_else(|| "missing BMC password".to_string())?;

    let output = Command::new("ipmitool")
        .arg("-I")
        .arg("lanplus")
        .arg("-H")
        .arg(&bmc.endpoint)
        .arg("-U")
        .arg(username)
        .arg("-P")
        .arg(password)
        .arg("chassis")
        .arg("power")
        .arg("cycle")
        .output()
        .await
        .map_err(|err| format!("failed to execute ipmitool: {}", err))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn log_job(state: &Arc<AppState>, job_id: Uuid, message: &str) {
    state.append_job_output(job_id, message.to_string(), "stdout");
}

async fn fail_job(state: &Arc<AppState>, job_id: Uuid, message: String) {
    state.append_job_output(job_id, message.clone(), "stderr");
    state.set_job_error(job_id, message.clone());
    if let Some(runtime) = state.get_kernel_job_runtime(job_id) {
        runtime.set_stage(KernelDeploymentStage::Failed, None).await;
    }
    state.update_job_status(job_id, JobStatus::Failed);
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

trait BmcProviderDisplay {
    fn provider_string(&self) -> &'static str;
}

impl BmcProviderDisplay for KernelDeploymentBmc {
    fn provider_string(&self) -> &'static str {
        match self.provider {
            KernelDeploymentBmcProvider::Redfish => "Redfish",
            KernelDeploymentBmcProvider::Ipmi => "IPMI",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_request_deserializes_portable_shape() {
        let request: KernelDeploymentRequest = serde_json::from_value(json!({
            "hosts": [
                {
                    "name": "vm-1",
                    "address": "192.0.2.10",
                    "port": 2222,
                    "username": "ubuntu",
                    "password": "secret",
                    "sudo_enabled": true,
                    "secure_boot_mode": "console_bmc",
                    "bootloader_hint": "grub",
                    "bmc": {
                        "provider": "redfish",
                        "endpoint": "https://bmc.example.test",
                        "username": "admin",
                        "password": "bmc-secret"
                    }
                }
            ],
            "artifact": {
                "url": "https://artifacts.example.test/kernel.deb",
                "sha256": "abc123",
                "package_name": "linux-image-esse",
                "expected_kernel_release": "6.8.0-esse",
                "signature_url": "https://artifacts.example.test/kernel.sig",
                "public_key_url": "https://artifacts.example.test/cosign.pub",
                "public_key_fingerprint": "sha256:feedbeef",
                "secure_boot_cert_url": "https://artifacts.example.test/mok.der",
                "secure_boot_cert_fingerprint": "sha256:cafebabe"
            },
            "reboot_policy": "required"
        }))
        .unwrap();

        assert_eq!(request.hosts.len(), 1);
        assert_eq!(request.hosts[0].port, 2222);
        assert_eq!(
            request.hosts[0].bootloader_hint,
            Some(KernelBootloader::Grub)
        );
        assert_eq!(request.artifact.public_key_fingerprint, "sha256:feedbeef");
    }

    #[test]
    fn test_normalize_sha256_accepts_prefix() {
        assert_eq!(normalize_sha256("sha256:ABCDEF"), "abcdef");
        assert_eq!(normalize_sha256("ABCDEF"), "abcdef");
    }

    #[test]
    fn test_parse_secure_boot_state() {
        assert!(parse_secure_boot_state("SecureBoot enabled"));
        assert!(!parse_secure_boot_state("SecureBoot disabled"));
    }

    #[test]
    fn test_resolve_bootloader_prefers_explicit_hint() {
        assert_eq!(
            resolve_bootloader(Some(KernelBootloader::Grub), "systemd_boot"),
            Some(KernelBootloader::Grub)
        );
        assert_eq!(
            resolve_bootloader(None, "systemd_boot"),
            Some(KernelBootloader::SystemdBoot)
        );
        assert_eq!(resolve_bootloader(None, "unknown"), None);
    }
}
