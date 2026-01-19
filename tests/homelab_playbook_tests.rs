//! Homelab playbook integration tests.
//!
//! These tests run a small smoke playbook against real homelab hosts.
//!
//! To run:
//!   export RUSTIBLE_HOMELAB_TESTS=1
//!   cargo test --test homelab_playbook_tests -- --ignored
//!
//! Optional env:
//!   RUSTIBLE_HOMELAB_INVENTORY=path/to/inventory.yml

use std::env;
use std::path::PathBuf;

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::Task;
use rustible::executor::{Executor, ExecutorConfig};
use rustible::inventory::Inventory;

fn homelab_enabled() -> bool {
    env::var("RUSTIBLE_HOMELAB_TESTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn homelab_inventory_path() -> PathBuf {
    env::var("RUSTIBLE_HOMELAB_INVENTORY")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("tests/fixtures/homelab_inventory.yml"))
}

#[tokio::test]
#[ignore = "Requires homelab hosts and SSH access"]
async fn test_homelab_smoke_playbook() {
    if !homelab_enabled() {
        eprintln!("Skipping homelab playbook test (RUSTIBLE_HOMELAB_TESTS not set)");
        return;
    }

    let inventory_path = homelab_inventory_path();
    if !inventory_path.exists() {
        eprintln!(
            "Skipping homelab playbook test (inventory not found at {})",
            inventory_path.display()
        );
        return;
    }

    let inventory = Inventory::load(&inventory_path).expect("Failed to load homelab inventory");
    let runtime = RuntimeContext::from_inventory(&inventory);
    let config = ExecutorConfig {
        gather_facts: false,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);

    let tmp_dir = "/tmp/rustible-homelab-test";
    let tmp_file = format!("{}/hello.txt", tmp_dir);

    let mut playbook = Playbook::new("Homelab Smoke Playbook");
    let mut play = Play::new("Homelab Smoke", "all");
    play.gather_facts = false;
    play.add_task(Task::new("Check kernel", "command").arg("cmd", "uname -s"));
    play.add_task(
        Task::new("Create temp dir", "file")
            .arg("path", tmp_dir)
            .arg("state", "directory"),
    );
    play.add_task(
        Task::new("Write temp file", "copy")
            .arg("dest", tmp_file.as_str())
            .arg("content", "rustible homelab smoke"),
    );
    play.add_task(
        Task::new("Read temp file", "command").arg("cmd", format!("cat {}", tmp_file)),
    );
    play.add_task(
        Task::new("Cleanup temp file", "file")
            .arg("path", tmp_file.as_str())
            .arg("state", "absent"),
    );
    play.add_task(
        Task::new("Cleanup temp dir", "file")
            .arg("path", tmp_dir)
            .arg("state", "absent"),
    );
    playbook.add_play(play);

    let results = executor
        .run_playbook(&playbook)
        .await
        .expect("Homelab playbook failed to execute");

    for (host, result) in results {
        assert!(!result.unreachable, "Host {} unreachable", host);
        assert!(!result.failed, "Host {} failed", host);
        assert!(result.stats.ok + result.stats.changed > 0, "Host {} had no task results", host);
    }
}
