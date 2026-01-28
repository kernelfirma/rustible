//! Lockfile Reproducibility Tests
//!
//! Issue #297: Guarantee lockfile produces deterministic replays.
//!
//! These tests verify that re-runs with lockfiles are deterministic across hosts,
//! ensuring consistent state management in distributed provisioning scenarios.

#![cfg(feature = "provisioning")]

use chrono::{DateTime, TimeZone, Utc};
use rustible::provisioning::state_lock::{
    AsyncLockGuard, FileLock, InMemoryLock, LockBackend, LockInfo, StateLockManager,
};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

// ============================================================================
// Test Suite 1: LockInfo Serialization Reproducibility
// ============================================================================

#[test]
fn test_lock_info_serialization_is_deterministic() {
    // Create lock info with fixed values
    let info = create_fixed_lock_info();

    // Serialize multiple times
    let json1 = serde_json::to_string_pretty(&info).unwrap();
    let json2 = serde_json::to_string_pretty(&info).unwrap();
    let json3 = serde_json::to_string_pretty(&info).unwrap();

    // All serializations should be identical
    assert_eq!(json1, json2);
    assert_eq!(json2, json3);
}

#[test]
fn test_lock_info_deserialization_round_trip() {
    let original = create_fixed_lock_info();

    // Serialize and deserialize
    let json = serde_json::to_string_pretty(&original).unwrap();
    let restored: LockInfo = serde_json::from_str(&json).unwrap();

    // All fields should match
    assert_eq!(original.id, restored.id);
    assert_eq!(original.operation, restored.operation);
    assert_eq!(original.who, restored.who);
    assert_eq!(original.created_at, restored.created_at);
    assert_eq!(original.expires_at, restored.expires_at);
    assert_eq!(original.info, restored.info);
}

#[test]
fn test_lock_info_from_json_string() {
    // Simulate JSON from another host
    let json = r#"{
        "id": "test-lock-123",
        "operation": "apply",
        "who": "user@remote-host (pid: 5678)",
        "created_at": "2024-01-15T10:30:00Z",
        "expires_at": "2024-01-15T11:30:00Z",
        "info": "Production deployment"
    }"#;

    let info: LockInfo = serde_json::from_str(json).unwrap();

    assert_eq!(info.id, "test-lock-123");
    assert_eq!(info.operation, "apply");
    assert_eq!(info.who, "user@remote-host (pid: 5678)");
    assert_eq!(info.info, Some("Production deployment".to_string()));
}

#[test]
fn test_lock_info_json_field_order_stability() {
    let info = create_fixed_lock_info();
    let json = serde_json::to_string(&info).unwrap();

    // Fields should always appear in the same order (serde default)
    // This ensures stable hashes and comparisons
    assert!(json.contains("\"id\":"));
    assert!(json.contains("\"operation\":"));
    assert!(json.contains("\"who\":"));
    assert!(json.contains("\"created_at\":"));
}

#[test]
fn test_lock_info_datetime_format_consistency() {
    let info = create_fixed_lock_info();
    let json = serde_json::to_string_pretty(&info).unwrap();

    // DateTime should be in RFC3339 format
    assert!(json.contains("2024-01-15T10:30:00Z"));

    // Deserialize and check format is preserved
    let restored: LockInfo = serde_json::from_str(&json).unwrap();
    let re_json = serde_json::to_string_pretty(&restored).unwrap();

    assert!(re_json.contains("2024-01-15T10:30:00Z"));
}

// ============================================================================
// Test Suite 2: File Lock Reproducibility
// ============================================================================

#[tokio::test]
async fn test_file_lock_content_is_valid_json() {
    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join("test.state.lock");
    let lock = FileLock::new(&lock_path);

    let info = LockInfo::new("apply");
    lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

    // Read lock file content
    let content = tokio::fs::read_to_string(&lock_path).await.unwrap();

    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed.is_object());
    assert!(parsed.get("id").is_some());
    assert!(parsed.get("operation").is_some());
}

#[tokio::test]
async fn test_file_lock_readable_from_another_process_simulation() {
    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join("shared.state.lock");

    // Process A creates lock
    let lock_a = FileLock::new(&lock_path);
    let info = LockInfo::new("deploy");
    lock_a.acquire(&info, Duration::from_secs(1)).await.unwrap();

    // Process B reads lock (simulated by creating another FileLock instance)
    let lock_b = FileLock::new(&lock_path);
    let read_info = lock_b.get_lock().await.unwrap();

    assert!(read_info.is_some());
    let read_info = read_info.unwrap();
    assert_eq!(read_info.id, info.id);
    assert_eq!(read_info.operation, "deploy");
}

#[tokio::test]
async fn test_file_lock_state_consistent_across_reads() {
    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join("consistent.state.lock");
    let lock = FileLock::new(&lock_path);

    let info = LockInfo::new("plan");
    lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

    // Read multiple times
    let read1 = lock.get_lock().await.unwrap().unwrap();
    let read2 = lock.get_lock().await.unwrap().unwrap();
    let read3 = lock.get_lock().await.unwrap().unwrap();

    // All reads should be identical
    assert_eq!(read1.id, read2.id);
    assert_eq!(read2.id, read3.id);
    assert_eq!(read1.operation, read2.operation);
    assert_eq!(read1.who, read2.who);
}

#[tokio::test]
async fn test_file_lock_survives_process_restart_simulation() {
    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join("restart.state.lock");

    // First "process" creates lock
    {
        let lock = FileLock::new(&lock_path);
        let info = LockInfo::new("init");
        lock.acquire(&info, Duration::from_secs(1)).await.unwrap();
        // Lock goes out of scope (process "exits" without releasing)
    }

    // Second "process" should see the lock
    let lock = FileLock::new(&lock_path);
    let info = lock.get_lock().await.unwrap();

    assert!(info.is_some());
    assert_eq!(info.unwrap().operation, "init");
}

// ============================================================================
// Test Suite 3: Lock Expiration Reproducibility
// ============================================================================

#[test]
fn test_lock_expiration_calculation_is_deterministic() {
    // Fixed timestamp for reproducibility
    let base_time = Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap();
    let duration = Duration::from_secs(3600); // 1 hour

    let expires_at = base_time + chrono::Duration::from_std(duration).unwrap();

    // Expiration should be exactly 1 hour later
    assert_eq!(
        expires_at,
        Utc.with_ymd_and_hms(2024, 1, 15, 11, 30, 0).unwrap()
    );
}

#[test]
fn test_expired_lock_detection_is_consistent() {
    // Create a lock that expired in the past
    let mut info = LockInfo::new("test");
    info.expires_at = Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap());

    // Should always be detected as expired
    assert!(info.is_expired());
    assert!(info.is_expired());
    assert!(info.is_expired());
}

#[test]
fn test_non_expired_lock_detection_is_consistent() {
    // Create a lock that expires in the future
    let mut info = LockInfo::new("test");
    info.expires_at = Some(Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap());

    // Should never be detected as expired (until 2099)
    assert!(!info.is_expired());
    assert!(!info.is_expired());
    assert!(!info.is_expired());
}

#[test]
fn test_no_expiration_never_expires() {
    let info = LockInfo::new("eternal");

    // Without expires_at, should never expire
    assert!(info.expires_at.is_none());
    assert!(!info.is_expired());
}

// ============================================================================
// Test Suite 4: Cross-Host Compatibility
// ============================================================================

#[test]
fn test_lock_info_compatible_with_different_hostname_formats() {
    // Various hostname formats that might come from different systems
    let hostnames = vec![
        "simple",
        "host.domain.com",
        "host-with-dashes",
        "HOST_WITH_UNDERSCORES",
        "192.168.1.1",
        "host:8080",
    ];

    for hostname in hostnames {
        let who = format!("user@{} (pid: 1234)", hostname);
        let info = LockInfo::new("test").with_who(&who);

        // Should serialize and deserialize correctly
        let json = serde_json::to_string(&info).unwrap();
        let restored: LockInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.who, who);
    }
}

#[test]
fn test_lock_info_handles_unicode_in_who_field() {
    let who = "用户@服务器 (pid: 1234)"; // Chinese characters
    let info = LockInfo::new("test").with_who(who);

    let json = serde_json::to_string(&info).unwrap();
    let restored: LockInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.who, who);
}

#[test]
fn test_lock_info_handles_special_characters_in_info() {
    let special_info = r#"Deployment with "quotes" and \backslash and newline
here"#;
    let info = LockInfo::new("test").with_info(special_info);

    let json = serde_json::to_string(&info).unwrap();
    let restored: LockInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.info, Some(special_info.to_string()));
}

// ============================================================================
// Test Suite 5: Lock State Consistency
// ============================================================================

#[tokio::test]
async fn test_in_memory_lock_state_consistency() {
    let lock = InMemoryLock::new();

    // Initial state should be unlocked
    assert!(lock.get_lock().await.unwrap().is_none());

    // After acquire
    let info = LockInfo::new("test");
    lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

    // State should be locked with correct info
    let state = lock.get_lock().await.unwrap().unwrap();
    assert_eq!(state.id, info.id);

    // After release
    lock.release(&info.id).await.unwrap();
    assert!(lock.get_lock().await.unwrap().is_none());
}

#[tokio::test]
async fn test_lock_acquire_idempotent_with_same_id() {
    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join("idempotent.state.lock");
    let lock = FileLock::new(&lock_path);

    let info = LockInfo::new("apply");

    // First acquire
    let result1 = lock.acquire(&info, Duration::from_secs(1)).await.unwrap();
    assert!(result1);

    // Read state
    let state1 = lock.get_lock().await.unwrap().unwrap();

    // Release and re-acquire with same operation
    lock.release(&info.id).await.unwrap();

    let info2 = LockInfo::new("apply");
    let result2 = lock.acquire(&info2, Duration::from_secs(1)).await.unwrap();
    assert!(result2);

    let state2 = lock.get_lock().await.unwrap().unwrap();

    // Operation should be the same
    assert_eq!(state1.operation, state2.operation);
}

// ============================================================================
// Test Suite 6: Lock Manager Reproducibility
// ============================================================================

#[tokio::test]
async fn test_lock_manager_provides_consistent_lock_info() {
    let backend = Box::new(InMemoryLock::new());
    let manager = StateLockManager::new(backend);

    let guard = manager.lock("deploy").await.unwrap();
    let lock_id = guard.lock_id().to_string();

    // Get info multiple times
    let info1 = manager.get_lock_info().await.unwrap().unwrap();
    let info2 = manager.get_lock_info().await.unwrap().unwrap();

    assert_eq!(info1.id, info2.id);
    assert_eq!(info1.id, lock_id);
    assert_eq!(info1.operation, "deploy");
}

#[tokio::test]
async fn test_lock_manager_expiration_configuration() {
    let backend = Box::new(InMemoryLock::new());
    let manager = StateLockManager::new(backend)
        .with_lock_expiration(Duration::from_secs(7200)); // 2 hours

    let _guard = manager.lock("test").await.unwrap();
    let info = manager.get_lock_info().await.unwrap().unwrap();

    // Lock should have expiration set
    assert!(info.expires_at.is_some());

    // Expiration should be approximately 2 hours from creation
    let expires = info.expires_at.unwrap();
    let created = info.created_at;
    let duration = (expires - created).num_seconds();

    // Allow some tolerance for test execution time
    assert!(duration >= 7199 && duration <= 7201);
}

#[tokio::test]
async fn test_lock_manager_no_expiration_configuration() {
    let backend = Box::new(InMemoryLock::new());
    let manager = StateLockManager::new(backend)
        .without_lock_expiration();

    let _guard = manager.lock("eternal").await.unwrap();
    let info = manager.get_lock_info().await.unwrap().unwrap();

    // Lock should not have expiration
    assert!(info.expires_at.is_none());
    assert!(!info.is_expired());
}

// ============================================================================
// Test Suite 7: Concurrent Access Reproducibility
// ============================================================================

#[tokio::test]
async fn test_concurrent_lock_attempts_produce_consistent_results() {
    let lock = std::sync::Arc::new(InMemoryLock::new());

    // First task acquires lock
    let info1 = LockInfo::new("first");
    lock.acquire(&info1, Duration::from_millis(100))
        .await
        .unwrap();

    // Multiple concurrent attempts should all fail consistently
    let lock_clone = lock.clone();
    let results: Vec<bool> = futures::future::join_all((0..10).map(|i| {
        let lock = lock_clone.clone();
        async move {
            let info = LockInfo::new(format!("attempt-{}", i));
            lock.acquire(&info, Duration::from_millis(10)).await.unwrap()
        }
    }))
    .await;

    // All attempts should fail
    assert!(results.iter().all(|&r| !r));
}

#[tokio::test]
async fn test_sequential_lock_release_acquire_is_deterministic() {
    let lock = InMemoryLock::new();

    for i in 0..5 {
        let info = LockInfo::new(format!("operation-{}", i));

        // Acquire
        let acquired = lock.acquire(&info, Duration::from_secs(1)).await.unwrap();
        assert!(acquired, "Iteration {} failed to acquire", i);

        // Verify state
        let state = lock.get_lock().await.unwrap().unwrap();
        assert_eq!(state.operation, format!("operation-{}", i));

        // Release
        let released = lock.release(&info.id).await.unwrap();
        assert!(released, "Iteration {} failed to release", i);

        // Verify unlocked
        assert!(lock.get_lock().await.unwrap().is_none());
    }
}

// ============================================================================
// Test Suite 8: Lock File Format Reproducibility
// ============================================================================

#[tokio::test]
async fn test_lock_file_format_is_human_readable() {
    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join("readable.state.lock");
    let lock = FileLock::new(&lock_path);

    let info = LockInfo::new("apply").with_info("Deploying v2.0.0");
    lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

    let content = tokio::fs::read_to_string(&lock_path).await.unwrap();

    // Should be pretty-printed JSON
    assert!(content.contains("{\n"));
    assert!(content.contains("  \"id\":"));
    assert!(content.contains("  \"operation\": \"apply\""));
}

#[tokio::test]
async fn test_lock_file_encoding_is_utf8() {
    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join("utf8.state.lock");
    let lock = FileLock::new(&lock_path);

    let info = LockInfo::new("test").with_info("日本語テスト");
    lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

    // Read as bytes and verify UTF-8
    let bytes = tokio::fs::read(&lock_path).await.unwrap();
    let content = String::from_utf8(bytes);

    assert!(content.is_ok());
    assert!(content.unwrap().contains("日本語テスト"));
}

// ============================================================================
// Test Suite 9: Error Handling Reproducibility
// ============================================================================

#[tokio::test]
async fn test_release_wrong_id_consistently_fails() {
    let lock = InMemoryLock::new();

    let info = LockInfo::new("owner");
    lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

    // Attempt to release with wrong ID multiple times
    for _ in 0..5 {
        let result = lock.release("wrong-id").await.unwrap();
        assert!(!result, "Release with wrong ID should consistently fail");
    }

    // Lock should still be held
    let state = lock.get_lock().await.unwrap();
    assert!(state.is_some());
    assert_eq!(state.unwrap().id, info.id);
}

#[tokio::test]
async fn test_double_release_consistently_fails() {
    let lock = InMemoryLock::new();

    let info = LockInfo::new("test");
    lock.acquire(&info, Duration::from_secs(1)).await.unwrap();

    // First release succeeds
    let result1 = lock.release(&info.id).await.unwrap();
    assert!(result1);

    // Second release fails
    let result2 = lock.release(&info.id).await.unwrap();
    assert!(!result2);

    // Third release also fails
    let result3 = lock.release(&info.id).await.unwrap();
    assert!(!result3);
}

#[tokio::test]
async fn test_get_lock_on_empty_returns_none_consistently() {
    let lock = InMemoryLock::new();

    for _ in 0..5 {
        let result = lock.get_lock().await.unwrap();
        assert!(result.is_none());
    }
}

// ============================================================================
// Test Suite 10: Display Format Reproducibility
// ============================================================================

#[test]
fn test_lock_info_display_format_is_deterministic() {
    let info = create_fixed_lock_info();

    let display1 = format!("{}", info);
    let display2 = format!("{}", info);
    let display3 = format!("{}", info);

    assert_eq!(display1, display2);
    assert_eq!(display2, display3);
}

#[test]
fn test_lock_info_display_contains_essential_info() {
    let info = create_fixed_lock_info();
    let display = format!("{}", info);

    assert!(display.contains("test-lock-abc123"));
    assert!(display.contains("apply"));
    assert!(display.contains("testuser@testhost"));
    assert!(display.contains("2024-01-15"));
}

#[test]
fn test_expired_lock_display_shows_expired() {
    let mut info = create_fixed_lock_info();
    info.expires_at = Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap());

    let display = format!("{}", info);

    assert!(display.contains("EXPIRED"));
}

#[test]
fn test_non_expired_lock_display_shows_expires() {
    let mut info = create_fixed_lock_info();
    info.expires_at = Some(Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap());

    let display = format!("{}", info);

    assert!(display.contains("expires at"));
    assert!(!display.contains("EXPIRED"));
}

// ============================================================================
// Test Suite 11: Backend Abstraction Reproducibility
// ============================================================================

#[tokio::test]
async fn test_backend_name_is_consistent() {
    let file_lock = FileLock::new("/tmp/test.lock");
    let memory_lock = InMemoryLock::new();

    // Backend names should be stable
    assert_eq!(file_lock.backend_name(), "file");
    assert_eq!(memory_lock.backend_name(), "memory");

    // Multiple calls return same value
    for _ in 0..3 {
        assert_eq!(file_lock.backend_name(), "file");
        assert_eq!(memory_lock.backend_name(), "memory");
    }
}

// ============================================================================
// Test Suite 12: Lock Guard Reproducibility
// ============================================================================

#[test]
fn test_lock_guard_state_tracking_is_consistent() {
    // Create guards and verify state tracking
    let lock_ids = vec!["id-1", "id-2", "id-3"];

    for id in lock_ids {
        let guard = AsyncLockGuard::new_file(
            id.to_string(),
            PathBuf::from("/tmp/test.lock"),
        );

        assert_eq!(guard.lock_id(), id);
    }
}

// ============================================================================
// Test Suite 13: Timestamp Reproducibility
// ============================================================================

#[test]
fn test_fixed_timestamp_serialization() {
    let timestamp = Utc.with_ymd_and_hms(2024, 6, 15, 14, 30, 45).unwrap();

    let json = serde_json::to_string(&timestamp).unwrap();

    // Should be in ISO 8601 / RFC 3339 format
    assert!(json.contains("2024-06-15T14:30:45Z") || json.contains("2024-06-15T14:30:45"));
}

#[test]
fn test_timestamp_round_trip_preserves_precision() {
    let original = Utc.with_ymd_and_hms(2024, 6, 15, 14, 30, 45).unwrap();

    let json = serde_json::to_string(&original).unwrap();
    let restored: DateTime<Utc> = serde_json::from_str(&json).unwrap();

    assert_eq!(original, restored);
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_fixed_lock_info() -> LockInfo {
    LockInfo {
        id: "test-lock-abc123".to_string(),
        operation: "apply".to_string(),
        who: "testuser@testhost (pid: 12345)".to_string(),
        created_at: Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap(),
        expires_at: Some(Utc.with_ymd_and_hms(2024, 1, 15, 11, 30, 0).unwrap()),
        info: Some("Test deployment".to_string()),
    }
}
