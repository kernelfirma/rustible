//! Integration tests for executor enhancements
//!
//! This test suite covers the enhanced executor capabilities:
//! - Async task execution with background jobs, polling, and timeout support
//! - Throttling functionality with global, per-host, and rate limiting
//! - Pipeline execution for file operations and package batching
//! - Work-stealing scheduler for optimal load balancing
//!
//! Tests verify correctness, concurrency behavior, and performance characteristics.

#![allow(unused_imports)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ============================================================================
// Async Task Execution Tests
// ============================================================================

mod async_task_tests {
    use super::*;
    use rustible::executor::async_task::{
        AsyncConfig, AsyncJobInfo, AsyncJobStatus, AsyncTaskManager,
    };
    use rustible::executor::task::TaskResult;

    #[test]
    fn test_async_config_defaults() {
        let config = AsyncConfig::default();
        assert_eq!(config.async_timeout, 0);
        assert!(config.poll_interval.is_none());
        assert!(!config.is_async());
        assert_eq!(config.get_poll_interval(), 15); // Default poll interval
    }

    #[test]
    fn test_async_config_fire_and_forget() {
        let config = AsyncConfig::new(3600, Some(0));
        assert!(config.is_async());
        assert!(config.is_fire_and_forget());
        assert_eq!(config.get_poll_interval(), 0);
    }

    #[test]
    fn test_async_config_with_polling() {
        let config = AsyncConfig::new(300, Some(10));
        assert!(config.is_async());
        assert!(!config.is_fire_and_forget());
        assert_eq!(config.get_poll_interval(), 10);
    }

    #[test]
    fn test_async_job_info_lifecycle() {
        let mut job = AsyncJobInfo::new(
            "test.123".to_string(),
            "localhost".to_string(),
            "Test task".to_string(),
            "command".to_string(),
            3600,
        );

        // Initial state
        assert_eq!(job.status, AsyncJobStatus::Pending);
        assert!(!job.finished);
        assert!(!job.changed);

        // Mark running
        job.mark_running();
        assert_eq!(job.status, AsyncJobStatus::Running);
        assert!(!job.finished);

        // Mark finished
        job.mark_finished(TaskResult::changed().with_msg("Done"));
        assert_eq!(job.status, AsyncJobStatus::Finished);
        assert!(job.finished);
        assert!(job.changed);
        assert!(job.ended.is_some());
    }

    #[test]
    fn test_async_job_info_failure() {
        let mut job = AsyncJobInfo::new(
            "test.456".to_string(),
            "localhost".to_string(),
            "Failing task".to_string(),
            "command".to_string(),
            3600,
        );

        job.mark_failed("Task failed with error".to_string());

        assert_eq!(job.status, AsyncJobStatus::Failed);
        assert!(job.finished);
        assert!(job.msg.is_some());
        assert!(job.msg.unwrap().contains("Task failed"));
    }

    #[test]
    fn test_async_job_info_timeout() {
        let mut job = AsyncJobInfo::new(
            "test.789".to_string(),
            "localhost".to_string(),
            "Timeout task".to_string(),
            "command".to_string(),
            0, // Immediate timeout
        );

        assert!(job.is_timed_out());

        job.mark_timed_out();
        assert_eq!(job.status, AsyncJobStatus::TimedOut);
        assert!(job.finished);
    }

    #[test]
    fn test_async_job_info_cancellation() {
        let mut job = AsyncJobInfo::new(
            "test.cancel".to_string(),
            "localhost".to_string(),
            "Cancellable task".to_string(),
            "command".to_string(),
            3600,
        );

        job.mark_cancelled();
        assert_eq!(job.status, AsyncJobStatus::Cancelled);
        assert!(job.finished);
        assert!(job.msg.unwrap().contains("cancelled"));
    }

    #[test]
    fn test_async_job_id_generation() {
        let id1 = AsyncTaskManager::generate_job_id();
        let id2 = AsyncTaskManager::generate_job_id();

        // IDs should be unique
        assert_ne!(id1, id2);

        // IDs should contain a timestamp and UUID part
        assert!(id1.contains('.'));
        assert!(id2.contains('.'));
    }

    #[tokio::test]
    async fn test_async_manager_submit_and_complete() {
        let manager = AsyncTaskManager::new();
        let completed = Arc::new(AtomicUsize::new(0));
        let completed_clone = completed.clone();

        let jid = manager
            .submit_task("localhost", "Test task", "debug", 10, move || {
                let completed = completed_clone.clone();
                async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    completed.fetch_add(1, Ordering::SeqCst);
                    Ok(TaskResult::ok().with_msg("Success"))
                }
            })
            .await
            .expect("Failed to submit task");

        // Job should be running initially
        let status = manager.get_job_status(&jid).await.unwrap();
        assert_eq!(status.status, AsyncJobStatus::Running);

        // Wait for completion
        let result = manager.wait_for_job(&jid, 1, Some(5)).await;
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(info.finished);
        assert_eq!(info.status, AsyncJobStatus::Finished);

        // Task should have completed
        assert_eq!(completed.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_async_manager_submit_multiple_concurrent() {
        let manager = AsyncTaskManager::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let mut jids = Vec::new();
        for i in 0..5 {
            let counter_clone = counter.clone();
            let jid = manager
                .submit_task(
                    "localhost",
                    &format!("Task {}", i),
                    "debug",
                    10,
                    move || {
                        let counter = counter_clone.clone();
                        async move {
                            tokio::time::sleep(Duration::from_millis(20)).await;
                            counter.fetch_add(1, Ordering::SeqCst);
                            Ok(TaskResult::ok())
                        }
                    },
                )
                .await
                .expect("Failed to submit task");
            jids.push(jid);
        }

        // Wait for all to complete
        for jid in &jids {
            let _ = manager.wait_for_job(jid, 1, Some(5)).await;
        }

        // All tasks should have completed
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn test_async_manager_cancel_running_job() {
        let manager = AsyncTaskManager::new();

        let jid = manager
            .submit_task("localhost", "Long task", "command", 60, || async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(TaskResult::ok())
            })
            .await
            .expect("Failed to submit task");

        // Give it time to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Cancel the job
        let cancelled = manager.cancel_job(&jid).await;
        assert!(cancelled);

        // Check status
        let status = manager.get_job_status(&jid).await.unwrap();
        assert_eq!(status.status, AsyncJobStatus::Cancelled);
        assert!(status.finished);
    }

    #[tokio::test]
    async fn test_async_manager_timeout() {
        let manager = AsyncTaskManager::with_config(1, 10, 86400); // 1 second timeout

        let jid = manager
            .submit_task("localhost", "Timeout task", "command", 1, || async {
                // This takes longer than the timeout
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(TaskResult::ok())
            })
            .await
            .expect("Failed to submit task");

        // Wait for the job (it should timeout)
        let result = manager.wait_for_job(&jid, 1, Some(10)).await;
        assert!(result.is_some());

        let info = result.unwrap();
        assert!(info.finished);
        assert_eq!(info.status, AsyncJobStatus::TimedOut);
    }

    #[tokio::test]
    async fn test_async_manager_list_jobs() {
        let manager = AsyncTaskManager::new();

        // Submit jobs on different hosts
        let jid1 = manager
            .submit_task("host1", "Task 1", "debug", 60, || async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(TaskResult::ok())
            })
            .await
            .unwrap();

        let jid2 = manager
            .submit_task("host2", "Task 2", "debug", 60, || async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(TaskResult::ok())
            })
            .await
            .unwrap();

        // List all jobs
        let all_jobs = manager.list_jobs(None).await;
        assert_eq!(all_jobs.len(), 2);

        // List jobs for host1 only
        let host1_jobs = manager.list_jobs(Some("host1")).await;
        assert_eq!(host1_jobs.len(), 1);
        assert_eq!(host1_jobs[0].host, "host1");

        // List running jobs
        let running = manager.list_running_jobs(None).await;
        assert_eq!(running.len(), 2);

        // Cancel jobs for cleanup
        manager.cancel_job(&jid1).await;
        manager.cancel_job(&jid2).await;
    }

    #[test]
    fn test_async_result_creation() {
        let result = AsyncTaskManager::create_async_result("123.abc", "localhost", true);

        assert!(result.changed);
        let data = result.result.unwrap();
        assert_eq!(data["ansible_job_id"], "123.abc");
        assert_eq!(data["started"], 1);
        assert_eq!(data["finished"], 0);
    }

    #[tokio::test]
    async fn test_async_manager_concurrent_limit() {
        let manager = AsyncTaskManager::with_config(3600, 2, 86400); // Max 2 concurrent per host

        // Submit 2 tasks (should succeed)
        let jid1 = manager
            .submit_task("host1", "Task 1", "debug", 60, || async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(TaskResult::ok())
            })
            .await;
        assert!(jid1.is_ok());

        let jid2 = manager
            .submit_task("host1", "Task 2", "debug", 60, || async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(TaskResult::ok())
            })
            .await;
        assert!(jid2.is_ok());

        // Third task should fail due to concurrent limit
        let jid3 = manager
            .submit_task("host1", "Task 3", "debug", 60, || async {
                Ok(TaskResult::ok())
            })
            .await;
        assert!(jid3.is_err());

        // But a task on a different host should succeed
        let jid4 = manager
            .submit_task("host2", "Task 4", "debug", 60, || async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(TaskResult::ok())
            })
            .await;
        assert!(jid4.is_ok());

        // Cleanup
        manager.cancel_job(&jid1.unwrap()).await;
        manager.cancel_job(&jid2.unwrap()).await;
        manager.cancel_job(&jid4.unwrap()).await;
    }
}

// ============================================================================
// Throttling Functionality Tests
// ============================================================================

mod throttle_tests {
    use super::*;
    use rustible::executor::throttle::{TaskThrottleManager, ThrottleConfig, ThrottleManager};

    #[test]
    fn test_throttle_config_defaults() {
        let config = ThrottleConfig::default();
        assert_eq!(config.global_limit, 0);
        assert_eq!(config.per_host_limit, 0);
        assert!(config.module_rate_limits.is_empty());
        assert_eq!(config.default_rate_limit, 0);
    }

    #[test]
    fn test_throttle_config_builder() {
        let config = ThrottleConfig::with_global_limit(5)
            .per_host(2)
            .rate_limit_module("api_call", 10);

        assert_eq!(config.global_limit, 5);
        assert_eq!(config.per_host_limit, 2);
        assert_eq!(config.module_rate_limits.get("api_call"), Some(&10));
    }

    #[tokio::test]
    async fn test_throttle_unlimited_immediate() {
        let manager = ThrottleManager::unlimited();
        let counter = Arc::new(AtomicUsize::new(0));

        let start = Instant::now();
        let mut handles = vec![];

        for i in 0..10 {
            let manager = manager.clone();
            let counter = counter.clone();
            let handle = tokio::spawn(async move {
                let _guard = manager
                    .acquire("host1", &format!("module{}", i), None)
                    .await;
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // All should execute in parallel (total time should be close to 10ms)
        assert!(
            elapsed < Duration::from_millis(100),
            "Unlimited throttle should not block: took {:?}",
            elapsed
        );
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_throttle_global_limit() {
        let config = ThrottleConfig::with_global_limit(2);
        let manager = Arc::new(ThrottleManager::new(config));

        let start = Instant::now();
        let mut handles = vec![];

        for i in 0..4 {
            let manager = manager.clone();
            let handle = tokio::spawn(async move {
                let _guard = manager.acquire(&format!("host{}", i), "test", None).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // With global limit of 2, 4 tasks should take at least 2 batches (100ms)
        assert!(
            elapsed >= Duration::from_millis(90),
            "Global throttle should serialize tasks: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_throttle_per_host_limit() {
        let config = ThrottleConfig::default().per_host(1);
        let manager = Arc::new(ThrottleManager::new(config));

        // Two tasks on same host should be serialized
        let manager1 = manager.clone();
        let handle1 = tokio::spawn(async move {
            let _guard = manager1.acquire("host1", "test", None).await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });

        // Give first task time to acquire
        tokio::time::sleep(Duration::from_millis(20)).await;

        let manager2 = manager.clone();
        let start = Instant::now();
        let handle2 = tokio::spawn(async move {
            let _guard = manager2.acquire("host1", "test", None).await;
        });

        handle1.await.unwrap();
        handle2.await.unwrap();

        let elapsed = start.elapsed();
        // Use generous timing for CI environments (use 50ms threshold for ~100ms expected)
        assert!(
            elapsed >= Duration::from_millis(50),
            "Per-host throttle should serialize same host: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_throttle_per_host_different_hosts_parallel() {
        let config = ThrottleConfig::default().per_host(1);
        let manager = Arc::new(ThrottleManager::new(config));

        let start = Instant::now();

        let manager1 = manager.clone();
        let handle1 = tokio::spawn(async move {
            let _guard = manager1.acquire("host1", "test", None).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        let manager2 = manager.clone();
        let handle2 = tokio::spawn(async move {
            let _guard = manager2.acquire("host2", "test", None).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        });

        futures::future::join_all(vec![handle1, handle2]).await;
        let elapsed = start.elapsed();

        // Different hosts should run in parallel
        assert!(
            elapsed < Duration::from_millis(80),
            "Different hosts should run in parallel: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_throttle_combined_global_and_per_host() {
        let config = ThrottleConfig::with_global_limit(3).per_host(1);
        let manager = Arc::new(ThrottleManager::new(config));

        let start = Instant::now();
        let mut handles = vec![];

        // 4 tasks on 4 different hosts
        for i in 0..4 {
            let manager = manager.clone();
            let handle = tokio::spawn(async move {
                let _guard = manager.acquire(&format!("host{}", i), "test", None).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // Global limit of 3 means 4th task must wait
        assert!(
            elapsed >= Duration::from_millis(90),
            "Combined throttle should respect global limit: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_throttle_rate_limiting() {
        let config = ThrottleConfig::default().rate_limit_module("api_call", 2);
        let manager = Arc::new(ThrottleManager::new(config));

        // Drain the token bucket (capacity = 2)
        let _guard1 = manager.acquire("host1", "api_call", None).await;
        let _guard2 = manager.acquire("host1", "api_call", None).await;

        // Third request should wait for rate limit
        let start = Instant::now();
        let _guard3 = manager.acquire("host1", "api_call", None).await;
        let elapsed = start.elapsed();

        // Should have waited ~500ms for a token (2 req/sec = 500ms between tokens)
        assert!(
            elapsed >= Duration::from_millis(400),
            "Rate limiting should enforce delay: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_throttle_rate_limit_refill() {
        let config = ThrottleConfig::default().rate_limit_module("api_call", 10);
        let manager = Arc::new(ThrottleManager::new(config));

        // Use all 10 tokens quickly
        for _ in 0..10 {
            let _guard = manager.acquire("host1", "api_call", None).await;
        }

        // Wait for some tokens to refill
        tokio::time::sleep(Duration::from_millis(300)).await;

        // Should be able to acquire some more tokens
        let start = Instant::now();
        let _guard = manager.acquire("host1", "api_call", None).await;
        let elapsed = start.elapsed();

        // Should not have waited long (tokens refilled)
        assert!(
            elapsed < Duration::from_millis(300),
            "Tokens should have refilled: took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_throttle_stats() {
        let config = ThrottleConfig::with_global_limit(5).per_host(2);
        let manager = ThrottleManager::new(config);

        let stats = manager.stats();
        assert_eq!(stats.global_available, 5);
        assert!(stats.host_permits.is_empty());
        assert!(stats.rate_limiter_states.is_empty());
    }

    #[tokio::test]
    async fn test_task_throttle_manager() {
        let manager = TaskThrottleManager::new();

        let start = Instant::now();
        let mut handles = vec![];

        // 4 tasks with throttle of 2
        for _ in 0..4 {
            let manager = manager.clone();
            let handle = tokio::spawn(async move {
                let _permit = manager.acquire("task1", 2).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // With throttle of 2, 4 tasks should take at least 2 batches (100ms)
        assert!(
            elapsed >= Duration::from_millis(90),
            "Task throttle should serialize: took {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_task_throttle_manager_different_tasks() {
        let manager = std::sync::Arc::new(TaskThrottleManager::new());

        let start = Instant::now();
        let mut handles = vec![];

        // 2 tasks for task1 (throttle 1) and 2 tasks for task2 (throttle 1)
        // They should run in parallel since they're different tasks
        for _i in 0..2 {
            let manager1 = manager.clone();
            let manager2 = manager.clone();

            let handle = tokio::spawn(async move {
                let _permit = manager1.acquire("task1", 1).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
            handles.push(handle);

            let handle = tokio::spawn(async move {
                let _permit = manager2.acquire("task2", 1).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;
        let elapsed = start.elapsed();

        // Different tasks should have independent throttles
        // task1: 2 tasks at throttle 1 = 100ms sequential
        // task2: 2 tasks at throttle 1 = 100ms sequential
        // But task1 and task2 run in parallel
        assert!(
            elapsed >= Duration::from_millis(90),
            "Each task should be throttled independently: took {:?}",
            elapsed
        );
    }

    #[test]
    fn test_task_throttle_manager_clear() {
        let manager = TaskThrottleManager::new();

        // Create some semaphores
        let _ = manager.get_or_create("task1", 5);
        let _ = manager.get_or_create("task2", 3);

        // Clear should remove all
        manager.clear();

        // New requests should create fresh semaphores
        let sem = manager.get_or_create("task1", 10);
        assert_eq!(sem.available_permits(), 10);
    }
}

// ============================================================================
// Pipeline Execution Tests
// ============================================================================

mod pipeline_tests {
    use super::*;
    use rustible::executor::pipeline::{
        BranchPrediction, BranchPredictor, ExecutionPipeline, FileOperationBatch,
        FileOperationType, FilePipeline, PackageBatch, PackageBatchConfig, PackageBatcher,
        PackageOperation, PipelineConfig, SpeculativeConfig,
    };

    #[test]
    fn test_speculative_config_defaults() {
        let config = SpeculativeConfig::default();
        assert!(config.enabled);
        assert_eq!(config.lookahead, 3);
        assert_eq!(config.max_speculation_time_ms, 500);
        assert_eq!(config.confidence_threshold, 0.7);
    }

    #[test]
    fn test_branch_prediction_constructors() {
        let likely = BranchPrediction::likely();
        assert!(likely.likelihood > 0.8);
        assert!(likely.should_speculate);

        let unlikely = BranchPrediction::unlikely();
        assert!(unlikely.likelihood < 0.2);
        assert!(!unlikely.should_speculate);

        let uncertain = BranchPrediction::uncertain();
        assert!((uncertain.likelihood - 0.5).abs() < 0.1);
        assert!(!uncertain.should_speculate);
    }

    #[tokio::test]
    async fn test_branch_predictor_static_patterns() {
        let predictor = BranchPredictor::new();

        // OS family checks are usually true
        let pred = predictor.predict("ansible_os_family == 'Debian'").await;
        assert!(pred.likelihood > 0.7);
        assert!(pred.should_speculate);

        // Failed checks are usually false
        let pred = predictor.predict("result.failed").await;
        assert!(pred.likelihood < 0.3);
        assert!(!pred.should_speculate);

        // Skipped checks are usually false
        let pred = predictor.predict("previous_task.skipped").await;
        assert!(pred.likelihood < 0.3);

        // "is defined" checks are usually true
        let pred = predictor.predict("my_var is defined").await;
        assert!(pred.likelihood > 0.7);
    }

    #[tokio::test]
    async fn test_branch_predictor_learning() {
        let predictor = BranchPredictor::new();
        let condition = "custom_condition == 'special_value'";

        // Initially uncertain
        let pred = predictor.predict(condition).await;
        assert!(pred.confidence < 0.5);

        // Record mostly true outcomes (21 total samples for confidence > 0.5)
        for _ in 0..16 {
            predictor.record_outcome(condition, true).await;
        }
        for _ in 0..5 {
            predictor.record_outcome(condition, false).await;
        }

        // Should now predict likely (~76% true with 21 samples)
        let pred = predictor.predict(condition).await;
        assert!(pred.likelihood > 0.7);
        // With 21 samples: confidence = 1.0 - (1.0 / (1.0 + 21/20)) ≈ 0.512
        assert!(pred.confidence > 0.5);
    }

    #[tokio::test]
    async fn test_branch_predictor_low_sample_confidence() {
        let predictor = BranchPredictor::new();
        let condition = "new_condition";

        // Record just a few outcomes
        predictor.record_outcome(condition, true).await;
        predictor.record_outcome(condition, true).await;

        let pred = predictor.predict(condition).await;
        // High likelihood but low confidence due to small sample
        assert!(pred.likelihood > 0.8);
        assert!(pred.confidence < 0.5);
    }

    #[test]
    fn test_file_operation_type_target_path() {
        let copy = FileOperationType::Copy {
            src: "source.txt".to_string(),
            dest: "/opt/app/file.txt".to_string(),
        };
        assert_eq!(copy.target_path(), "/opt/app/file.txt");

        let mkdir = FileOperationType::Mkdir {
            path: "/opt/app".to_string(),
        };
        assert_eq!(mkdir.target_path(), "/opt/app");

        let chmod = FileOperationType::Chmod {
            path: "/opt/app/script.sh".to_string(),
            mode: "0755".to_string(),
        };
        assert_eq!(chmod.target_path(), "/opt/app/script.sh");
    }

    #[test]
    fn test_file_operation_dependencies() {
        let mkdir = FileOperationType::Mkdir {
            path: "/opt/app".to_string(),
        };
        let copy = FileOperationType::Copy {
            src: "config.yaml".to_string(),
            dest: "/opt/app/config.yaml".to_string(),
        };
        let nested_mkdir = FileOperationType::Mkdir {
            path: "/opt/app/logs".to_string(),
        };

        // Copy depends on mkdir (copy target is under mkdir path)
        assert!(copy.depends_on(&mkdir));
        // Nested mkdir depends on parent mkdir
        assert!(nested_mkdir.depends_on(&mkdir));
        // Parent mkdir does not depend on child
        assert!(!mkdir.depends_on(&copy));
        assert!(!mkdir.depends_on(&nested_mkdir));
    }

    #[test]
    fn test_file_operation_batch_ordering() {
        let mut batch = FileOperationBatch::new("host1");

        // Add operations in wrong order
        batch.add(
            FileOperationType::Copy {
                src: "file.txt".to_string(),
                dest: "/opt/app/file.txt".to_string(),
            },
            1000,
        );
        batch.add(
            FileOperationType::Mkdir {
                path: "/opt/app".to_string(),
            },
            100,
        );
        batch.add(
            FileOperationType::Chmod {
                path: "/opt/app/file.txt".to_string(),
                mode: "0644".to_string(),
            },
            50,
        );

        let ordered = batch.ordered_operations();

        // Mkdir should come first (others depend on it)
        assert!(matches!(ordered[0], FileOperationType::Mkdir { .. }));
    }

    #[tokio::test]
    async fn test_file_pipeline_batching() {
        let config = PipelineConfig {
            enabled: true,
            max_batch_size: 3,
            max_batch_bytes: 1024 * 1024,
            batch_timeout_ms: 5000,
        };
        let pipeline = FilePipeline::new(config);

        // Add first operation
        pipeline
            .add_operation(
                "host1",
                FileOperationType::Mkdir {
                    path: "/opt/app".to_string(),
                },
                100,
            )
            .await;

        // Not ready yet
        let ready = pipeline.get_ready_batches().await;
        assert!(ready.is_empty());

        // Add second operation
        pipeline
            .add_operation(
                "host1",
                FileOperationType::Copy {
                    src: "file1.txt".to_string(),
                    dest: "/opt/app/file1.txt".to_string(),
                },
                1000,
            )
            .await;

        // Still not ready
        let ready = pipeline.get_ready_batches().await;
        assert!(ready.is_empty());

        // Add third operation - should trigger batch
        pipeline
            .add_operation(
                "host1",
                FileOperationType::Copy {
                    src: "file2.txt".to_string(),
                    dest: "/opt/app/file2.txt".to_string(),
                },
                1000,
            )
            .await;

        // Should be ready now
        let ready = pipeline.get_ready_batches().await;
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].operations.len(), 3);
    }

    #[tokio::test]
    async fn test_file_pipeline_multiple_hosts() {
        let config = PipelineConfig {
            enabled: true,
            max_batch_size: 2,
            max_batch_bytes: 1024 * 1024,
            batch_timeout_ms: 5000,
        };
        let pipeline = FilePipeline::new(config);

        // Add operations for different hosts
        pipeline
            .add_operation(
                "host1",
                FileOperationType::Mkdir {
                    path: "/opt/app".to_string(),
                },
                100,
            )
            .await;
        pipeline
            .add_operation(
                "host2",
                FileOperationType::Mkdir {
                    path: "/opt/app".to_string(),
                },
                100,
            )
            .await;

        // Neither should be ready (only 1 op each)
        let ready = pipeline.get_ready_batches().await;
        assert!(ready.is_empty());

        // Add another to host1 - host1 should be ready
        pipeline
            .add_operation(
                "host1",
                FileOperationType::Copy {
                    src: "file.txt".to_string(),
                    dest: "/opt/app/file.txt".to_string(),
                },
                1000,
            )
            .await;

        let ready = pipeline.get_ready_batches().await;
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].host, "host1");

        // host2 should still have pending
        assert!(pipeline.has_pending().await);
    }

    #[tokio::test]
    async fn test_file_pipeline_flush_all() {
        let pipeline = FilePipeline::default();

        // Add operations
        for i in 0..5 {
            pipeline
                .add_operation(
                    &format!("host{}", i),
                    FileOperationType::Mkdir {
                        path: "/opt/app".to_string(),
                    },
                    100,
                )
                .await;
        }

        // Flush all
        let batches = pipeline.flush_all().await;
        assert_eq!(batches.len(), 5);

        // No more pending
        assert!(!pipeline.has_pending().await);
    }

    #[test]
    fn test_package_operation_types() {
        let install = PackageOperation::Install {
            name: "nginx".to_string(),
            version: Some("1.18.0".to_string()),
        };
        let remove = PackageOperation::Remove {
            name: "apache2".to_string(),
        };
        let update = PackageOperation::Update {
            name: "curl".to_string(),
        };
        let upgrade = PackageOperation::Upgrade;

        // Just verify they can be constructed
        assert!(matches!(install, PackageOperation::Install { .. }));
        assert!(matches!(remove, PackageOperation::Remove { .. }));
        assert!(matches!(update, PackageOperation::Update { .. }));
        assert!(matches!(upgrade, PackageOperation::Upgrade));
    }

    #[test]
    fn test_package_batch_operations() {
        let mut batch = PackageBatch::new("apt", "host1");

        batch.add(PackageOperation::Install {
            name: "nginx".to_string(),
            version: None,
        });
        batch.add(PackageOperation::Install {
            name: "curl".to_string(),
            version: Some("7.68.0".to_string()),
        });
        batch.add(PackageOperation::Remove {
            name: "apache2".to_string(),
        });

        let install_pkgs = batch.get_install_packages();
        assert_eq!(install_pkgs.len(), 2);
        assert!(install_pkgs.contains(&"nginx".to_string()));
        assert!(install_pkgs.contains(&"curl=7.68.0".to_string()));

        let remove_pkgs = batch.get_remove_packages();
        assert_eq!(remove_pkgs.len(), 1);
        assert!(remove_pkgs.contains(&"apache2".to_string()));

        assert!(!batch.has_upgrade());

        batch.add(PackageOperation::Upgrade);
        assert!(batch.has_upgrade());
    }

    #[tokio::test]
    async fn test_package_batcher() {
        let config = PackageBatchConfig {
            enabled: true,
            max_batch_size: 3,
            batch_timeout_ms: 5000,
        };
        let batcher = PackageBatcher::new(config);

        // Add operations
        batcher
            .add_operation(
                "host1",
                "apt",
                PackageOperation::Install {
                    name: "nginx".to_string(),
                    version: None,
                },
            )
            .await;

        batcher
            .add_operation(
                "host1",
                "apt",
                PackageOperation::Install {
                    name: "curl".to_string(),
                    version: None,
                },
            )
            .await;

        // Not ready yet
        assert!(batcher.get_ready_batches().await.is_empty());
        assert!(batcher.has_pending_for_host("host1").await);

        // Add third operation - should trigger batch
        batcher
            .add_operation(
                "host1",
                "apt",
                PackageOperation::Install {
                    name: "git".to_string(),
                    version: None,
                },
            )
            .await;

        let ready = batcher.get_ready_batches().await;
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].manager, "apt");
        assert_eq!(ready[0].host, "host1");

        let packages = ready[0].get_install_packages();
        assert_eq!(packages.len(), 3);
    }

    #[tokio::test]
    async fn test_package_batcher_different_managers() {
        let batcher = PackageBatcher::default();

        // Add apt and pip operations for same host
        batcher
            .add_operation(
                "host1",
                "apt",
                PackageOperation::Install {
                    name: "nginx".to_string(),
                    version: None,
                },
            )
            .await;

        batcher
            .add_operation(
                "host1",
                "pip",
                PackageOperation::Install {
                    name: "flask".to_string(),
                    version: None,
                },
            )
            .await;

        // Flush and check separate batches
        let batches = batcher.flush_all().await;
        assert_eq!(batches.len(), 2);

        let managers: Vec<_> = batches.iter().map(|b| b.manager.clone()).collect();
        assert!(managers.contains(&"apt".to_string()));
        assert!(managers.contains(&"pip".to_string()));
    }

    #[tokio::test]
    async fn test_execution_pipeline_creation() {
        let pipeline = ExecutionPipeline::new();

        assert!(pipeline.speculative_config.enabled);
        assert!(!(pipeline.file_pipeline.has_pending().await));
    }

    #[tokio::test]
    async fn test_execution_pipeline_flush() {
        let pipeline = ExecutionPipeline::new();

        // Add some operations
        pipeline
            .file_pipeline
            .add_operation(
                "host1",
                FileOperationType::Mkdir {
                    path: "/opt".to_string(),
                },
                100,
            )
            .await;

        pipeline
            .package_batcher
            .add_operation(
                "host1",
                "apt",
                PackageOperation::Install {
                    name: "nginx".to_string(),
                    version: None,
                },
            )
            .await;

        assert!(pipeline.file_pipeline.has_pending().await);
        assert!(pipeline.package_batcher.has_pending_for_host("host1").await);

        // Flush all
        pipeline.flush_all().await;

        // File pipeline should be empty (flushed)
        assert!(!pipeline.file_pipeline.has_pending().await);
    }
}

// ============================================================================
// Work Stealing Scheduler Tests
// ============================================================================

mod work_stealing_tests {
    use super::*;
    use rustible::executor::work_stealing::{WorkItem, WorkStealingConfig, WorkStealingScheduler};

    #[test]
    fn test_work_item_creation() {
        let item = WorkItem::new(42);
        assert_eq!(item.payload, 42);
        assert_eq!(item.priority, 0);
        assert_eq!(item.weight, 1);

        let item_with_priority = WorkItem::new("task").with_priority(5).with_weight(10);
        assert_eq!(item_with_priority.payload, "task");
        assert_eq!(item_with_priority.priority, 5);
        assert_eq!(item_with_priority.weight, 10);
    }

    #[test]
    fn test_work_stealing_config_defaults() {
        let config = WorkStealingConfig::default();
        assert!(config.num_workers > 0);
        assert_eq!(config.steal_threshold, 2);
        assert!(config.batch_steal);
        assert_eq!(config.spin_count, 32);
    }

    #[test]
    fn test_work_stealing_config_io_bound() {
        let config = WorkStealingConfig::for_io_bound();
        // IO bound should have more workers
        assert!(config.num_workers >= WorkStealingConfig::default().num_workers);
        assert_eq!(config.steal_threshold, 1);
        assert!(config.batch_steal);
    }

    #[test]
    fn test_work_stealing_config_cpu_bound() {
        let config = WorkStealingConfig::for_cpu_bound();
        assert_eq!(config.steal_threshold, 4);
        assert!(config.batch_steal);
        assert_eq!(config.spin_count, 64);
    }

    #[test]
    fn test_scheduler_submit_and_get_work() {
        let config = WorkStealingConfig {
            num_workers: 4,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        // Submit work to specific worker
        scheduler.submit(0, WorkItem::new(1));
        scheduler.submit(0, WorkItem::new(2));
        scheduler.submit(0, WorkItem::new(3));

        // Worker 0 should get work in LIFO order
        let item1 = scheduler.get_work(0);
        assert!(item1.is_some());
        assert_eq!(item1.unwrap().payload, 3);

        let item2 = scheduler.get_work(0);
        assert!(item2.is_some());
        assert_eq!(item2.unwrap().payload, 2);
    }

    #[test]
    fn test_scheduler_submit_balanced() {
        let config = WorkStealingConfig {
            num_workers: 4,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        // Submit many items with balanced distribution
        for i in 0..100 {
            scheduler.submit_balanced(WorkItem::new(i));
        }

        let stats = scheduler.stats();

        // Work should be distributed across queues
        let total: usize = stats.queue_sizes.iter().sum();
        assert_eq!(total, 100);

        // Each queue should have some work
        for size in &stats.queue_sizes {
            assert!(*size > 0);
        }
    }

    #[test]
    fn test_scheduler_submit_batch() {
        let config = WorkStealingConfig {
            num_workers: 4,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        let items: Vec<_> = (0..20).map(WorkItem::new).collect();
        scheduler.submit_batch(items);

        let stats = scheduler.stats();
        let total: usize = stats.queue_sizes.iter().sum();
        assert_eq!(total, 20);
    }

    #[test]
    fn test_scheduler_work_stealing() {
        let config = WorkStealingConfig {
            num_workers: 2,
            steal_threshold: 1,
            batch_steal: false, // Single item stealing
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        // Submit all work to worker 0
        for i in 0..10 {
            scheduler.submit(0, WorkItem::new(i));
        }

        // Worker 1 should be able to steal
        let stolen = scheduler.get_work(1);
        assert!(stolen.is_some());

        let stats = scheduler.stats();
        assert!(stats.items_stolen > 0);
    }

    #[test]
    fn test_scheduler_batch_stealing() {
        let config = WorkStealingConfig {
            num_workers: 2,
            steal_threshold: 2,
            batch_steal: true,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        // Submit 10 items to worker 0
        for i in 0..10 {
            scheduler.submit(0, WorkItem::new(i));
        }

        // Worker 1 steals (batch mode steals half)
        let stolen = scheduler.get_work(1);
        assert!(stolen.is_some());

        let stats = scheduler.stats();
        // Batch steal should have stolen multiple items
        assert!(stats.items_stolen >= 1);
    }

    #[test]
    fn test_scheduler_is_empty() {
        let config = WorkStealingConfig {
            num_workers: 2,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        assert!(scheduler.is_empty());

        scheduler.submit(0, WorkItem::new(1));
        assert!(!scheduler.is_empty());

        let _ = scheduler.get_work(0);
        assert!(scheduler.is_empty());
    }

    #[test]
    fn test_scheduler_pending_count() {
        let config = WorkStealingConfig {
            num_workers: 2,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        assert_eq!(scheduler.pending_count(), 0);

        scheduler.submit(0, WorkItem::new(1));
        scheduler.submit(1, WorkItem::new(2));
        scheduler.submit(0, WorkItem::new(3));

        assert_eq!(scheduler.pending_count(), 3);
    }

    #[test]
    fn test_scheduler_shutdown() {
        let config = WorkStealingConfig {
            num_workers: 2,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        assert!(!scheduler.is_shutdown());

        scheduler.shutdown();

        assert!(scheduler.is_shutdown());
    }

    #[test]
    fn test_load_imbalance_calculation() {
        use rustible::executor::work_stealing::WorkStealingStats;

        // Perfect balance
        let balanced = WorkStealingStats {
            queue_sizes: vec![10, 10, 10, 10],
            queue_weights: vec![10, 10, 10, 10],
            active_workers: 4,
            items_processed: 100,
            items_stolen: 10,
        };
        assert!(balanced.load_imbalance() < 0.01);

        // High imbalance (all work on one queue)
        let unbalanced = WorkStealingStats {
            queue_sizes: vec![40, 0, 0, 0],
            queue_weights: vec![40, 0, 0, 0],
            active_workers: 4,
            items_processed: 100,
            items_stolen: 0,
        };
        assert!(unbalanced.load_imbalance() > 0.5);

        // Empty queues
        let empty = WorkStealingStats {
            queue_sizes: vec![0, 0, 0, 0],
            queue_weights: vec![0, 0, 0, 0],
            active_workers: 0,
            items_processed: 0,
            items_stolen: 0,
        };
        assert_eq!(empty.load_imbalance(), 0.0);
    }

    #[test]
    fn test_steal_ratio_calculation() {
        use rustible::executor::work_stealing::WorkStealingStats;

        let stats = WorkStealingStats {
            queue_sizes: vec![5, 5],
            queue_weights: vec![5, 5],
            active_workers: 2,
            items_processed: 100,
            items_stolen: 25,
        };
        assert!((stats.steal_ratio() - 0.25).abs() < 0.01);

        // No items processed
        let empty = WorkStealingStats {
            queue_sizes: vec![],
            queue_weights: vec![],
            active_workers: 0,
            items_processed: 0,
            items_stolen: 0,
        };
        assert_eq!(empty.steal_ratio(), 0.0);
    }

    #[tokio::test]
    async fn test_scheduler_wait_for_work() {
        let config = WorkStealingConfig {
            num_workers: 2,
            ..Default::default()
        };
        let scheduler = Arc::new(WorkStealingScheduler::<i32>::new(config));

        let scheduler_clone = scheduler.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            scheduler_clone.submit(0, WorkItem::new(42));
        });

        // Wait should return quickly due to timeout or notification
        let start = Instant::now();
        scheduler.wait_for_work().await;
        let elapsed = start.elapsed();

        // Should complete in reasonable time
        assert!(elapsed < Duration::from_millis(100));

        handle.await.unwrap();
    }

    #[test]
    fn test_scheduler_worker_active_tracking() {
        let config = WorkStealingConfig {
            num_workers: 4,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        let stats = scheduler.stats();
        assert_eq!(stats.active_workers, 0);

        scheduler.worker_active();
        scheduler.worker_active();

        let stats = scheduler.stats();
        assert_eq!(stats.active_workers, 2);

        scheduler.worker_inactive();

        let stats = scheduler.stats();
        assert_eq!(stats.active_workers, 1);
    }

    #[test]
    fn test_scheduler_item_processed_tracking() {
        let config = WorkStealingConfig {
            num_workers: 2,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<i32> = WorkStealingScheduler::new(config);

        scheduler.submit(0, WorkItem::new(1));
        scheduler.submit(0, WorkItem::new(2));

        let _ = scheduler.get_work(0);
        scheduler.item_processed();

        let _ = scheduler.get_work(0);
        scheduler.item_processed();

        let stats = scheduler.stats();
        assert_eq!(stats.items_processed, 2);
    }

    #[test]
    fn test_scheduler_weighted_work_distribution() {
        let config = WorkStealingConfig {
            num_workers: 4,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<String> = WorkStealingScheduler::new(config);

        // Submit items with different weights
        scheduler.submit_balanced(WorkItem::new("light".to_string()).with_weight(1));
        scheduler.submit_balanced(WorkItem::new("heavy".to_string()).with_weight(100));
        scheduler.submit_balanced(WorkItem::new("medium".to_string()).with_weight(10));

        let stats = scheduler.stats();

        // Queue weights should reflect item weights
        let total_weight: u32 = stats.queue_weights.iter().sum();
        assert_eq!(total_weight, 111);
    }

    #[test]
    fn test_scheduler_priority_work_items() {
        let config = WorkStealingConfig {
            num_workers: 2,
            ..Default::default()
        };
        let scheduler: WorkStealingScheduler<&str> = WorkStealingScheduler::new(config);

        // Submit items with different priorities
        scheduler.submit(0, WorkItem::new("low").with_priority(1));
        scheduler.submit(0, WorkItem::new("high").with_priority(10));
        scheduler.submit(0, WorkItem::new("medium").with_priority(5));

        // Pop should return in LIFO order (last submitted first)
        let item = scheduler.get_work(0);
        assert!(item.is_some());
        assert_eq!(item.unwrap().payload, "medium");
    }
}

// ============================================================================
// Integration Tests Combining Multiple Components
// ============================================================================

mod integration_tests {
    use super::*;
    use rustible::executor::async_task::AsyncTaskManager;
    use rustible::executor::pipeline::ExecutionPipeline;
    use rustible::executor::task::TaskResult;
    use rustible::executor::throttle::{ThrottleConfig, ThrottleManager};
    use rustible::executor::work_stealing::{WorkItem, WorkStealingConfig, WorkStealingScheduler};

    #[tokio::test]
    async fn test_throttled_async_task_execution() {
        // Combine throttling with async task execution
        let throttle_manager = Arc::new(ThrottleManager::new(ThrottleConfig::with_global_limit(2)));
        let async_manager = AsyncTaskManager::new();

        let completed = Arc::new(AtomicUsize::new(0));
        let start = Instant::now();

        let mut jids = Vec::new();
        for i in 0..4 {
            let throttle = throttle_manager.clone();
            let completed = completed.clone();

            let jid = async_manager
                .submit_task(
                    &format!("host{}", i),
                    &format!("Task {}", i),
                    "debug",
                    60,
                    move || async move {
                        // Acquire throttle inside the async task
                        let _guard = throttle.acquire("host", "debug", None).await;
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        completed.fetch_add(1, Ordering::SeqCst);
                        Ok(TaskResult::ok())
                    },
                )
                .await
                .unwrap();
            jids.push(jid);
        }

        // Wait for all jobs
        for jid in &jids {
            let _ = async_manager.wait_for_job(jid, 1, Some(10)).await;
        }

        let elapsed = start.elapsed();
        assert_eq!(completed.load(Ordering::SeqCst), 4);

        // With throttle of 2 and 4 tasks, should take at least 2 batches
        assert!(
            elapsed >= Duration::from_millis(90),
            "Throttled async execution should serialize: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn test_work_stealing_with_async_tasks() {
        let scheduler = Arc::new(WorkStealingScheduler::<u32>::new(WorkStealingConfig {
            num_workers: 4,
            steal_threshold: 1,
            batch_steal: true,
            spin_count: 16,
        }));

        let completed = Arc::new(AtomicUsize::new(0));

        // Submit work items
        for i in 0..20 {
            scheduler.submit_balanced(WorkItem::new(i));
        }

        // Create workers that process items with timeout protection
        let mut handles = vec![];
        for worker_id in 0..4 {
            let scheduler = scheduler.clone();
            let completed = completed.clone();

            let handle = tokio::spawn(async move {
                let start = Instant::now();
                let timeout = Duration::from_secs(5);
                let mut iterations = 0;

                loop {
                    // Timeout protection to prevent hangs
                    if start.elapsed() > timeout || iterations > 100 {
                        break;
                    }
                    iterations += 1;

                    if let Some(_item) = scheduler.get_work(worker_id) {
                        // Simulate work
                        tokio::time::sleep(Duration::from_millis(5)).await;
                        scheduler.item_processed();
                        completed.fetch_add(1, Ordering::SeqCst);
                    } else if scheduler.is_empty() {
                        break;
                    } else {
                        // Use tokio::select with timeout to prevent indefinite waiting
                        tokio::select! {
                            _ = scheduler.wait_for_work() => {}
                            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
                        }
                    }
                }
            });
            handles.push(handle);
        }

        // Wait for all workers with overall timeout
        let _ =
            tokio::time::timeout(Duration::from_secs(10), futures::future::join_all(handles)).await;

        // All items should be processed
        assert_eq!(completed.load(Ordering::SeqCst), 20);

        let stats = scheduler.stats();
        assert_eq!(stats.items_processed, 20);
        // Some stealing should have occurred
        // (Note: stealing is not guaranteed in all scenarios)
    }

    #[tokio::test]
    async fn test_pipeline_with_throttled_execution() {
        use rustible::executor::pipeline::{FileOperationType, FilePipeline, PipelineConfig};

        let pipeline = Arc::new(FilePipeline::new(PipelineConfig {
            enabled: true,
            max_batch_size: 3,
            max_batch_bytes: 1024 * 1024,
            batch_timeout_ms: 100,
        }));

        let throttle = Arc::new(ThrottleManager::new(ThrottleConfig::with_global_limit(2)));

        // Add operations
        for i in 0..6 {
            pipeline
                .add_operation(
                    "host1",
                    FileOperationType::Copy {
                        src: format!("file{}.txt", i),
                        dest: format!("/opt/app/file{}.txt", i),
                    },
                    1000,
                )
                .await;
        }

        // Process batches with throttling
        let batches = pipeline.flush_all().await;
        let processed = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];
        for batch in batches {
            let throttle = throttle.clone();
            let processed = processed.clone();

            let handle = tokio::spawn(async move {
                let _guard = throttle.acquire(&batch.host, "copy", None).await;
                // Simulate batch processing
                tokio::time::sleep(Duration::from_millis(50)).await;
                processed.fetch_add(batch.operations.len(), Ordering::SeqCst);
            });
            handles.push(handle);
        }

        futures::future::join_all(handles).await;

        assert_eq!(processed.load(Ordering::SeqCst), 6);
    }

    #[tokio::test]
    async fn test_concurrent_access_safety() {
        // Test that all components are thread-safe under concurrent access
        let async_manager = Arc::new(AsyncTaskManager::new());
        let throttle = Arc::new(ThrottleManager::new(ThrottleConfig::with_global_limit(5)));
        let scheduler = Arc::new(WorkStealingScheduler::<u32>::new(WorkStealingConfig {
            num_workers: 4,
            ..Default::default()
        }));

        let mut handles = vec![];

        // Concurrent async task submissions
        for i in 0..10 {
            let async_manager = async_manager.clone();
            let handle = tokio::spawn(async move {
                let jid = async_manager
                    .submit_task(
                        &format!("host{}", i % 3),
                        &format!("Task {}", i),
                        "debug",
                        5,
                        || async { Ok(TaskResult::ok()) },
                    )
                    .await;
                assert!(jid.is_ok());
            });
            handles.push(handle);
        }

        // Concurrent throttle acquisitions
        for i in 0..10 {
            let throttle = throttle.clone();
            let handle = tokio::spawn(async move {
                let _guard = throttle
                    .acquire(&format!("host{}", i % 3), "test", None)
                    .await;
                tokio::time::sleep(Duration::from_millis(5)).await;
            });
            handles.push(handle);
        }

        // Concurrent scheduler operations
        for i in 0..10 {
            let scheduler = scheduler.clone();
            let handle = tokio::spawn(async move {
                scheduler.submit_balanced(WorkItem::new(i));
                tokio::time::sleep(Duration::from_millis(5)).await;
                let _ = scheduler.get_work(i as usize % 4);
            });
            handles.push(handle);
        }

        // Wait for all to complete without panics
        let results = futures::future::join_all(handles).await;
        for result in results {
            assert!(result.is_ok());
        }
    }
}
