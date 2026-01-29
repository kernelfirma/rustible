//! Executor Single Runtime Tests
//!
//! Issue #288: Ensure the executor is the single runtime and recap stats
//! are consistent across strategies.

use serde_json::json;

use rustible::executor::playbook::{Play, Playbook};
use rustible::executor::runtime::RuntimeContext;
use rustible::executor::task::Task;
use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig};

fn build_runtime(hosts: &[&str]) -> RuntimeContext {
    let mut runtime = RuntimeContext::new();
    for host in hosts {
        runtime.add_host((*host).to_string(), None);
        runtime.set_host_var(host, "ansible_connection".to_string(), json!("local"));
    }
    runtime
}

fn build_playbook() -> Playbook {
    let mut playbook = Playbook::new("Executor Recap Stats");
    let mut play = Play::new("Recap Play", "all");
    play.gather_facts = false;

    let ok_task = Task::new("Ok task", "debug").arg("msg", "ok");

    let mut changed_task = Task::new("Changed task", "debug").arg("msg", "changed");
    changed_task.changed_when = Some("true".to_string());

    play.add_task(ok_task);
    play.add_task(changed_task);
    playbook.add_play(play);
    playbook
}

async fn run_with_strategy(strategy: ExecutionStrategy, hosts: &[&str]) -> rustible::executor::ExecutionStats {
    let runtime = build_runtime(hosts);
    let config = ExecutorConfig {
        gather_facts: false,
        strategy,
        ..Default::default()
    };
    let executor = Executor::with_runtime(config, runtime);
    let playbook = build_playbook();
    let results = executor.run_playbook(&playbook).await.unwrap();
    Executor::summarize_results(&results)
}

#[tokio::test]
async fn test_changed_only_increments_changed() {
    let stats = run_with_strategy(ExecutionStrategy::Linear, &["host1"]).await;

    assert_eq!(stats.ok, 1);
    assert_eq!(stats.changed, 1);
    assert_eq!(stats.failed, 0);
    assert_eq!(stats.skipped, 0);
    assert_eq!(stats.unreachable, 0);
}

#[tokio::test]
async fn test_recap_stats_consistent_across_strategies() {
    let hosts = ["host1", "host2"];

    let linear = run_with_strategy(ExecutionStrategy::Linear, &hosts).await;
    let free = run_with_strategy(ExecutionStrategy::Free, &hosts).await;
    let pinned = run_with_strategy(ExecutionStrategy::HostPinned, &hosts).await;

    assert_eq!(linear.ok, 2);
    assert_eq!(linear.changed, 2);
    assert_eq!(linear.failed, 0);
    assert_eq!(linear.skipped, 0);
    assert_eq!(linear.unreachable, 0);

    assert_eq!(free.ok, linear.ok);
    assert_eq!(free.changed, linear.changed);
    assert_eq!(free.failed, linear.failed);
    assert_eq!(free.skipped, linear.skipped);
    assert_eq!(free.unreachable, linear.unreachable);

    assert_eq!(pinned.ok, linear.ok);
    assert_eq!(pinned.changed, linear.changed);
    assert_eq!(pinned.failed, linear.failed);
    assert_eq!(pinned.skipped, linear.skipped);
    assert_eq!(pinned.unreachable, linear.unreachable);
}
