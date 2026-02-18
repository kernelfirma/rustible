//! Remote State Backends Reliability Tests (#300)
//!
//! Integration tests for GCS/Azure/Consul backends with locking and retries.
//! Tests verify:
//! - Backend configuration and creation
//! - Load/save/delete operations with retries
//! - Locking behavior and concurrency protection
//! - Error handling and recovery scenarios
//! - Backend-specific features

#![cfg(feature = "provisioning")]

use rustible::provisioning::state::{ProvisioningState, ResourceId, ResourceState};
use rustible::provisioning::state_backends::{
    BackendConfig, ConsulBackend, HttpBackend, LocalBackend, MemoryBackend, StateBackend,
};
use rustible::provisioning::state_lock::LockInfo;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::Barrier;

// ============================================================================
// Test Helpers
// ============================================================================

fn create_test_state() -> ProvisioningState {
    let mut state = ProvisioningState::new();
    let resource = ResourceState::new(
        ResourceId::new("aws_vpc", "test"),
        "vpc-123",
        "aws",
        json!({"cidr_block": "10.0.0.0/16"}),
        json!({"id": "vpc-123", "arn": "arn:aws:ec2:us-east-1:123456789012:vpc/vpc-123"}),
    );
    state.add_resource(resource);
    state
}

fn create_larger_test_state(resource_count: usize) -> ProvisioningState {
    let mut state = ProvisioningState::new();
    for i in 0..resource_count {
        let resource = ResourceState::new(
            ResourceId::new("aws_instance", format!("server_{}", i)),
            format!("i-{:08x}", i),
            "aws",
            json!({
                "ami": "ami-12345678",
                "instance_type": "t3.micro",
                "tags": {
                    "Name": format!("server-{}", i),
                    "Index": i
                }
            }),
            json!({
                "id": format!("i-{:08x}", i),
                "public_ip": format!("10.0.{}.{}", i / 256, i % 256),
                "private_ip": format!("192.168.{}.{}", i / 256, i % 256)
            }),
        );
        state.add_resource(resource);
    }
    state
}

// ============================================================================
// Consul Backend Tests
// ============================================================================

#[test]
fn test_consul_backend_creation() {
    let backend = ConsulBackend::new("rustible/state".to_string());
    assert_eq!(backend.name(), "consul");
    assert_eq!(backend.path(), "rustible/state");
}

#[test]
fn test_consul_backend_with_custom_address() {
    let backend = ConsulBackend::new("terraform/state".to_string())
        .with_address("http://consul.example.com:8500");

    assert_eq!(backend.address(), "http://consul.example.com:8500");
    assert_eq!(backend.path(), "terraform/state");
}

#[test]
fn test_consul_backend_with_token() {
    let backend = ConsulBackend::new("terraform/state".to_string()).with_token("my-secret-token");

    assert_eq!(backend.name(), "consul");
}

#[test]
fn test_consul_backend_with_custom_timeout() {
    let backend =
        ConsulBackend::new("terraform/state".to_string()).with_timeout(Duration::from_secs(60));

    assert_eq!(backend.name(), "consul");
}

#[test]
fn test_consul_backend_has_session_lock() {
    let backend = ConsulBackend::new("terraform/state".to_string());
    let lock_backend = backend.lock_backend();
    assert!(lock_backend.is_some());
    assert_eq!(lock_backend.unwrap().backend_name(), "consul");
}

#[test]
fn test_consul_backend_without_session_lock() {
    let backend = ConsulBackend::new("terraform/state".to_string()).without_session_lock();
    assert!(backend.lock_backend().is_none());
}

#[tokio::test]
async fn test_consul_config_creation() {
    let config = BackendConfig::Consul {
        address: Some("http://localhost:8500".to_string()),
        path: "terraform/state".to_string(),
        token: Some("test-token".to_string()),
    };

    let backend = config.create_backend().await.unwrap();
    assert_eq!(backend.name(), "consul");
    assert!(backend.lock_backend().is_some());
}

#[tokio::test]
async fn test_consul_config_with_defaults() {
    let config = BackendConfig::Consul {
        address: None,
        path: "terraform/state".to_string(),
        token: None,
    };

    let backend = config.create_backend().await.unwrap();
    assert_eq!(backend.name(), "consul");
}

#[test]
fn test_consul_config_serialization() {
    let config = BackendConfig::Consul {
        address: Some("http://consul.example.com:8500".to_string()),
        path: "terraform/state".to_string(),
        token: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("consul"));
    assert!(json.contains("terraform/state"));

    let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();
    match deserialized {
        BackendConfig::Consul { address, path, .. } => {
            assert_eq!(address, Some("http://consul.example.com:8500".to_string()));
            assert_eq!(path, "terraform/state");
        }
        _ => panic!("Expected Consul config"),
    }
}

// ============================================================================
// HTTP Backend Tests (Terraform Cloud Compatible)
// ============================================================================

#[test]
fn test_http_backend_creation() {
    let backend = HttpBackend::new("http://localhost:8080/state".to_string());
    assert_eq!(backend.name(), "http");
}

#[test]
fn test_http_backend_with_lock_addresses() {
    let backend = HttpBackend::new("http://localhost:8080/state".to_string())
        .with_lock_address("http://localhost:8080/lock")
        .with_unlock_address("http://localhost:8080/unlock");

    assert_eq!(backend.name(), "http");
    assert!(backend.lock_backend().is_some());
}

#[test]
fn test_http_backend_with_auth() {
    let backend = HttpBackend::new("http://localhost:8080/state".to_string())
        .with_auth("user", "password")
        .with_lock_address("http://localhost:8080/lock");

    assert_eq!(backend.name(), "http");
}

#[test]
fn test_http_backend_with_timeout() {
    let backend = HttpBackend::new("http://localhost:8080/state".to_string())
        .with_timeout(Duration::from_secs(120));

    assert_eq!(backend.name(), "http");
}

#[test]
fn test_http_backend_no_lock_without_address() {
    let backend = HttpBackend::new("http://localhost:8080/state".to_string());
    assert!(backend.lock_backend().is_none());
}

#[tokio::test]
async fn test_http_config_creation() {
    let config = BackendConfig::Http {
        address: "http://localhost:8080/state".to_string(),
        lock_address: Some("http://localhost:8080/lock".to_string()),
        unlock_address: Some("http://localhost:8080/unlock".to_string()),
        username: Some("user".to_string()),
        password: Some("pass".to_string()),
    };

    let backend = config.create_backend().await.unwrap();
    assert_eq!(backend.name(), "http");
    assert!(backend.lock_backend().is_some());
}

#[test]
fn test_http_config_serialization() {
    let config = BackendConfig::Http {
        address: "https://app.terraform.io/api/v2/workspaces/ws-123/current-state-version"
            .to_string(),
        lock_address: Some("https://app.terraform.io/api/v2/workspaces/ws-123/locks".to_string()),
        unlock_address: Some("https://app.terraform.io/api/v2/workspaces/ws-123/locks".to_string()),
        username: None,
        password: None,
    };

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("terraform.io"));

    let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();
    match deserialized {
        BackendConfig::Http { address, .. } => {
            assert!(address.contains("terraform.io"));
        }
        _ => panic!("Expected HTTP config"),
    }
}

// ============================================================================
// GCS Backend Config Tests (Feature-gated)
// ============================================================================

#[test]
fn test_gcs_config_serialization() {
    let config = BackendConfig::Gcs {
        bucket: "my-terraform-state".to_string(),
        key: "env/production/terraform.tfstate".to_string(),
    };

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("my-terraform-state"));
    assert!(json.contains("env/production/terraform.tfstate"));

    let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();
    match deserialized {
        BackendConfig::Gcs { bucket, key } => {
            assert_eq!(bucket, "my-terraform-state");
            assert_eq!(key, "env/production/terraform.tfstate");
        }
        _ => panic!("Expected GCS config"),
    }
}

#[test]
fn test_gcs_config_with_nested_key() {
    let config = BackendConfig::Gcs {
        bucket: "company-terraform".to_string(),
        key: "projects/web-app/envs/staging/terraform.tfstate".to_string(),
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();

    match deserialized {
        BackendConfig::Gcs { key, .. } => {
            assert!(key.contains("projects/web-app"));
            assert!(key.contains("staging"));
        }
        _ => panic!("Expected GCS config"),
    }
}

// ============================================================================
// Azure Blob Backend Config Tests (Feature-gated)
// ============================================================================

#[test]
fn test_azure_config_serialization() {
    let config = BackendConfig::AzureBlob {
        storage_account: "myterraformstate".to_string(),
        container: "tfstate".to_string(),
        name: "terraform.tfstate".to_string(),
    };

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("myterraformstate"));
    assert!(json.contains("tfstate"));

    let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();
    match deserialized {
        BackendConfig::AzureBlob {
            storage_account,
            container,
            name,
        } => {
            assert_eq!(storage_account, "myterraformstate");
            assert_eq!(container, "tfstate");
            assert_eq!(name, "terraform.tfstate");
        }
        _ => panic!("Expected Azure config"),
    }
}

#[test]
fn test_azure_config_with_env_prefix() {
    let config = BackendConfig::AzureBlob {
        storage_account: "productionstate".to_string(),
        container: "terraform".to_string(),
        name: "prod/web-app.tfstate".to_string(),
    };

    let json = serde_json::to_string(&config).unwrap();
    let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();

    match deserialized {
        BackendConfig::AzureBlob { name, .. } => {
            assert!(name.starts_with("prod/"));
        }
        _ => panic!("Expected Azure config"),
    }
}

// ============================================================================
// Memory Backend Reliability Tests (Simulates Remote Backend Behavior)
// ============================================================================

#[tokio::test]
async fn test_memory_backend_load_save_cycle() {
    let backend = MemoryBackend::new();

    // Initially no state
    assert!(!backend.exists().await.unwrap());
    assert!(backend.load().await.unwrap().is_none());

    // Save state
    let state = create_test_state();
    backend.save(&state).await.unwrap();

    // Load and verify
    assert!(backend.exists().await.unwrap());
    let loaded = backend.load().await.unwrap().unwrap();
    assert_eq!(loaded.resources.len(), 1);
}

#[tokio::test]
async fn test_memory_backend_multiple_save_load_cycles() {
    let backend = MemoryBackend::new();

    for i in 0..5 {
        let mut state = create_test_state();
        state.resources.values_mut().for_each(|r| {
            r.attributes["cycle"] = json!(i);
        });

        backend.save(&state).await.unwrap();
        let loaded = backend.load().await.unwrap().unwrap();

        assert_eq!(loaded.resources.len(), 1);
    }
}

#[tokio::test]
async fn test_memory_backend_delete_cycle() {
    let backend = MemoryBackend::new();

    // Save and verify
    let state = create_test_state();
    backend.save(&state).await.unwrap();
    assert!(backend.exists().await.unwrap());

    // Delete
    backend.delete().await.unwrap();
    assert!(!backend.exists().await.unwrap());
    assert!(backend.load().await.unwrap().is_none());

    // Save again
    backend.save(&state).await.unwrap();
    assert!(backend.exists().await.unwrap());
}

#[tokio::test]
async fn test_memory_backend_large_state() {
    let backend = MemoryBackend::new();

    // Create state with many resources
    let state = create_larger_test_state(100);
    assert_eq!(state.resources.len(), 100);

    // Save
    backend.save(&state).await.unwrap();

    // Load and verify
    let loaded = backend.load().await.unwrap().unwrap();
    assert_eq!(loaded.resources.len(), 100);
}

// ============================================================================
// Locking Reliability Tests
// ============================================================================

#[tokio::test]
async fn test_memory_backend_basic_locking() {
    let backend = MemoryBackend::new();
    let lock_backend = backend.lock_backend().unwrap();

    // Acquire lock
    let lock_info = LockInfo::new("apply");
    assert!(lock_backend
        .acquire(&lock_info, Duration::from_millis(100))
        .await
        .unwrap());

    // Verify lock is held
    let current_lock = lock_backend.get_lock().await.unwrap();
    assert!(current_lock.is_some());

    // Release lock
    assert!(lock_backend.release(&lock_info.id).await.unwrap());

    // Verify lock is released
    let current_lock = lock_backend.get_lock().await.unwrap();
    assert!(current_lock.is_none());
}

#[tokio::test]
async fn test_memory_backend_lock_prevents_double_lock() {
    let backend = MemoryBackend::new();
    let lock_backend = backend.lock_backend().unwrap();

    // First lock succeeds
    let lock_info1 = LockInfo::new("apply");
    assert!(lock_backend
        .acquire(&lock_info1, Duration::from_millis(100))
        .await
        .unwrap());

    // Second lock fails (already locked)
    let lock_info2 = LockInfo::new("destroy");
    assert!(!lock_backend
        .acquire(&lock_info2, Duration::from_millis(100))
        .await
        .unwrap());

    // Release first lock
    lock_backend.release(&lock_info1.id).await.unwrap();

    // Now second lock succeeds
    assert!(lock_backend
        .acquire(&lock_info2, Duration::from_millis(100))
        .await
        .unwrap());
}

#[tokio::test]
async fn test_memory_backend_force_unlock() {
    let backend = MemoryBackend::new();
    let lock_backend = backend.lock_backend().unwrap();

    // Acquire lock
    let lock_info = LockInfo::new("apply");
    lock_backend
        .acquire(&lock_info, Duration::from_millis(100))
        .await
        .unwrap();

    // Force unlock
    lock_backend.force_unlock(&lock_info.id).await.unwrap();

    // Lock should be gone
    let current_lock = lock_backend.get_lock().await.unwrap();
    assert!(current_lock.is_none());

    // New lock should work
    let lock_info2 = LockInfo::new("destroy");
    assert!(lock_backend
        .acquire(&lock_info2, Duration::from_millis(100))
        .await
        .unwrap());
}

#[tokio::test]
async fn test_lock_info_creation() {
    let lock_info = LockInfo::new("apply");
    assert_eq!(lock_info.operation, "apply");
    assert!(!lock_info.is_expired());
    assert!(lock_info.info.is_none());

    let lock_info2 = LockInfo::new("destroy").with_info("Destroying all resources");
    assert_eq!(lock_info2.operation, "destroy");
    assert_eq!(
        lock_info2.info,
        Some("Destroying all resources".to_string())
    );
}

#[tokio::test]
async fn test_lock_info_with_expiration() {
    let lock_info = LockInfo::with_expiration("apply", Duration::from_secs(10));
    assert!(!lock_info.is_expired());
    assert!(lock_info.expires_at.is_some());

    // Very short expiration for testing
    let expired_lock = LockInfo::with_expiration("apply", Duration::from_millis(1));
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(expired_lock.is_expired());
}

#[tokio::test]
async fn test_lock_info_display() {
    let lock_info = LockInfo::new("apply").with_info("Running terraform apply");

    let display = format!("{}", lock_info);
    assert!(display.contains("apply"));
    assert!(display.contains("Running terraform apply"));
}

// ============================================================================
// Concurrent Lock Tests
// ============================================================================

#[tokio::test]
async fn test_concurrent_lock_attempts() {
    let backend = Arc::new(MemoryBackend::new());
    let lock_backend = backend.lock_backend().unwrap();

    let barrier = Arc::new(Barrier::new(3));
    let lock_results = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let mut handles = vec![];

    for i in 0..3 {
        let lock_backend_clone = Arc::clone(&lock_backend);
        let barrier_clone = Arc::clone(&barrier);
        let results_clone = Arc::clone(&lock_results);

        let handle = tokio::spawn(async move {
            barrier_clone.wait().await;
            let lock_info = LockInfo::new(format!("worker_{}", i));
            let result = lock_backend_clone
                .acquire(&lock_info, Duration::from_millis(10))
                .await
                .unwrap();
            results_clone.lock().await.push((i, result, lock_info.id));
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let results = lock_results.lock().await;

    // Exactly one should have acquired the lock
    let successful_locks: Vec<_> = results.iter().filter(|(_, success, _)| *success).collect();
    assert_eq!(
        successful_locks.len(),
        1,
        "Exactly one worker should acquire the lock"
    );

    // Release the lock
    let (_, _, lock_id) = &successful_locks[0];
    lock_backend.release(lock_id).await.unwrap();
}

#[tokio::test]
async fn test_lock_release_allows_new_lock() {
    let backend = MemoryBackend::new();
    let lock_backend = backend.lock_backend().unwrap();

    // Acquire and release multiple times
    for i in 0..5 {
        let lock_info = LockInfo::new(format!("operation_{}", i));

        // Acquire
        let acquired = lock_backend
            .acquire(&lock_info, Duration::from_millis(100))
            .await
            .unwrap();
        assert!(acquired, "Should acquire lock on iteration {}", i);

        // Release
        let released = lock_backend.release(&lock_info.id).await.unwrap();
        assert!(released, "Should release lock on iteration {}", i);
    }
}

// ============================================================================
// Local Backend Reliability Tests
// ============================================================================

#[tokio::test]
async fn test_local_backend_atomic_save() {
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("state.json");
    let backend = LocalBackend::new(state_path.clone());

    let state = create_test_state();
    backend.save(&state).await.unwrap();

    // Verify no temp file remains
    let tmp_path = state_path.with_extension("tmp");
    assert!(!tmp_path.exists(), "Temp file should be cleaned up");

    // Verify state file is valid JSON
    let content = std::fs::read_to_string(&state_path).unwrap();
    let _: ProvisioningState = serde_json::from_str(&content).unwrap();
}

#[tokio::test]
async fn test_local_backend_creates_parent_dirs() {
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("nested/deeply/state.json");
    let backend = LocalBackend::new(state_path.clone());

    // Parent doesn't exist
    assert!(!state_path.parent().unwrap().exists());

    // Save creates parent directories
    let state = create_test_state();
    backend.save(&state).await.unwrap();

    assert!(state_path.parent().unwrap().exists());
    assert!(state_path.exists());
}

#[tokio::test]
async fn test_local_backend_has_file_lock() {
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("state.json");
    let backend = LocalBackend::new(state_path);

    let lock_backend = backend.lock_backend();
    assert!(lock_backend.is_some());
    assert_eq!(lock_backend.unwrap().backend_name(), "file");
}

#[tokio::test]
async fn test_local_backend_sequential_saves() {
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("state.json");
    let backend = LocalBackend::new(state_path.clone());

    // Sequential saves should all succeed
    for i in 0..5 {
        let mut state = create_test_state();
        state.resources.values_mut().for_each(|r| {
            r.attributes["writer"] = json!(i);
        });
        backend.save(&state).await.unwrap();

        // Verify after each save
        let loaded = backend.load().await.unwrap().unwrap();
        let saved_writer = loaded.resources.values().next().unwrap().attributes["writer"]
            .as_i64()
            .unwrap();
        assert_eq!(saved_writer, i as i64);
    }

    // Final state should be valid
    let loaded = backend.load().await.unwrap();
    assert!(loaded.is_some());
}

// ============================================================================
// Backend Config Tests
// ============================================================================

#[test]
fn test_backend_config_default_is_local() {
    let config = BackendConfig::default();
    match config {
        BackendConfig::Local { path } => {
            assert_eq!(path, PathBuf::from(".rustible/provisioning.state.json"));
        }
        _ => panic!("Expected local backend"),
    }
}

#[tokio::test]
async fn test_local_config_creation() {
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("state.json");

    let config = BackendConfig::Local {
        path: state_path.clone(),
    };

    let backend = config.create_backend().await.unwrap();
    assert_eq!(backend.name(), "local");
}

#[test]
fn test_s3_config_serialization() {
    let config = BackendConfig::S3 {
        bucket: "my-terraform-state".to_string(),
        key: "env/prod/terraform.tfstate".to_string(),
        region: "us-east-1".to_string(),
        encrypt: true,
        dynamodb_table: Some("terraform-locks".to_string()),
    };

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("my-terraform-state"));
    assert!(json.contains("dynamodb_table"));

    let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();
    match deserialized {
        BackendConfig::S3 {
            bucket,
            key,
            region,
            encrypt,
            dynamodb_table,
        } => {
            assert_eq!(bucket, "my-terraform-state");
            assert_eq!(key, "env/prod/terraform.tfstate");
            assert_eq!(region, "us-east-1");
            assert!(encrypt);
            assert_eq!(dynamodb_table, Some("terraform-locks".to_string()));
        }
        _ => panic!("Expected S3 config"),
    }
}

#[test]
fn test_all_backend_configs_roundtrip() {
    let configs = vec![
        BackendConfig::Local {
            path: PathBuf::from("/tmp/state.json"),
        },
        BackendConfig::S3 {
            bucket: "bucket".to_string(),
            key: "key".to_string(),
            region: "us-east-1".to_string(),
            encrypt: false,
            dynamodb_table: None,
        },
        BackendConfig::Gcs {
            bucket: "bucket".to_string(),
            key: "key".to_string(),
        },
        BackendConfig::AzureBlob {
            storage_account: "account".to_string(),
            container: "container".to_string(),
            name: "name".to_string(),
        },
        BackendConfig::Consul {
            address: None,
            path: "path".to_string(),
            token: None,
        },
        BackendConfig::Http {
            address: "http://localhost".to_string(),
            lock_address: None,
            unlock_address: None,
            username: None,
            password: None,
        },
    ];

    for config in configs {
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: BackendConfig = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(json, json2, "Config should roundtrip: {:?}", config);
    }
}

// ============================================================================
// Retry Simulation Tests
// ============================================================================

/// Simulates retry behavior by tracking attempts
struct RetryTracker {
    attempts: std::sync::atomic::AtomicUsize,
    succeed_on_attempt: usize,
}

impl RetryTracker {
    fn new(succeed_on_attempt: usize) -> Self {
        Self {
            attempts: std::sync::atomic::AtomicUsize::new(0),
            succeed_on_attempt,
        }
    }

    fn try_operation(&self) -> bool {
        let attempt = self
            .attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        attempt >= self.succeed_on_attempt
    }

    fn attempts(&self) -> usize {
        self.attempts.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[tokio::test]
async fn test_retry_simulation_immediate_success() {
    let tracker = RetryTracker::new(1);

    for _ in 0..3 {
        if tracker.try_operation() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(tracker.attempts(), 1);
}

#[tokio::test]
async fn test_retry_simulation_eventual_success() {
    let tracker = RetryTracker::new(3);

    for _ in 0..5 {
        if tracker.try_operation() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(tracker.attempts(), 3);
}

#[tokio::test]
async fn test_retry_with_exponential_backoff_pattern() {
    let tracker = RetryTracker::new(3);
    let mut backoff_ms = 10;

    for _ in 0..5 {
        if tracker.try_operation() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(1000);
    }

    assert_eq!(tracker.attempts(), 3);
}

// ============================================================================
// State Serialization Tests
// ============================================================================

#[test]
fn test_state_serialization_roundtrip() {
    let state = create_test_state();
    let json = serde_json::to_string(&state).unwrap();
    let deserialized: ProvisioningState = serde_json::from_str(&json).unwrap();

    assert_eq!(state.lineage, deserialized.lineage);
    assert_eq!(state.resources.len(), deserialized.resources.len());
}

#[test]
fn test_large_state_serialization() {
    let state = create_larger_test_state(50);
    let json = serde_json::to_string(&state).unwrap();

    // Should be reasonably sized
    assert!(json.len() < 1_000_000, "State JSON should be under 1MB");

    let deserialized: ProvisioningState = serde_json::from_str(&json).unwrap();
    assert_eq!(state.resources.len(), deserialized.resources.len());
}

#[test]
fn test_state_with_special_characters() {
    let mut state = ProvisioningState::new();
    let resource = ResourceState::new(
        ResourceId::new("aws_instance", "test-with-special"),
        "i-12345678",
        "aws",
        json!({
            "user_data": "#!/bin/bash\necho 'Hello \"World\"'\necho $HOME",
            "tags": {
                "Name": "Test <Instance>",
                "Description": "Contains 'quotes' and \"double quotes\""
            }
        }),
        json!({"id": "i-12345678"}),
    );
    state.add_resource(resource);

    let json = serde_json::to_string(&state).unwrap();
    let deserialized: ProvisioningState = serde_json::from_str(&json).unwrap();

    assert_eq!(state.resources.len(), deserialized.resources.len());
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_local_backend_load_nonexistent() {
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("nonexistent.json");
    let backend = LocalBackend::new(state_path);

    // Should return None, not error
    let result = backend.load().await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_local_backend_exists_nonexistent() {
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("nonexistent.json");
    let backend = LocalBackend::new(state_path);

    assert!(!backend.exists().await.unwrap());
}

#[tokio::test]
async fn test_local_backend_delete_nonexistent() {
    let temp_dir = TempDir::new().unwrap();
    let state_path = temp_dir.path().join("nonexistent.json");
    let backend = LocalBackend::new(state_path);

    // Should not error
    backend.delete().await.unwrap();
}

#[tokio::test]
async fn test_memory_backend_delete_empty() {
    let backend = MemoryBackend::new();

    // Should not error
    backend.delete().await.unwrap();
    assert!(!backend.exists().await.unwrap());
}

#[tokio::test]
async fn test_lock_release_without_acquire() {
    let backend = MemoryBackend::new();
    let lock_backend = backend.lock_backend().unwrap();

    // Releasing non-existent lock should return false
    let released = lock_backend.release("nonexistent-id").await.unwrap();
    assert!(!released);
}
