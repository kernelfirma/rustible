//! Native Remote Facts Parity Tests
//!
//! Issue #294: Native remote facts parity with Ansible
//!
//! These tests exercise the production remote fact gathering path and
//! validate that gathered facts are exposed under ansible_* aliases
//! in the runtime variable context.

#![cfg(target_os = "linux")]

use indexmap::IndexMap;
use rustible::connection::local::LocalConnection;
use rustible::connection::Connection;
use rustible::executor::runtime::RuntimeContext;
use rustible::modules::facts::gather_facts_via_connection;
use serde_json::Value as JsonValue;
use std::sync::Arc;

fn to_index_map(facts: std::collections::HashMap<String, JsonValue>) -> IndexMap<String, JsonValue> {
    facts.into_iter().collect()
}

#[tokio::test]
async fn test_native_remote_facts_gathered() {
    let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
    let facts = gather_facts_via_connection(&conn, None).await;

    assert!(!facts.is_empty(), "Expected facts from remote gatherer");

    // Ensure core OS facts exist (may vary slightly by environment).
    assert!(
        facts.contains_key("hostname")
            || facts.contains_key("system")
            || facts.contains_key("kernel"),
        "Expected OS facts, got: {:?}",
        facts.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn test_native_remote_facts_expose_ansible_prefixed_vars() {
    let conn: Arc<dyn Connection + Send + Sync> = Arc::new(LocalConnection::new());
    let facts = gather_facts_via_connection(&conn, None).await;
    let facts = to_index_map(facts);

    let mut ctx = RuntimeContext::new();
    ctx.add_host("localhost".to_string(), None);
    ctx.set_host_facts("localhost", facts.clone());

    let merged = ctx.get_merged_vars("localhost");

    let candidate_keys = [
        "hostname",
        "kernel",
        "architecture",
        "distribution",
        "os_family",
        "user_id",
    ];

    let mut checked = 0;
    for key in candidate_keys {
        if let Some(value) = facts.get(key) {
            checked += 1;
            let ansible_key = format!("ansible_{}", key);
            assert_eq!(merged.get(key), Some(value));
            assert_eq!(merged.get(ansible_key.as_str()), Some(value));
        }
    }

    assert!(checked > 0, "Expected at least one candidate fact to validate");
}
