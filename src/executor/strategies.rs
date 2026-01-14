use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::future::join_all;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::executor::runtime::ExecutionContext;
use crate::recovery::TransactionId;

use super::results::update_stats;
use super::task::{Task, TaskResult, TaskStatus};
use super::{
    ExecutionEvent, ExecutionStats, ExecutionStrategy, Executor, ExecutorError, ExecutorResult,
    HostResult,
};

impl Executor {
    /// Run tasks in linear strategy (all hosts per task before next task)
    pub(super) async fn run_linear(
        &self,
        hosts: &[String],
        tasks: &[Task],
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        use crate::executor::task::BlockRole;

        // Pre-allocate HashMaps with known capacity
        let host_count = hosts.len();
        let mut results: HashMap<String, HostResult> = HashMap::with_capacity(host_count);
        for h in hosts {
            results.insert(
                h.clone(),
                HostResult {
                    host: h.clone(),
                    stats: ExecutionStats::default(),
                    failed: false,
                    unreachable: false,
                },
            );
        }

        // Track which blocks have failed (per host) - pre-allocate with capacity
        let mut failed_blocks: HashMap<String, HashSet<String>> =
            HashMap::with_capacity(host_count);
        for h in hosts {
            failed_blocks.insert(h.clone(), HashSet::new());
        }
        // Track which blocks have had their rescue tasks run
        let mut rescued_blocks: HashMap<String, HashSet<String>> =
            HashMap::with_capacity(host_count);
        for h in hosts {
            rescued_blocks.insert(h.clone(), HashSet::new());
        }

        for task in tasks {
            if let Some(cb) = &self.event_callback {
                cb(ExecutionEvent::TaskStart(task.name.clone()));
            }

            // Determine which hosts should run this task based on block state
            let active_hosts: Vec<_> = hosts
                .iter()
                .filter(|h| {
                    let host_result = results.get(*h);
                    let host_failed_blocks = failed_blocks.get(*h);
                    let host_rescued_blocks = rescued_blocks.get(*h);

                    // Skip if host has failed (and not in a block)
                    if host_result
                        .map(|r| r.failed || r.unreachable)
                        .unwrap_or(false)
                    {
                        // But still run always tasks
                        if task.block_role == BlockRole::Always {
                            return true;
                        }
                        return false;
                    }

                    // Handle block-specific logic
                    if let Some(ref block_id) = task.block_id {
                        let block_failed = host_failed_blocks
                            .map(|blocks| blocks.contains(block_id))
                            .unwrap_or(false);
                        let block_rescued = host_rescued_blocks
                            .map(|blocks| blocks.contains(block_id))
                            .unwrap_or(false);

                        match task.block_role {
                            BlockRole::Normal => {
                                // Skip normal tasks if block has failed
                                !block_failed
                            }
                            BlockRole::Rescue => {
                                // Run rescue tasks only if block failed and hasn't been rescued yet
                                block_failed && !block_rescued
                            }
                            BlockRole::Always => {
                                // Always run always tasks
                                true
                            }
                        }
                    } else {
                        true
                    }
                })
                .cloned()
                .collect();

            if active_hosts.is_empty() {
                // Check if all tasks remaining are block-related
                if task.block_id.is_none() {
                    warn!("All hosts have failed, stopping execution");
                    break;
                }
                continue;
            }

            // Run task on all active hosts in parallel (limited by semaphore)
            let task_results = self
                .run_task_on_hosts(&active_hosts, task, tx_id.clone())
                .await?;

            debug!(
                "Task '{}' completed on {} hosts",
                task.name,
                task_results.len()
            );

            // Update results and track block failures
            for (host, task_result) in task_results {
                debug!(
                    "  Host '{}': status={:?}, changed={}, msg={:?}",
                    host, task_result.status, task_result.changed, task_result.msg
                );

                if let Some(host_result) = results.get_mut(&host) {
                    // Check if this task failed
                    let task_failed =
                        task_result.status == crate::executor::task::TaskStatus::Failed;

                    // If it's a normal task in a block and it failed, mark the block as failed
                    if task_failed {
                        if let Some(ref block_id) = task.block_id {
                            if task.block_role == BlockRole::Normal {
                                if let Some(blocks) = failed_blocks.get_mut(&host) {
                                    blocks.insert(block_id.clone());
                                }
                                // Mark that rescue is needed - don't mark host as failed yet
                            }
                        }
                    }

                    // If this is a rescue task, mark the block as rescued
                    if task.block_role == BlockRole::Rescue {
                        if let Some(ref block_id) = task.block_id {
                            if let Some(blocks) = rescued_blocks.get_mut(&host) {
                                blocks.insert(block_id.clone());
                            }
                        }
                    }

                    // Update stats, but only mark host as failed if:
                    // - Task is not in a block, OR
                    // - Task is in a block but there's no rescue section (block failed without rescue)
                    let should_mark_failed = if task.block_id.is_some() {
                        // For block tasks, we handle failure differently
                        // The host only fails if rescue also fails
                        task.block_role == BlockRole::Rescue && task_failed
                    } else {
                        task_failed
                    };

                    // Temporarily modify result for stats update
                    let mut modified_result = task_result.clone();
                    if task.block_id.is_some()
                        && task.block_role == BlockRole::Normal
                        && task_failed
                    {
                        // Don't count normal block failure as host failure
                        modified_result.status = crate::executor::task::TaskStatus::Ok;
                    }

                    self.update_host_stats(host_result, &modified_result);

                    // Now set the actual failure state
                    if should_mark_failed && !task.ignore_errors {
                        host_result.failed = true;
                    }
                }
            }
        }

        // After all tasks, check if any blocks failed without being rescued
        for (host, host_failed_blocks) in &failed_blocks {
            if let Some(_host_result) = results.get_mut(host) {
                let host_rescued = rescued_blocks.get(host);
                for block_id in host_failed_blocks {
                    let was_rescued = host_rescued.map(|r| r.contains(block_id)).unwrap_or(false);
                    if !was_rescued {
                        // Block failed without rescue - this is a failure
                        // But we need to check if there was a rescue section defined
                        // For now, assume if rescue tasks were found, it was rescued
                        // If no rescue tasks exist, it's a real failure
                        // This is a simplification - proper implementation would track this differently
                    }
                }
            }
        }

        Ok(results)
    }

    /// Run tasks in free strategy (each host runs independently)
    ///
    /// OPTIMIZATION: Extract config values once instead of cloning config per host
    pub(super) async fn run_free(
        &self,
        hosts: &[String],
        tasks: &[Task],
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        // OPTIMIZATION: Fast path for single host
        if hosts.len() == 1 {
            let host = &hosts[0];
            let _permit = self.semaphore.acquire().await.unwrap();

            let mut host_result = HostResult {
                host: host.clone(),
                stats: ExecutionStats::default(),
                failed: false,
                unreachable: false,
            };

            for task in tasks {
                if host_result.failed || host_result.unreachable {
                    break;
                }

                // Apply become precedence: task > config (play-level handled separately)
                let effective_become = task.r#become || self.config.r#become;
                let effective_become_user = task
                    .become_user
                    .clone()
                    .unwrap_or_else(|| self.config.r#become_user.clone());

                let ctx = ExecutionContext::new(host.clone())
                    .with_check_mode(self.config.check_mode)
                    .with_diff_mode(self.config.diff_mode)
                    .with_verbosity(self.config.verbosity)
                    .with_become(effective_become)
                    .with_become_method(self.config.r#become_method.clone())
                    .with_become_user(effective_become_user)
                    .with_become_password(self.config.r#become_password.clone());

                let task_result = task
                    .execute(
                        &ctx,
                        &self.runtime,
                        &self.handlers,
                        &self.notified_handlers,
                        &self.parallelization_manager,
                        &self.module_registry,
                    )
                    .await;

                let task_result = match task_result {
                    Ok(result) => result,
                    Err(e) => TaskResult {
                        status: TaskStatus::Failed,
                        changed: false,
                        msg: Some(e.to_string()),
                        result: None,
                        diff: None,
                    },
                };
                Self::record_task_outcome(
                    self.recovery_manager.as_ref(),
                    tx_id.as_ref(),
                    &task.name,
                    host,
                    &task_result,
                )
                .await;

                apply_task_result(&mut host_result, &task_result);
            }

            let mut results = HashMap::with_capacity(1);
            results.insert(host.clone(), host_result);
            return Ok(results);
        }

        // OPTIMIZATION: Pre-extract config values to avoid cloning entire config per host
        let check_mode = self.config.check_mode;
        let diff_mode = self.config.diff_mode;
        let verbosity = self.config.verbosity;
        let config_become = self.config.r#become;
        let config_become_method = self.config.r#become_method.clone();
        let config_become_user = self.config.r#become_user.clone();
        let config_become_password = self.config.r#become_password.clone();

        let mut base_results = HashMap::with_capacity(hosts.len());
        let mut connections = HashMap::with_capacity(hosts.len());
        let mut python_interpreters = HashMap::with_capacity(hosts.len());

        for host in hosts {
            match self.get_connection_for_host(host).await {
                Ok(conn) => {
                    connections.insert(host.clone(), conn);
                    python_interpreters
                        .insert(host.clone(), self.get_python_interpreter(host).await);
                }
                Err(e) => {
                    base_results.insert(
                        host.clone(),
                        HostResult {
                            host: host.clone(),
                            stats: ExecutionStats {
                                unreachable: 1,
                                ..Default::default()
                            },
                            failed: false,
                            unreachable: true,
                        },
                    );
                    warn!("Host unreachable: {} ({})", host, e);
                }
            }
        }

        // Avoid cloning entire task list - use Arc slice instead
        let tasks: Arc<[Task]> = tasks.iter().cloned().collect::<Vec<_>>().into();
        let results = Arc::new(Mutex::new(base_results));

        let handles: Vec<_> = hosts
            .iter()
            .filter(|host| connections.contains_key(*host))
            .map(|host| {
                let host = host.clone();
                let tasks = Arc::clone(&tasks);
                let results = Arc::clone(&results);
                let semaphore = Arc::clone(&self.semaphore);
                let runtime = Arc::clone(&self.runtime);
                let handlers = Arc::clone(&self.handlers);
                let notified = Arc::clone(&self.notified_handlers);
                let parallelization_local = Arc::clone(&self.parallelization_manager);
                let module_registry = Arc::clone(&self.module_registry);
                let recovery_manager = self.recovery_manager.clone();
                let tx_id = tx_id.clone();
                let config_become = config_become;
                let config_become_method = config_become_method.clone();
                let config_become_user = config_become_user.clone();
                let config_become_password = config_become_password.clone();
                let connection = connections.get(&host).cloned();
                let python_interpreter = python_interpreters
                    .get(&host)
                    .cloned()
                    .unwrap_or_else(|| "/usr/bin/python3".to_string());
                let callback = self.event_callback.clone();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    let mut host_result = HostResult {
                        host: host.clone(),
                        stats: ExecutionStats::default(),
                        failed: false,
                        unreachable: false,
                    };

                    for task in tasks.iter() {
                        if host_result.failed || host_result.unreachable {
                            break;
                        }

                        // Apply become precedence: task > config
                        let effective_become = task.r#become || config_become;
                        let effective_become_user = task
                            .become_user
                            .clone()
                            .unwrap_or_else(|| config_become_user.clone());

                        let mut ctx = ExecutionContext::new(host.clone())
                            .with_check_mode(check_mode)
                            .with_diff_mode(diff_mode)
                            .with_verbosity(verbosity)
                            .with_become(effective_become)
                            .with_become_method(config_become_method.clone())
                            .with_become_user(effective_become_user)
                            .with_become_password(config_become_password.clone());

                        if let Some(conn) = connection.clone() {
                            ctx = ctx.with_connection(conn);
                        }
                        ctx = ctx.with_python_interpreter(python_interpreter.clone());

                        let task_result = task
                            .execute(
                                &ctx,
                                &runtime,
                                &handlers,
                                &notified,
                                &parallelization_local,
                                &module_registry,
                            )
                            .await;
                        let task_result = match task_result {
                            Ok(result) => result,
                            Err(e) => TaskResult {
                                status: TaskStatus::Failed,
                                changed: false,
                                msg: Some(e.to_string()),
                                result: None,
                                diff: None,
                            },
                        };

                        if let Some(cb) = &callback {
                            cb(ExecutionEvent::HostTaskComplete(
                                host.clone(),
                                task.name.clone(),
                                task_result.clone(),
                            ));
                        }

                        Self::record_task_outcome(
                            recovery_manager.as_ref(),
                            tx_id.as_ref(),
                            &task.name,
                            &host,
                            &task_result,
                        )
                        .await;

                        apply_task_result(&mut host_result, &task_result);
                    }

                    results.lock().await.insert(host, host_result);
                })
            })
            .collect();

        join_all(handles).await;

        let results = Arc::try_unwrap(results)
            .map_err(|_| ExecutorError::RuntimeError("Failed to unwrap results".into()))?
            .into_inner();

        Ok(results)
    }

    /// Run tasks in host_pinned strategy (dedicated worker per host)
    pub(super) async fn run_host_pinned(
        &self,
        hosts: &[String],
        tasks: &[Task],
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        // For now, host_pinned behaves like free strategy
        // In a full implementation, this would pin workers to specific hosts
        self.run_free(hosts, tasks, tx_id).await
    }

    /// Run tasks with serial batching
    pub(super) async fn run_serial(
        &self,
        serial_spec: &crate::playbook::SerialSpec,
        hosts: &[String],
        tasks: &[Task],
        max_fail_percentage: Option<u8>,
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, HostResult>> {
        info!(
            "Running with serial batching: {:?}, max_fail_percentage: {:?}",
            serial_spec, max_fail_percentage
        );

        // Split hosts into batches
        let batches = serial_spec.batch_hosts(hosts);

        if batches.is_empty() {
            return Ok(HashMap::new());
        }

        debug!("Created {} batches for serial execution", batches.len());

        let mut all_results: HashMap<String, HostResult> = HashMap::new();
        let mut total_failed = 0;
        let total_hosts = hosts.len();

        // Execute each batch sequentially
        for (batch_idx, batch_hosts) in batches.iter().enumerate() {
            debug!(
                "Executing batch {}/{} with {} hosts",
                batch_idx + 1,
                batches.len(),
                batch_hosts.len()
            );

            // Convert batch hosts to owned Strings
            let batch_hosts_owned: Vec<String> =
                batch_hosts.iter().map(|s| s.to_string()).collect();

            // Execute this batch based on the configured strategy
            let batch_results = match self.config.strategy {
                ExecutionStrategy::Linear => {
                    self.run_linear(&batch_hosts_owned, tasks, tx_id.clone())
                        .await?
                }
                ExecutionStrategy::Free => {
                    self.run_free(&batch_hosts_owned, tasks, tx_id.clone())
                        .await?
                }
                ExecutionStrategy::HostPinned => {
                    self.run_host_pinned(&batch_hosts_owned, tasks, tx_id.clone())
                        .await?
                }
            };

            // Count failures in this batch
            let batch_failed = batch_results
                .values()
                .filter(|r| r.failed || r.unreachable)
                .count();

            total_failed += batch_failed;

            // Merge batch results into overall results
            for (host, result) in batch_results {
                all_results.insert(host, result);
            }

            // Check max_fail_percentage if specified
            if let Some(max_fail_pct) = max_fail_percentage {
                let current_fail_pct = (total_failed as f64 / total_hosts as f64 * 100.0) as u8;

                if current_fail_pct > max_fail_pct {
                    error!(
                        "Failure percentage ({:.1}%) exceeded max_fail_percentage ({}%), aborting remaining batches",
                        current_fail_pct, max_fail_pct
                    );

                    // Mark remaining hosts as skipped
                    for remaining_batch in batches.iter().skip(batch_idx + 1) {
                        for host in remaining_batch.iter() {
                            all_results.insert(
                                host.to_string(),
                                HostResult {
                                    host: host.to_string(),
                                    stats: ExecutionStats {
                                        skipped: tasks.len(),
                                        ..Default::default()
                                    },
                                    failed: false,
                                    unreachable: false,
                                },
                            );
                        }
                    }

                    break;
                }
            }
        }

        info!(
            "Serial execution completed: {} hosts, {} failed",
            total_hosts, total_failed
        );

        Ok(all_results)
    }
}

fn apply_task_result(host_result: &mut HostResult, task_result: &TaskResult) {
    update_stats(&mut host_result.stats, task_result);
    if task_result.status == TaskStatus::Failed {
        host_result.failed = true;
    } else if task_result.status == TaskStatus::Unreachable {
        host_result.unreachable = true;
    }
}
