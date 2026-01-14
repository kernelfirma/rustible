use std::collections::HashMap;
use std::sync::Arc;

use futures::future::join_all;
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use crate::executor::runtime::ExecutionContext;
use crate::recovery::{TaskOutcome, TransactionId};

use super::results::update_stats;
use super::task::{Task, TaskResult, TaskStatus};
use super::{ExecutionEvent, Executor, ExecutorError, ExecutorResult, HostResult};

impl Executor {
    /// Run a single task on multiple hosts in parallel
    ///
    /// OPTIMIZATION: Fast path for single host and small host counts (< 10)
    /// to avoid Arc clone overhead and tokio::spawn overhead for small workloads.
    pub(super) async fn run_task_on_hosts(
        &self,
        hosts: &[String],
        task: &Task,
        tx_id: Option<TransactionId>,
    ) -> ExecutorResult<HashMap<String, TaskResult>> {
        debug!("Running task '{}' on {} hosts", task.name, hosts.len());

        // OPTIMIZATION: Fast path for single host - avoid Arc overhead and tokio::spawn
        if hosts.len() == 1 {
            let host = &hosts[0];
            let _permit = self.semaphore.acquire().await.unwrap();
            let connection = match self.get_connection_for_host(host).await {
                Ok(conn) => conn,
                Err(e) => {
                    let mut results = HashMap::with_capacity(1);
                    results.insert(
                        host.clone(),
                        TaskResult {
                            status: TaskStatus::Unreachable,
                            changed: false,
                            msg: Some(e.to_string()),
                            result: None,
                            diff: None,
                        },
                    );
                    return Ok(results);
                }
            };
            let python_interpreter = self.get_python_interpreter(host).await;

            // Apply become precedence: task > config
            let effective_become = task.r#become || self.config.r#become;
            let effective_become_user = task
                .become_user
                .clone()
                .unwrap_or_else(|| self.config.r#become_user.clone());

            let ctx = ExecutionContext::new(host.clone())
                .with_check_mode(self.config.check_mode)
                .with_diff_mode(self.config.diff_mode)
                .with_verbosity(self.config.verbosity)
                .with_connection(connection)
                .with_python_interpreter(python_interpreter)
                .with_become(effective_become)
                .with_become_method(self.config.r#become_method.clone())
                .with_become_user(effective_become_user)
                .with_become_password(self.config.r#become_password.clone());

            let result = task
                .execute(
                    &ctx,
                    &self.runtime,
                    &self.handlers,
                    &self.notified_handlers,
                    &self.parallelization_manager,
                    &self.module_registry,
                )
                .await;

            let mut results = HashMap::with_capacity(1);
            match result {
                Ok(task_result) => {
                    results.insert(host.clone(), task_result);
                }
                Err(e) => {
                    error!("Task failed on host {}: {}", host, e);
                    results.insert(
                        host.clone(),
                        TaskResult {
                            status: TaskStatus::Failed,
                            changed: false,
                            msg: Some(e.to_string()),
                            result: None,
                            diff: None,
                        },
                    );
                }
            }
            if let Some(rm) = &self.recovery_manager {
                if let Some(tid) = tx_id.as_ref() {
                    for (host, res) in &results {
                        let outcome = match res.status {
                            TaskStatus::Ok => TaskOutcome::Success,
                            TaskStatus::Changed => TaskOutcome::Changed,
                            TaskStatus::Failed => TaskOutcome::Failed {
                                message: res.msg.clone().unwrap_or_default(),
                            },
                            TaskStatus::Skipped => TaskOutcome::Skipped,
                            TaskStatus::Unreachable => TaskOutcome::Unreachable {
                                message: res.msg.clone().unwrap_or_default(),
                            },
                        };

                        if let Err(e) = rm
                            .record_task(
                                tid.clone(),
                                task.name.clone(),
                                host.clone(),
                                outcome,
                                res.changed,
                            )
                            .await
                        {
                            warn!("Failed to record task outcome for host {}: {}", host, e);
                        }
                    }
                }
            }
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

        let mut results = HashMap::with_capacity(hosts.len());
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
                    results.insert(
                        host.clone(),
                        TaskResult {
                            status: TaskStatus::Unreachable,
                            changed: false,
                            msg: Some(e.to_string()),
                            result: None,
                            diff: None,
                        },
                    );
                }
            }
        }

        // Apply become precedence: task > config
        let effective_become = task.r#become || config_become;
        let effective_become_user = task
            .become_user
            .clone()
            .unwrap_or_else(|| config_become_user.clone());

        // OPTIMIZATION: For small host counts, share task via Arc instead of cloning per host
        let task_arc = Arc::new(task.clone());
        let results = Arc::new(Mutex::new(results));

        let handles: Vec<_> = hosts
            .iter()
            .map(|host| {
                let host = host.clone();
                let task = Arc::clone(&task_arc);
                let results = Arc::clone(&results);
                let semaphore = Arc::clone(&self.semaphore);
                let runtime = Arc::clone(&self.runtime);
                let handlers = Arc::clone(&self.handlers);
                let notified = Arc::clone(&self.notified_handlers);
                let parallelization = Arc::clone(&self.parallelization_manager);
                let module_registry = Arc::clone(&self.module_registry);
                let effective_become = effective_become;
                let config_become_method = config_become_method.clone();
                let effective_become_user = effective_become_user.clone();
                let config_become_password = config_become_password.clone();
                let connection = connections.get(&host).cloned();
                let python_interpreter = python_interpreters
                    .get(&host)
                    .cloned()
                    .unwrap_or_else(|| "/usr/bin/python3".to_string());
                let callback = self.event_callback.clone();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();

                    let mut ctx = ExecutionContext::new(host.clone())
                        .with_check_mode(check_mode)
                        .with_diff_mode(diff_mode)
                        .with_verbosity(verbosity)
                        .with_become(effective_become)
                        .with_become_method(config_become_method)
                        .with_become_user(effective_become_user)
                        .with_become_password(config_become_password);

                    if let Some(conn) = connection {
                        ctx = ctx.with_connection(conn);
                    }
                    ctx = ctx.with_python_interpreter(python_interpreter);

                    let result = task
                        .execute(
                            &ctx,
                            &runtime,
                            &handlers,
                            &notified,
                            &parallelization,
                            &module_registry,
                        )
                        .await;

                    match result {
                        Ok(task_result) => {
                            if let Some(cb) = &callback {
                                cb(ExecutionEvent::HostTaskComplete(
                                    host.clone(),
                                    task.name.clone(),
                                    task_result.clone(),
                                ));
                            }
                            results.lock().await.insert(host, task_result);
                        }
                        Err(e) => {
                            error!("Task failed on host {}: {}", host, e);
                            if let Some(cb) = &callback {
                                let res = TaskResult {
                                    status: TaskStatus::Failed,
                                    changed: false,
                                    msg: Some(e.to_string()),
                                    result: None,
                                    diff: None,
                                };
                                cb(ExecutionEvent::HostTaskComplete(
                                    host.clone(),
                                    task.name.clone(),
                                    res,
                                ));
                            }
                            results.lock().await.insert(
                                host,
                                TaskResult {
                                    status: TaskStatus::Failed,
                                    changed: false,
                                    msg: Some(e.to_string()),
                                    result: None,
                                    diff: None,
                                },
                            );
                        }
                    }
                })
            })
            .collect();

        join_all(handles).await;

        let results = Arc::try_unwrap(results)
            .map_err(|_| ExecutorError::RuntimeError("Failed to unwrap results".into()))?
            .into_inner();

        if let Some(rm) = &self.recovery_manager {
            if let Some(tid) = tx_id.as_ref() {
                for (host, res) in &results {
                    let outcome = match res.status {
                        TaskStatus::Ok => TaskOutcome::Success,
                        TaskStatus::Changed => TaskOutcome::Changed,
                        TaskStatus::Failed => TaskOutcome::Failed {
                            message: res.msg.clone().unwrap_or_default(),
                        },
                        TaskStatus::Skipped => TaskOutcome::Skipped,
                        TaskStatus::Unreachable => TaskOutcome::Unreachable {
                            message: res.msg.clone().unwrap_or_default(),
                        },
                    };

                    if let Err(e) = rm
                        .record_task(
                            tid.clone(),
                            task.name.clone(),
                            host.clone(),
                            outcome,
                            res.changed,
                        )
                        .await
                    {
                        warn!("Failed to record task outcome for host {}: {}", host, e);
                    }
                }
            }
        }
        Ok(results)
    }

    /// Update host statistics based on task result
    pub(super) fn update_host_stats(&self, host_result: &mut HostResult, task_result: &TaskResult) {
        update_stats(&mut host_result.stats, task_result);
        if task_result.status == TaskStatus::Failed {
            host_result.failed = true;
        } else if task_result.status == TaskStatus::Unreachable {
            host_result.unreachable = true;
        }
    }
}
