use std::collections::HashMap;
use std::sync::Arc;

use futures::future::join_all;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::executor::runtime::ExecutionContext;
use crate::executor::task::BlockRole;
use crate::modules::ModuleClassification;
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

        let block_meta = collect_block_meta(tasks);
        let mut block_states: HashMap<String, HashMap<String, BlockState>> =
            HashMap::with_capacity(host_count);
        for h in hosts {
            block_states.insert(h.clone(), HashMap::new());
        }

        for (task_index, task) in tasks.iter().enumerate() {
            if let Some(cb) = &self.event_callback {
                cb(ExecutionEvent::TaskStart(task.name.clone()));
            }

            // Determine which hosts should run this task based on block state
            let mut active_hosts = Vec::new();
            for host in hosts {
                let (host_failed, host_unreachable) = results
                    .get(host)
                    .map(|r| (r.failed, r.unreachable))
                    .unwrap_or((false, false));
                let host_blocks = block_states
                    .get_mut(host)
                    .unwrap_or_else(|| panic!("Missing block state for host {}", host));
                if !host_failed && !host_unreachable {
                    ensure_block_states_for_task(task, host_blocks);
                }
                if should_run_task_for_host(task, host_failed, host_unreachable, host_blocks) {
                    active_hosts.push(host.clone());
                }
            }

            if active_hosts.is_empty() {
                // Check if all tasks remaining are block-related
                if task.block_stack.is_empty() {
                    warn!("All hosts have failed, stopping execution");
                    break;
                }
                for host in hosts {
                    if let Some(host_result) = results.get_mut(host) {
                        let host_blocks = block_states
                            .get_mut(host)
                            .unwrap_or_else(|| panic!("Missing block state for host {}", host));
                        finalize_blocks_for_task_index(
                            host_result,
                            task,
                            task_index,
                            host_blocks,
                            &block_meta,
                        );
                    }
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
                    let host_blocks = block_states
                        .get_mut(&host)
                        .unwrap_or_else(|| panic!("Missing block state for host {}", host));
                    update_host_result_for_task(host_result, task, &task_result, host_blocks, &block_meta);
                }
            }

            for host in hosts {
                if let Some(host_result) = results.get_mut(host) {
                    let host_blocks = block_states
                        .get_mut(host)
                        .unwrap_or_else(|| panic!("Missing block state for host {}", host));
                    finalize_blocks_for_task_index(
                        host_result,
                        task,
                        task_index,
                        host_blocks,
                        &block_meta,
                    );
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
        let requires_connection = tasks.iter().any(|task| {
            self.module_registry
                .get(&task.module)
                .map(|module| !matches!(module.classification(), ModuleClassification::LocalLogic))
                .unwrap_or(true)
        });
        let block_meta = collect_block_meta(tasks);

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
            let (connection, python_interpreter) = if requires_connection {
                match self.get_connection_for_host(host).await {
                    Ok(conn) => (Some(conn), self.get_python_interpreter(host).await),
                    Err(e) => {
                        host_result.stats.unreachable = 1;
                        host_result.unreachable = true;
                        warn!("Host unreachable: {} ({})", host, e);
                        let mut results = HashMap::with_capacity(1);
                        results.insert(host.clone(), host_result);
                        return Ok(results);
                    }
                }
            } else {
                (None, "/usr/bin/python3".to_string())
            };

            let mut block_states: HashMap<String, BlockState> = HashMap::new();

            for (task_index, task) in tasks.iter().enumerate() {
                if !host_result.failed && !host_result.unreachable {
                    ensure_block_states_for_task(task, &mut block_states);
                }
                let should_run =
                    should_run_task_for_host(task, host_result.failed, host_result.unreachable, &block_states);

                if should_run {
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
                    let mut ctx = if let Some(conn) = connection.clone() {
                        ctx.with_connection(conn)
                    } else {
                        ctx
                    };
                    ctx = ctx.with_python_interpreter(python_interpreter.clone());

                    {
                        let mut rt = self.runtime.write().await;
                        rt.set_block_vars(host, task.merged_block_vars());
                        rt.set_task_vars(host, task.vars.clone());
                    }

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

                    {
                        let mut rt = self.runtime.write().await;
                        rt.clear_task_vars(host);
                        rt.clear_block_vars(host);
                    }

                    Self::record_task_outcome(
                        self.recovery_manager.as_ref(),
                        tx_id.as_ref(),
                        &task.name,
                        host,
                        &task_result,
                    )
                    .await;

                    update_host_result_for_task(
                        &mut host_result,
                        task,
                        &task_result,
                        &mut block_states,
                        &block_meta,
                    );
                }

                finalize_blocks_for_task_index(
                    &mut host_result,
                    task,
                    task_index,
                    &mut block_states,
                    &block_meta,
                );
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

        if requires_connection {
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
        }

        // Avoid cloning entire task list - use Arc slice instead
        let tasks: Arc<[Task]> = tasks.to_vec().into();
        let results = Arc::new(Mutex::new(base_results));

        let handles: Vec<_> = hosts
            .iter()
            .filter(|host| !requires_connection || connections.contains_key(*host))
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
                let config_become_method = config_become_method.clone();
                let config_become_user = config_become_user.clone();
                let config_become_password = config_become_password.clone();
                let connection = connections.get(&host).cloned();
                let python_interpreter = python_interpreters
                    .get(&host)
                    .cloned()
                    .unwrap_or_else(|| "/usr/bin/python3".to_string());
                let callback = self.event_callback.clone();
                let block_meta = block_meta.clone();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    let mut host_result = HostResult {
                        host: host.clone(),
                        stats: ExecutionStats::default(),
                        failed: false,
                        unreachable: false,
                    };
                    let mut block_states: HashMap<String, BlockState> = HashMap::new();

                    for (task_index, task) in tasks.iter().enumerate() {
                        if !host_result.failed && !host_result.unreachable {
                            ensure_block_states_for_task(task, &mut block_states);
                        }
                        let should_run = should_run_task_for_host(
                            task,
                            host_result.failed,
                            host_result.unreachable,
                            &block_states,
                        );

                        if should_run {
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

                            {
                                let mut rt = runtime.write().await;
                                rt.set_block_vars(&host, task.merged_block_vars());
                                rt.set_task_vars(&host, task.vars.clone());
                            }

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

                            {
                                let mut rt = runtime.write().await;
                                rt.clear_task_vars(&host);
                                rt.clear_block_vars(&host);
                            }

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

                            update_host_result_for_task(
                                &mut host_result,
                                task,
                                &task_result,
                                &mut block_states,
                                &block_meta,
                            );
                        }

                        finalize_blocks_for_task_index(
                            &mut host_result,
                            task,
                            task_index,
                            &mut block_states,
                            &block_meta,
                        );
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

#[derive(Debug, Default, Clone)]
struct BlockState {
    failed: bool,
    rescue_failed: bool,
}

#[derive(Debug, Clone)]
struct BlockMeta {
    parent: Option<String>,
    parent_role: Option<BlockRole>,
    has_rescue: bool,
    last_index: usize,
}

fn collect_block_meta(tasks: &[Task]) -> HashMap<String, BlockMeta> {
    let mut meta = HashMap::new();
    for (index, task) in tasks.iter().enumerate() {
        for (depth, ctx) in task.block_stack.iter().enumerate() {
            let entry = meta.entry(ctx.id.clone()).or_insert(BlockMeta {
                parent: None,
                parent_role: None,
                has_rescue: false,
                last_index: index,
            });
            if entry.last_index < index {
                entry.last_index = index;
            }
            if ctx.role == BlockRole::Rescue {
                entry.has_rescue = true;
            }
            if depth > 0 && entry.parent.is_none() {
                let parent_ctx = &task.block_stack[depth - 1];
                entry.parent = Some(parent_ctx.id.clone());
                entry.parent_role = Some(parent_ctx.role);
            }
        }
    }
    meta
}

fn ensure_block_states_for_task(task: &Task, block_states: &mut HashMap<String, BlockState>) {
    for ctx in &task.block_stack {
        block_states.entry(ctx.id.clone()).or_default();
    }
}

fn should_run_task_for_host(
    task: &Task,
    host_failed: bool,
    host_unreachable: bool,
    block_states: &HashMap<String, BlockState>,
) -> bool {
    if host_unreachable {
        return false;
    }

    if task.block_stack.is_empty() {
        return !host_failed;
    }

    let mut has_always = false;
    for ctx in &task.block_stack {
        let Some(state) = block_states.get(&ctx.id) else {
            return false;
        };
        match ctx.role {
            BlockRole::Normal => {
                if state.failed || state.rescue_failed {
                    return false;
                }
            }
            BlockRole::Rescue => {
                if !state.failed || state.rescue_failed {
                    return false;
                }
            }
            BlockRole::Always => {
                has_always = true;
            }
        }
    }

    if host_failed {
        return has_always;
    }

    true
}

fn update_host_result_for_task(
    host_result: &mut HostResult,
    task: &Task,
    task_result: &TaskResult,
    block_states: &mut HashMap<String, BlockState>,
    block_meta: &HashMap<String, BlockMeta>,
) {
    let mut stats_result = task_result.clone();
    if task_result.status == TaskStatus::Failed {
        if let Some(last_ctx) = task.block_stack.last() {
            if last_ctx.role == BlockRole::Normal {
                let rescued = task.block_stack.iter().any(|ctx| {
                    block_meta
                        .get(&ctx.id)
                        .map(|meta| meta.has_rescue)
                        .unwrap_or(false)
                });
                if rescued {
                    stats_result.status = TaskStatus::Ok;
                }
            }
        }
    }
    update_stats(&mut host_result.stats, &stats_result);

    if task_result.status == TaskStatus::Unreachable {
        host_result.unreachable = true;
        return;
    }

    if let Some(last_ctx) = task.block_stack.last() {
        let state = block_states.entry(last_ctx.id.clone()).or_default();
        match last_ctx.role {
            BlockRole::Normal => {
                if task_result.status == TaskStatus::Failed {
                    state.failed = true;
                }
            }
            BlockRole::Rescue => {
                if task_result.status == TaskStatus::Failed {
                    state.rescue_failed = true;
                }
            }
            BlockRole::Always => {
                if task_result.status == TaskStatus::Failed {
                    state.rescue_failed = true;
                }
            }
        }
        return;
    }

    if task_result.status == TaskStatus::Failed {
        host_result.failed = true;
    }
}

fn finalize_blocks_for_task_index(
    host_result: &mut HostResult,
    task: &Task,
    task_index: usize,
    block_states: &mut HashMap<String, BlockState>,
    block_meta: &HashMap<String, BlockMeta>,
) {
    if task.block_stack.is_empty() {
        return;
    }

    for ctx in task.block_stack.iter().rev() {
        let Some(meta) = block_meta.get(&ctx.id) else {
            continue;
        };
        if meta.last_index != task_index {
            continue;
        }

        let Some(state) = block_states.remove(&ctx.id) else {
            continue;
        };

        let block_failed = if meta.has_rescue {
            state.rescue_failed
        } else {
            state.failed || state.rescue_failed
        };

        if !block_failed {
            continue;
        }

        if let Some(parent_id) = &meta.parent {
            let parent_state = block_states.entry(parent_id.clone()).or_default();
            match meta.parent_role.unwrap_or(BlockRole::Normal) {
                BlockRole::Normal => parent_state.failed = true,
                BlockRole::Rescue => parent_state.rescue_failed = true,
                BlockRole::Always => parent_state.rescue_failed = true,
            }
        } else {
            host_result.failed = true;
        }
    }
}
