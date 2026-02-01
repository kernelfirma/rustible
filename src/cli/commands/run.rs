//! Run command - Execute a playbook
//!
//! This module implements the `run` subcommand for executing Ansible-like playbooks.

use super::{CommandContext, Runnable};
use crate::cli::output::{RecapStats, TaskStatus};
use anyhow::Result;
use clap::Parser;
use indexmap::IndexMap;
use regex::Regex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// Arguments for the run command
#[derive(Parser, Debug, Clone)]
pub struct RunArgs {
    /// Path to the playbook file
    #[arg(required = true)]
    pub playbook: PathBuf,

    /// Tags to run (only tasks with these tags)
    #[arg(long, short = 't', action = clap::ArgAction::Append)]
    pub tags: Vec<String>,

    /// Tags to skip (skip tasks with these tags)
    #[arg(long, action = clap::ArgAction::Append)]
    pub skip_tags: Vec<String>,

    /// Start at a specific task
    #[arg(long)]
    pub start_at_task: Option<String>,

    /// Step through tasks one at a time
    #[arg(long)]
    pub step: bool,

    /// Ask for vault password
    #[arg(long)]
    pub ask_vault_pass: bool,

    /// Vault password file
    #[arg(long)]
    pub vault_password_file: Option<PathBuf>,

    /// Become (sudo/su)
    #[arg(short = 'b', long)]
    pub r#become: bool,

    /// Become method (sudo, su, etc.)
    #[arg(long, default_value = "sudo")]
    pub become_method: String,

    /// Become user
    #[arg(long, default_value = "root")]
    pub become_user: String,

    /// Ask for become password
    #[arg(short = 'K', long)]
    pub ask_become_pass: bool,

    /// Remote user
    #[arg(short = 'u', long)]
    pub user: Option<String>,

    /// Private key file
    #[arg(long)]
    pub private_key: Option<PathBuf>,

    /// SSH common args
    #[arg(long)]
    pub ssh_common_args: Option<String>,

    /// Plan mode - show what would be executed without running
    #[arg(long)]
    pub plan: bool,
}

impl RunArgs {
    /// Execute the run command using the executor as the single runtime
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        use rustible::executor::{Executor, ExecutorConfig, ExecutionStrategy, Playbook};
        use rustible::executor::runtime::RuntimeContext;
        use rustible::inventory::Inventory;

        let start_time = Instant::now();

        // Initialize progress bars
        ctx.output.init_progress();

        // Validate playbook exists
        if !self.playbook.exists() {
            ctx.output.error(&format!(
                "Playbook file not found: {}",
                self.playbook.display()
            ));
            return Ok(1);
        }

        // Display banner
        ctx.output.banner(&format!(
            "PLAYBOOK: {}",
            self.playbook
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));

        // Load playbook using executor's Playbook parser
        ctx.output.info("Loading playbook...");
        let playbook = match Playbook::load(&self.playbook) {
            Ok(pb) => pb,
            Err(e) => {
                ctx.output.error(&format!("Failed to parse playbook: {}", e));
                return Ok(1);
            }
        };

        // Get inventory path and load inventory
        let inventory_path = ctx.inventory().cloned();
        let runtime = if let Some(inv_path) = &inventory_path {
            if inv_path.exists() {
                match Inventory::load(inv_path) {
                    Ok(inventory) => {
                        ctx.output.debug(&format!("Loaded inventory from: {}", inv_path.display()));
                        RuntimeContext::from_inventory(&inventory)
                    }
                    Err(e) => {
                        ctx.output.warning(&format!("Failed to load inventory: {}", e));
                        RuntimeContext::new()
                    }
                }
            } else {
                ctx.output.warning(&format!("Inventory file not found: {}", inv_path.display()));
                RuntimeContext::new()
            }
        } else {
            ctx.output.warning("No inventory specified, using localhost");
            ctx.output.hint("Use -i <inventory_file> to specify an inventory");
            // Create runtime with localhost only
            let mut runtime = RuntimeContext::new();
            runtime.add_host("localhost".to_string(), Some("all"));
            runtime
        };

        // Validate limit pattern if specified
        if let Some(ref limit) = ctx.limit {
            if let Err(e) = Self::validate_limit_pattern(limit) {
                ctx.output.error(&e);
                return Ok(1);
            }
        }

        // Parse extra vars and convert to serde_json::Value
        let extra_vars_yaml = ctx.parse_extra_vars()?;
        let mut extra_vars = std::collections::HashMap::new();
        for (k, v) in extra_vars_yaml {
            if let Ok(json_value) = serde_json::to_value(&v) {
                extra_vars.insert(k, json_value);
            }
        }
        ctx.output.debug(&format!("Extra vars: {:?}", extra_vars));

        // Plan mode - use legacy show_plan for now as per issue #48:
        // "Plan mode is implemented on top of executor or clearly separated as non-executing"
        if self.plan {
            ctx.output.plan("WARNING: Running in PLAN MODE - showing execution plan only");
            let playbook_content = std::fs::read_to_string(&self.playbook)?;
            let playbook_yaml: serde_yaml::Value = serde_yaml::from_str(&playbook_content)?;
            if let Some(plays) = playbook_yaml.as_sequence() {
                let extra_vars_for_plan: std::collections::HashMap<String, serde_yaml::Value> =
                    ctx.parse_extra_vars()?;
                self.show_plan(ctx, plays, &extra_vars_for_plan).await?;
            }
            let duration = start_time.elapsed();
            ctx.output.info(&format!("Plan completed in {:.2}s", duration.as_secs_f64()));
            return Ok(0);
        }

        // Check mode notice
        if ctx.check_mode {
            ctx.output.warning("Running in CHECK MODE - no changes will be made");
        }

        // Build executor configuration from CLI args
        let executor_config = ExecutorConfig {
            forks: ctx.forks,
            check_mode: ctx.check_mode,
            diff_mode: ctx.diff_mode,
            verbosity: ctx.verbosity,
            strategy: ExecutionStrategy::Linear,
            task_timeout: 300,
            gather_facts: true,
            extra_vars,
        };

        // Create executor with runtime context
        let executor = Executor::with_runtime(executor_config, runtime);

        // Run playbook using executor
        ctx.output.info(&format!("Running playbook: {}", playbook.name));
        let results = match executor.run_playbook(&playbook).await {
            Ok(results) => results,
            Err(e) => {
                ctx.output.error(&format!("Playbook execution failed: {}", e));
                return Ok(2);
            }
        };

        // Close all pooled connections
        ctx.close_connections().await;

        // Convert executor results to RecapStats
        use crate::cli::output::HostStats;
        let mut stats = RecapStats::new();
        let mut has_failures = false;

        for (host, result) in &results {
            let host_stats = HostStats {
                ok: result.stats.ok as u32,
                changed: result.stats.changed as u32,
                skipped: result.stats.skipped as u32,
                failed: result.stats.failed as u32,
                unreachable: if result.unreachable { 1 } else { 0 },
                rescued: 0,
                ignored: 0,
            };
            stats.hosts.insert(host.clone(), host_stats);

            if result.failed || result.unreachable {
                has_failures = true;
            }
        }

        // Print recap
        ctx.output.recap(&stats);

        // Print timing
        let duration = start_time.elapsed();
        ctx.output.info(&format!(
            "Playbook finished in {:.2}s",
            duration.as_secs_f64()
        ));

        // Return exit code
        if has_failures {
            Ok(2)
        } else {
            Ok(0)
        }
    }

    /// Show execution plan for the playbook
    async fn show_plan(
        &self,
        ctx: &mut CommandContext,
        plays: &[serde_yaml::Value],
        extra_vars: &std::collections::HashMap<String, serde_yaml::Value>,
    ) -> Result<()> {
        ctx.output.section("EXECUTION PLAN");
        ctx.output
            .plan("Rustible will perform the following actions:\n");

        for (play_idx, play) in plays.iter().enumerate() {
            let play_name = play
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("Unnamed play");

            let hosts_pattern = play
                .get("hosts")
                .and_then(|h| h.as_str())
                .unwrap_or("localhost");

            // Resolve hosts
            let hosts = self.resolve_hosts(ctx, hosts_pattern)?;

            if hosts.is_empty() {
                continue;
            }

            // Print play header similar to terraform plan
            ctx.output.plan(&format!(
                "{}[Play {}/{}] {} {}",
                if play_idx > 0 { "\n" } else { "" },
                play_idx + 1,
                plays.len(),
                "*".to_string(),
                play_name
            ));
            ctx.output.plan(&format!(
                "  Hosts: {} ({} host{})",
                hosts_pattern,
                hosts.len(),
                if hosts.len() == 1 { "" } else { "s" }
            ));

            // Collect play variables
            let mut vars: IndexMap<String, serde_yaml::Value> = IndexMap::new();
            for (k, v) in extra_vars {
                vars.insert(k.clone(), v.clone());
            }
            if let Some(play_vars) = play.get("vars") {
                if let Some(mapping) = play_vars.as_mapping() {
                    for (k, v) in mapping {
                        if let Some(key) = k.as_str() {
                            vars.insert(key.to_string(), v.clone());
                        }
                    }
                }
            }

            // Get pre_tasks, tasks, post_tasks, roles
            let pre_tasks = play
                .get("pre_tasks")
                .and_then(|t| t.as_sequence())
                .cloned()
                .unwrap_or_default();

            let tasks = play
                .get("tasks")
                .and_then(|t| t.as_sequence())
                .cloned()
                .unwrap_or_default();

            let post_tasks = play
                .get("post_tasks")
                .and_then(|t| t.as_sequence())
                .cloned()
                .unwrap_or_default();

            let roles = play
                .get("roles")
                .and_then(|r| r.as_sequence())
                .cloned()
                .unwrap_or_default();

            // Count role tasks
            let playbook_dir = self.playbook.parent().unwrap_or(std::path::Path::new("."));
            let mut role_task_count = 0;
            for role in &roles {
                let role_name = if let Some(name) = role.as_str() {
                    name.to_string()
                } else if let Some(name) = role.get("role").and_then(|r| r.as_str()) {
                    name.to_string()
                } else if let Some(name) = role.get("name").and_then(|r| r.as_str()) {
                    name.to_string()
                } else {
                    continue;
                };
                let role_tasks_path = playbook_dir
                    .join("roles")
                    .join(&role_name)
                    .join("tasks")
                    .join("main.yml");
                if role_tasks_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&role_tasks_path) {
                        if let Ok(role_tasks) =
                            serde_yaml::from_str::<Vec<serde_yaml::Value>>(&content)
                        {
                            role_task_count += role_tasks.len();
                        }
                    }
                }
            }

            let total_play_tasks =
                pre_tasks.len() + role_task_count + tasks.len() + post_tasks.len();

            if total_play_tasks == 0 {
                ctx.output.plan("  No tasks to execute");
                continue;
            }

            ctx.output.plan(&format!(
                "  Tasks: {} task{}",
                total_play_tasks,
                if total_play_tasks == 1 { "" } else { "s" }
            ));

            let mut task_num = 0;

            // Helper closure to show a task
            let show_task = |ctx: &mut CommandContext,
                             task: &serde_yaml::Value,
                             task_num: usize,
                             total: usize,
                             hosts: &[String],
                             vars: &IndexMap<String, serde_yaml::Value>,
                             me: &Self| {
                let task_name = task
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("Unnamed task");

                if !me.should_run_task(task) {
                    return;
                }

                let (module, args) = me.detect_module(task);

                ctx.output.plan(&format!(
                    "\n  {} Task {}/{}: {}",
                    ">".to_string(),
                    task_num,
                    total,
                    task_name
                ));
                ctx.output.plan(&format!("    Module: {}", module));

                for host in hosts {
                    let action_desc = me.get_action_description(module, args, vars);
                    ctx.output
                        .plan(&format!("      [{}] {}", host, action_desc));
                }

                if let Some(when) = task.get("when") {
                    let condition = when.as_str().unwrap_or("<complex condition>");
                    ctx.output.plan(&format!("    When: {}", condition));
                }

                if let Some(notify) = task.get("notify") {
                    let handlers = if let Some(s) = notify.as_str() {
                        vec![s]
                    } else if let Some(seq) = notify.as_sequence() {
                        seq.iter().filter_map(|v| v.as_str()).collect()
                    } else {
                        vec![]
                    };
                    if !handlers.is_empty() {
                        ctx.output
                            .plan(&format!("    Notify: {}", handlers.join(", ")));
                    }
                }
            };

            // Show pre_tasks
            for task in &pre_tasks {
                task_num += 1;
                show_task(ctx, task, task_num, total_play_tasks, &hosts, &vars, self);
            }

            // Show role tasks
            for role in &roles {
                let role_name = if let Some(name) = role.as_str() {
                    name.to_string()
                } else if let Some(name) = role.get("role").and_then(|r| r.as_str()) {
                    name.to_string()
                } else if let Some(name) = role.get("name").and_then(|r| r.as_str()) {
                    name.to_string()
                } else {
                    continue;
                };

                let role_tasks_path = playbook_dir
                    .join("roles")
                    .join(&role_name)
                    .join("tasks")
                    .join("main.yml");
                if role_tasks_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&role_tasks_path) {
                        if let Ok(role_tasks) =
                            serde_yaml::from_str::<Vec<serde_yaml::Value>>(&content)
                        {
                            for task in &role_tasks {
                                task_num += 1;
                                show_task(
                                    ctx,
                                    task,
                                    task_num,
                                    total_play_tasks,
                                    &hosts,
                                    &vars,
                                    self,
                                );
                            }
                        }
                    }
                }
            }

            // Show tasks
            for task in &tasks {
                task_num += 1;
                show_task(ctx, task, task_num, total_play_tasks, &hosts, &vars, self);
            }

            // Show post_tasks
            for task in &post_tasks {
                task_num += 1;
                show_task(ctx, task, task_num, total_play_tasks, &hosts, &vars, self);
            }
        }

        ctx.output.section("\nPLAN SUMMARY");

        // Count total tasks and hosts
        let mut total_tasks = 0;
        let mut total_hosts = std::collections::HashSet::new();

        for play in plays {
            let hosts_pattern = play
                .get("hosts")
                .and_then(|h| h.as_str())
                .unwrap_or("localhost");

            if let Ok(hosts) = self.resolve_hosts(ctx, hosts_pattern) {
                for host in hosts {
                    total_hosts.insert(host);
                }
            }

            // Count all tasks: pre_tasks + role tasks + tasks + post_tasks
            let playbook_dir = self.playbook.parent().unwrap_or(std::path::Path::new("."));

            if let Some(pre_tasks) = play.get("pre_tasks").and_then(|t| t.as_sequence()) {
                for task in pre_tasks {
                    if self.should_run_task(task) {
                        total_tasks += 1;
                    }
                }
            }

            if let Some(roles) = play.get("roles").and_then(|r| r.as_sequence()) {
                for role in roles {
                    let role_name = if let Some(name) = role.as_str() {
                        name.to_string()
                    } else if let Some(name) = role.get("role").and_then(|r| r.as_str()) {
                        name.to_string()
                    } else if let Some(name) = role.get("name").and_then(|r| r.as_str()) {
                        name.to_string()
                    } else {
                        continue;
                    };
                    let role_tasks_path = playbook_dir
                        .join("roles")
                        .join(&role_name)
                        .join("tasks")
                        .join("main.yml");
                    if role_tasks_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&role_tasks_path) {
                            if let Ok(role_tasks) =
                                serde_yaml::from_str::<Vec<serde_yaml::Value>>(&content)
                            {
                                for task in &role_tasks {
                                    if self.should_run_task(task) {
                                        total_tasks += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                for task in tasks {
                    if self.should_run_task(task) {
                        total_tasks += 1;
                    }
                }
            }

            if let Some(post_tasks) = play.get("post_tasks").and_then(|t| t.as_sequence()) {
                for task in post_tasks {
                    if self.should_run_task(task) {
                        total_tasks += 1;
                    }
                }
            }
        }

        ctx.output.plan(&format!(
            "Plan: {} task{} across {} host{}",
            total_tasks,
            if total_tasks == 1 { "" } else { "s" },
            total_hosts.len(),
            if total_hosts.len() == 1 { "" } else { "s" }
        ));

        ctx.output
            .plan("\nTo execute this plan, run the same command without --plan");

        Ok(())
    }

    /// Get a human-readable description of what an action will do
    fn get_action_description(
        &self,
        module: &str,
        args: Option<&serde_yaml::Value>,
        vars: &IndexMap<String, serde_yaml::Value>,
    ) -> String {
        match module {
            "command" | "shell" => {
                let cmd = args
                    .and_then(|a| {
                        a.as_str().map(|s| s.to_string()).or_else(|| {
                            a.get("cmd").and_then(|c| c.as_str()).map(|s| s.to_string())
                        })
                    })
                    .unwrap_or_else(|| "<command>".to_string());
                let templated = Self::template_string(&cmd, vars);
                format!("will execute: {}", templated)
            }
            "package" | "apt" | "yum" | "dnf" | "pip" => {
                let name = args
                    .and_then(|a| a.get("name"))
                    .and_then(|n| {
                        n.as_str().map(|s| s.to_string()).or_else(|| {
                            n.as_sequence().map(|seq| {
                                seq.iter()
                                    .filter_map(|v| v.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            })
                        })
                    })
                    .unwrap_or_else(|| "<package>".to_string());
                let templated_name = Self::template_string(&name, vars);
                let state = args
                    .and_then(|a| a.get("state"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("present");
                format!(
                    "will {} package: {}",
                    if state == "absent" {
                        "remove"
                    } else {
                        "install"
                    },
                    templated_name
                )
            }
            "service" => {
                let name = args
                    .and_then(|a| a.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("<service>");
                let templated_name = Self::template_string(name, vars);
                let state = args
                    .and_then(|a| a.get("state"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("started");
                let templated_state = Self::template_string(state, vars);
                format!(
                    "will ensure service {} is {}",
                    templated_name, templated_state
                )
            }
            "copy" => {
                let src = args
                    .and_then(|a| a.get("src"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("<src>");
                let dest = args
                    .and_then(|a| a.get("dest"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("<dest>");
                format!("will copy {} to {}", src, dest)
            }
            "file" => {
                let path = args
                    .and_then(|a| a.get("path"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("<path>");
                let state = args
                    .and_then(|a| a.get("state"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("file");
                format!("will ensure {} exists as {}", path, state)
            }
            "template" => {
                let src = args
                    .and_then(|a| a.get("src"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("<template>");
                let dest = args
                    .and_then(|a| a.get("dest"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("<dest>");
                format!("will render template {} to {}", src, dest)
            }
            "user" => {
                let name = args
                    .and_then(|a| a.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("<user>");
                let state = args
                    .and_then(|a| a.get("state"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("present");
                format!(
                    "will {} user: {}",
                    if state == "absent" {
                        "remove"
                    } else {
                        "create/update"
                    },
                    name
                )
            }
            "group" => {
                let name = args
                    .and_then(|a| a.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("<group>");
                let state = args
                    .and_then(|a| a.get("state"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("present");
                format!(
                    "will {} group: {}",
                    if state == "absent" {
                        "remove"
                    } else {
                        "create/update"
                    },
                    name
                )
            }
            "git" => {
                let repo = args
                    .and_then(|a| a.get("repo"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("<repo>");
                let dest = args
                    .and_then(|a| a.get("dest"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("<dest>");
                format!("will clone/update {} to {}", repo, dest)
            }
            "debug" => {
                if let Some(msg) = args.and_then(|a| a.get("msg")).and_then(|m| m.as_str()) {
                    let templated = Self::template_string(msg, vars);
                    format!("will display: {}", templated)
                } else if let Some(var) = args.and_then(|a| a.get("var")).and_then(|v| v.as_str()) {
                    format!("will display variable: {}", var)
                } else {
                    "will display debug information".to_string()
                }
            }
            "set_fact" => "will set facts".to_string(),
            "lineinfile" => {
                let path = args
                    .and_then(|a| a.get("path"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("<file>");
                format!("will modify line in {}", path)
            }
            "blockinfile" => {
                let path = args
                    .and_then(|a| a.get("path"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("<file>");
                format!("will insert/update block in {}", path)
            }
            _ => format!("will execute {} module", module),
        }
    }

    /// Execute a single play
    async fn execute_play(
        &self,
        ctx: &mut CommandContext,
        play: &serde_yaml::Value,
        stats: &Arc<Mutex<RecapStats>>,
    ) -> Result<()> {
        // Get play name
        let play_name = play
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unnamed play");

        ctx.output.play_header(play_name);

        // Get hosts pattern
        let hosts_pattern = play
            .get("hosts")
            .and_then(|h| h.as_str())
            .unwrap_or("localhost");

        ctx.output.info(&format!("Target hosts: {}", hosts_pattern));

        // Get hosts from inventory (simplified for now)
        let hosts = self.resolve_hosts(ctx, hosts_pattern)?;

        if hosts.is_empty() {
            ctx.output
                .warning(&format!("No hosts matched pattern: {}", hosts_pattern));
            return Ok(());
        }

        // Extract play-level variables
        let mut vars: IndexMap<String, serde_yaml::Value> = IndexMap::new();

        // Add extra vars first (lowest precedence in this context)
        if let Ok(extra_vars) = ctx.parse_extra_vars() {
            for (k, v) in extra_vars {
                if let Ok(yaml_val) = serde_yaml::to_value(&v) {
                    vars.insert(k, yaml_val);
                }
            }
        }

        // Add play vars (higher precedence)
        if let Some(play_vars) = play.get("vars") {
            if let Some(mapping) = play_vars.as_mapping() {
                for (k, v) in mapping {
                    if let Some(key) = k.as_str() {
                        vars.insert(key.to_string(), v.clone());
                    }
                }
            }
        }

        // Get pre_tasks, tasks, post_tasks
        let pre_tasks = play
            .get("pre_tasks")
            .and_then(|t| t.as_sequence())
            .cloned()
            .unwrap_or_default();

        let tasks = play
            .get("tasks")
            .and_then(|t| t.as_sequence())
            .cloned()
            .unwrap_or_default();

        let post_tasks = play
            .get("post_tasks")
            .and_then(|t| t.as_sequence())
            .cloned()
            .unwrap_or_default();

        // Get roles
        let roles = play
            .get("roles")
            .and_then(|r| r.as_sequence())
            .cloned()
            .unwrap_or_default();

        // Check if gather_facts is enabled (defaults to true)
        let gather_facts = play
            .get("gather_facts")
            .and_then(|g| g.as_bool())
            .unwrap_or(true);

        // Ansible execution order: gather_facts -> pre_tasks -> roles -> tasks -> post_tasks

        // 0. Gather facts if enabled
        if gather_facts {
            ctx.output.task_header("Gathering Facts");

            // Execute the facts module using rustible's native implementation
            use rustible::modules::{facts::FactsModule, Module, ModuleContext};
            let facts_module = FactsModule;
            let params = std::collections::HashMap::new();
            let module_ctx = ModuleContext::default().with_verbosity(ctx.verbosity);

            match facts_module.execute(&params, &module_ctx) {
                Ok(output) => {
                    // Extract ansible_facts and add them to vars with ansible_ prefix
                    if let Some(facts) = output.data.get("ansible_facts") {
                        if let Some(facts_obj) = facts.as_object() {
                            for (key, value) in facts_obj {
                                // Convert JSON value to YAML value
                                if let Ok(yaml_val) = serde_yaml::to_value(value) {
                                    vars.insert(format!("ansible_{}", key), yaml_val);
                                }
                            }
                        }
                    }
                    for host in &hosts {
                        ctx.output.task_result(host, TaskStatus::Ok, None);
                        stats.lock().await.record(host, TaskStatus::Ok);
                    }
                }
                Err(e) => {
                    for host in &hosts {
                        ctx.output
                            .task_result(host, TaskStatus::Failed, Some(&e.to_string()));
                        stats.lock().await.record(host, TaskStatus::Failed);
                    }
                    return Err(anyhow::anyhow!("Failed to gather facts: {}", e));
                }
            }
        }

        // 1. Execute pre_tasks
        for task in &pre_tasks {
            self.execute_task(ctx, task, &hosts, stats, &vars).await?;
        }

        // 2. Execute role tasks
        for role in &roles {
            let role_name = if let Some(name) = role.as_str() {
                name.to_string()
            } else if let Some(name) = role.get("role").and_then(|r| r.as_str()) {
                name.to_string()
            } else if let Some(name) = role.get("name").and_then(|r| r.as_str()) {
                name.to_string()
            } else {
                continue;
            };

            // Load role tasks from roles/<role_name>/tasks/main.yml
            let playbook_dir = self.playbook.parent().unwrap_or(std::path::Path::new("."));
            let role_tasks_path = playbook_dir
                .join("roles")
                .join(&role_name)
                .join("tasks")
                .join("main.yml");

            if role_tasks_path.exists() {
                if let Ok(role_content) = std::fs::read_to_string(&role_tasks_path) {
                    if let Ok(role_tasks) =
                        serde_yaml::from_str::<Vec<serde_yaml::Value>>(&role_content)
                    {
                        // Merge role vars if present
                        let mut role_vars = vars.clone();

                        // Load role defaults
                        let defaults_path = playbook_dir
                            .join("roles")
                            .join(&role_name)
                            .join("defaults")
                            .join("main.yml");
                        if defaults_path.exists() {
                            if let Ok(defaults_content) = std::fs::read_to_string(&defaults_path) {
                                if let Ok(defaults) =
                                    serde_yaml::from_str::<serde_yaml::Value>(&defaults_content)
                                {
                                    if let Some(mapping) = defaults.as_mapping() {
                                        for (k, v) in mapping {
                                            if let Some(key) = k.as_str() {
                                                role_vars
                                                    .entry(key.to_string())
                                                    .or_insert(v.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Load role vars (higher precedence than defaults)
                        let vars_path = playbook_dir
                            .join("roles")
                            .join(&role_name)
                            .join("vars")
                            .join("main.yml");
                        if vars_path.exists() {
                            if let Ok(vars_content) = std::fs::read_to_string(&vars_path) {
                                if let Ok(role_vars_file) =
                                    serde_yaml::from_str::<serde_yaml::Value>(&vars_content)
                                {
                                    if let Some(mapping) = role_vars_file.as_mapping() {
                                        for (k, v) in mapping {
                                            if let Some(key) = k.as_str() {
                                                role_vars.insert(key.to_string(), v.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // Execute role tasks
                        for task in &role_tasks {
                            self.execute_task(ctx, task, &hosts, stats, &role_vars)
                                .await?;
                        }
                    }
                }
            } else {
                ctx.output.warning(&format!(
                    "Role '{}' not found at {}",
                    role_name,
                    role_tasks_path.display()
                ));
            }
        }

        // 3. Execute tasks
        for task in &tasks {
            self.execute_task(ctx, task, &hosts, stats, &vars).await?;
        }

        // 4. Execute post_tasks
        for task in &post_tasks {
            self.execute_task(ctx, task, &hosts, stats, &vars).await?;
        }

        Ok(())
    }

    /// Resolve hosts from pattern
    fn resolve_hosts(&self, ctx: &CommandContext, pattern: &str) -> Result<Vec<String>> {
        // Simplified host resolution
        // In a real implementation, this would parse the inventory file

        if pattern == "localhost" || pattern == "127.0.0.1" {
            return Ok(vec!["localhost".to_string()]);
        }

        if pattern == "all" {
            // Load from inventory if available
            if let Some(inv_path) = ctx.inventory() {
                if inv_path.exists() {
                    let content = std::fs::read_to_string(inv_path)?;
                    let inventory: serde_yaml::Value = serde_yaml::from_str(&content)?;

                    let mut hosts = Vec::new();
                    if let Some(all) = inventory.get("all") {
                        if let Some(host_list) = all.get("hosts") {
                            if let Some(map) = host_list.as_mapping() {
                                for (key, _) in map {
                                    if let Some(host) = key.as_str() {
                                        hosts.push(host.to_string());
                                    }
                                }
                            }
                        }
                    }
                    if !hosts.is_empty() {
                        return Ok(hosts);
                    }
                }
            }
        }

        // Apply limit if specified
        if let Some(ref limit) = ctx.limit {
            if pattern.contains(limit) || limit.contains(pattern) {
                return Ok(vec![limit.clone()]);
            }
        }

        // Default to the pattern itself as a hostname
        Ok(vec![pattern.to_string()])
    }

    /// Execute a single task
    async fn execute_task(
        &self,
        ctx: &mut CommandContext,
        task: &serde_yaml::Value,
        hosts: &[String],
        stats: &Arc<Mutex<RecapStats>>,
        vars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<()> {
        // Get task name
        let task_name = task
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("Unnamed task");

        // Check tags
        if !self.should_run_task(task) {
            let mut stats_guard = stats.lock().await;
            for host in hosts {
                stats_guard.record(host, TaskStatus::Skipped);
            }
            return Ok(());
        }

        ctx.output.task_header(task_name);

        // Check conditions (when)
        let when_condition = task.get("when");

        // Execute on each host
        for host in hosts {
            // Check when condition (simplified)
            if let Some(when) = when_condition {
                let condition = when.as_str().unwrap_or("true");
                if condition == "false" {
                    ctx.output.task_result(
                        host,
                        TaskStatus::Skipped,
                        Some("conditional check failed"),
                    );
                    stats.lock().await.record(host, TaskStatus::Skipped);
                    continue;
                }
            }

            // Determine the module being used
            let (module, _args) = self.detect_module(task);

            // In check mode, don't actually execute
            if ctx.check_mode {
                ctx.output.task_result(
                    host,
                    TaskStatus::Changed,
                    Some(&format!("[check mode] would run: {}", module)),
                );
                stats.lock().await.record(host, TaskStatus::Changed);
                continue;
            }

            // Execute the task (simplified)
            let spinner = ctx
                .output
                .create_spinner(&format!("Executing on {}...", host));

            let result = self.execute_module(ctx, host, task, vars).await;

            if let Some(sp) = spinner {
                sp.finish_and_clear();
            }

            match result {
                Ok((changed, message)) => {
                    let status = if changed {
                        TaskStatus::Changed
                    } else {
                        TaskStatus::Ok
                    };
                    ctx.output.task_result(host, status, message.as_deref());
                    stats.lock().await.record(host, status);
                }
                Err(e) => {
                    // Check for ignore_errors
                    let ignore_errors = task
                        .get("ignore_errors")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    if ignore_errors {
                        ctx.output.task_result(
                            host,
                            TaskStatus::Ignored,
                            Some(&format!("ignored error: {}", e)),
                        );
                        stats.lock().await.record(host, TaskStatus::Ignored);
                    } else {
                        ctx.output
                            .task_result(host, TaskStatus::Failed, Some(&e.to_string()));
                        stats.lock().await.record(host, TaskStatus::Failed);
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if a task should run based on tags
    fn should_run_task(&self, task: &serde_yaml::Value) -> bool {
        // If no tags specified, run everything
        if self.tags.is_empty() && self.skip_tags.is_empty() {
            return true;
        }

        let task_tags: Vec<String> = task
            .get("tags")
            .and_then(|t| {
                if let Some(s) = t.as_str() {
                    Some(vec![s.to_string()])
                } else if let Some(seq) = t.as_sequence() {
                    Some(
                        seq.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default();

        // Check skip_tags first
        for skip_tag in &self.skip_tags {
            if task_tags.contains(skip_tag) {
                return false;
            }
        }

        // Check tags
        if !self.tags.is_empty() {
            for tag in &self.tags {
                if task_tags.contains(tag) || tag == "all" {
                    return true;
                }
            }
            return false;
        }

        true
    }

    /// Detect which module a task is using
    fn detect_module<'a>(
        &self,
        task: &'a serde_yaml::Value,
    ) -> (&'static str, Option<&'a serde_yaml::Value>) {
        // Common modules to check for
        let modules = [
            "command",
            "shell",
            "copy",
            "file",
            "template",
            "package",
            "apt",
            "yum",
            "dnf",
            "pip",
            "service",
            "systemd",
            "user",
            "group",
            "git",
            "debug",
            "set_fact",
            "include_tasks",
            "import_tasks",
            "block",
        ];

        for module in modules {
            if let Some(args) = task.get(module) {
                return (module, Some(args));
            }
        }

        ("unknown", None)
    }

    /// Execute a module (simplified implementation)
    async fn execute_module(
        &self,
        ctx: &CommandContext,
        host: &str,
        task: &serde_yaml::Value,
        vars: &IndexMap<String, serde_yaml::Value>,
    ) -> Result<(bool, Option<String>)> {
        let (module, args) = self.detect_module(task);

        ctx.output
            .debug(&format!("Executing module '{}' on host '{}'", module, host));

        // Handle debug module locally
        if module == "debug" {
            let mut message = String::new();
            if let Some(args) = args {
                if let Some(msg) = args.get("msg").and_then(|m| m.as_str()) {
                    let templated_msg = Self::template_string(msg, vars);
                    message = templated_msg;
                } else if let Some(var) = args.get("var").and_then(|v| v.as_str()) {
                    // Look up the variable value
                    let var_name = Self::template_string(var, vars);
                    if let Some(value) = vars.get(&var_name) {
                        message = format!("{} = {:?}", var_name, value);
                    } else {
                        message = format!("{} = <undefined>", var_name);
                    }
                }
            }
            return Ok((
                false,
                if message.is_empty() {
                    None
                } else {
                    Some(message)
                },
            ));
        }

        // Handle set_fact locally (no remote execution needed)
        if module == "set_fact" {
            return Ok((true, None));
        }

        // For command/shell modules, execute remotely if not localhost
        if module == "command" || module == "shell" {
            let cmd = if let Some(args) = args {
                args.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| {
                        args.get("cmd")
                            .and_then(|c| c.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };

            if cmd.is_empty() {
                return Err(anyhow::anyhow!("No command specified"));
            }

            if host == "localhost" || host == "127.0.0.1" {
                // Local execution
                ctx.output.debug(&format!("Local execution: {}", cmd));
                let parts: Vec<&str> = cmd.split_whitespace().collect();
                if parts.is_empty() {
                    return Err(anyhow::anyhow!("Empty command"));
                }

                let output =
                    std::process::Command::new(if module == "shell" { "sh" } else { parts[0] })
                        .args(if module == "shell" {
                            vec!["-c", &cmd]
                        } else {
                            parts[1..].to_vec()
                        })
                        .output()
                        .map_err(|e| anyhow::anyhow!("Failed to execute command: {}", e))?;

                if output.status.success() {
                    return Ok((true, None));
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow::anyhow!("Command failed: {}", stderr));
                }
            } else {
                // Remote execution via SSH
                let success = self.execute_remote_command(ctx, host, &cmd).await?;
                return Ok((success, None));
            }
        }

        // For other modules, simulate execution for now
        Ok((true, None))
    }

    /// Execute a command on a remote host via SSH
    /// Uses connection pooling to reuse connections across multiple commands
    async fn execute_remote_command(
        &self,
        ctx: &CommandContext,
        host: &str,
        cmd: &str,
    ) -> Result<bool> {
        // Get host connection details from inventory
        let (ansible_host, ansible_user, ansible_port, ansible_key) =
            self.get_host_connection_info(ctx, host)?;

        // Get or create a pooled connection
        let conn = ctx
            .get_connection(
                host,
                &ansible_host,
                &ansible_user,
                ansible_port,
                ansible_key.as_deref(),
            )
            .await?;

        // Execute command on the pooled connection
        let result = conn
            .execute(cmd, None)
            .await
            .map_err(|e| anyhow::anyhow!("Command execution failed: {}", e))?;

        if result.success {
            Ok(true)
        } else {
            Err(anyhow::anyhow!(
                "Command failed with exit code {}: {}",
                result.exit_code,
                if result.stderr.is_empty() {
                    result.stdout
                } else {
                    result.stderr
                }
            ))
        }
    }

    /// Get connection info for a host from inventory
    fn get_host_connection_info(
        &self,
        ctx: &CommandContext,
        host: &str,
    ) -> Result<(String, String, u16, Option<String>)> {
        // Try to load from inventory
        if let Some(inv_path) = ctx.inventory() {
            if inv_path.exists() {
                let content = std::fs::read_to_string(inv_path)?;
                let inventory: serde_yaml::Value = serde_yaml::from_str(&content)?;

                // Look for host-specific vars
                if let Some(all) = inventory.get("all") {
                    // Get global vars
                    let global_user = all
                        .get("vars")
                        .and_then(|v| v.get("ansible_user"))
                        .and_then(|u| u.as_str())
                        .map(|s| s.to_string());
                    let global_key = all
                        .get("vars")
                        .and_then(|v| v.get("ansible_ssh_private_key_file"))
                        .and_then(|k| k.as_str())
                        .map(|s| s.to_string());

                    // Get host-specific vars
                    if let Some(hosts) = all.get("hosts") {
                        if let Some(host_config) = hosts.get(host) {
                            let ansible_host = host_config
                                .get("ansible_host")
                                .and_then(|h| h.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| host.to_string());
                            let ansible_user = host_config
                                .get("ansible_user")
                                .and_then(|u| u.as_str())
                                .map(|s| s.to_string())
                                .or(global_user)
                                .unwrap_or_else(|| {
                                    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
                                });
                            let ansible_port = host_config
                                .get("ansible_port")
                                .and_then(|p| p.as_u64())
                                .unwrap_or(22)
                                as u16;
                            let ansible_key = host_config
                                .get("ansible_ssh_private_key_file")
                                .and_then(|k| k.as_str())
                                .map(|s| s.to_string())
                                .or(global_key);

                            return Ok((ansible_host, ansible_user, ansible_port, ansible_key));
                        }
                    }
                }
            }
        }

        // Default: use host as-is with current user
        let user = self
            .user
            .clone()
            .unwrap_or_else(|| std::env::var("USER").unwrap_or_else(|_| "root".to_string()));
        let key = self
            .private_key
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        Ok((host.to_string(), user, 22, key))
    }

    /// Validate a limit pattern
    /// Returns an error message if the pattern is invalid
    fn validate_limit_pattern(limit: &str) -> std::result::Result<(), String> {
        // Check for limit from file (@filename)
        if let Some(file_path) = limit.strip_prefix('@') {
            let path = std::path::Path::new(file_path);
            if !path.exists() {
                return Err(format!("Limit file not found: {}", file_path));
            }
            return Ok(());
        }

        // Split by colon to check each part
        for part in limit.split(':') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            // Strip leading operators (!, &)
            let pattern = part.trim_start_matches('!').trim_start_matches('&');

            // Check for regex pattern
            if let Some(regex_str) = pattern.strip_prefix('~') {
                if regex::Regex::new(regex_str).is_err() {
                    return Err(format!("Invalid regex pattern in limit: {}", regex_str));
                }
            }
        }

        Ok(())
    }

    /// Template a string by replacing {{ variable }} patterns with values
    fn template_string(template: &str, vars: &IndexMap<String, serde_yaml::Value>) -> String {
        // Simple Jinja2-like templating for {{ variable }} syntax
        let re = Regex::new(r"\{\{\s*([^}]+?)\s*\}\}").unwrap();
        let mut result = template.to_string();

        for cap in re.captures_iter(template) {
            let full_match = cap.get(0).unwrap().as_str();
            let expr = cap.get(1).unwrap().as_str().trim();

            // Handle simple variable lookup (no filters for now)
            let var_name = expr.split('|').next().unwrap_or(expr).trim();

            if let Some(value) = vars.get(var_name) {
                let replacement = Self::yaml_value_to_string(value);
                result = result.replace(full_match, &replacement);
            }
            // If variable not found, leave the original template expression
        }

        result
    }

    /// Convert a YAML value to a display string
    fn yaml_value_to_string(value: &serde_yaml::Value) -> String {
        match value {
            serde_yaml::Value::Null => String::new(),
            serde_yaml::Value::Bool(b) => b.to_string(),
            serde_yaml::Value::Number(n) => n.to_string(),
            serde_yaml::Value::String(s) => s.clone(),
            serde_yaml::Value::Sequence(seq) => {
                let items: Vec<String> = seq.iter().map(Self::yaml_value_to_string).collect();
                format!("[{}]", items.join(", "))
            }
            serde_yaml::Value::Mapping(map) => {
                let items: Vec<String> = map
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}: {}",
                            Self::yaml_value_to_string(k),
                            Self::yaml_value_to_string(v)
                        )
                    })
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
            serde_yaml::Value::Tagged(tagged) => Self::yaml_value_to_string(&tagged.value),
        }
    }
}

#[async_trait::async_trait]
impl Runnable for RunArgs {
    async fn run(&self, ctx: &mut CommandContext) -> Result<i32> {
        self.execute(ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_args_parsing() {
        let args = RunArgs::try_parse_from(["run", "playbook.yml"]).unwrap();
        assert_eq!(args.playbook, PathBuf::from("playbook.yml"));
    }

    #[test]
    fn test_run_args_with_tags() {
        let args = RunArgs::try_parse_from([
            "run",
            "playbook.yml",
            "--tags",
            "install",
            "--tags",
            "configure",
        ])
        .unwrap();
        assert_eq!(args.tags, vec!["install", "configure"]);
    }

    #[test]
    fn test_run_args_become() {
        let args =
            RunArgs::try_parse_from(["run", "playbook.yml", "--become", "--become-user", "admin"])
                .unwrap();
        assert!(args.r#become);
        assert_eq!(args.become_user, "admin");
    }

    #[test]
    fn test_run_args_plan_flag() {
        let args = RunArgs::try_parse_from(["run", "playbook.yml", "--plan"]).unwrap();
        assert!(args.plan);
    }

    #[test]
    fn test_get_action_description_command() {
        let run_args = RunArgs::try_parse_from(["run", "playbook.yml"]).unwrap();
        let vars = IndexMap::new();

        let cmd_value = serde_yaml::Value::String("echo hello".to_string());
        let desc = run_args.get_action_description("command", Some(&cmd_value), &vars);
        assert_eq!(desc, "will execute: echo hello");
    }

    #[test]
    fn test_get_action_description_package() {
        let run_args = RunArgs::try_parse_from(["run", "playbook.yml"]).unwrap();
        let vars = IndexMap::new();

        let mut args = serde_yaml::Mapping::new();
        args.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String("nginx".to_string()),
        );
        args.insert(
            serde_yaml::Value::String("state".to_string()),
            serde_yaml::Value::String("present".to_string()),
        );
        let package_value = serde_yaml::Value::Mapping(args);

        let desc = run_args.get_action_description("apt", Some(&package_value), &vars);
        assert_eq!(desc, "will install package: nginx");
    }

    #[test]
    fn test_get_action_description_service() {
        let run_args = RunArgs::try_parse_from(["run", "playbook.yml"]).unwrap();
        let vars = IndexMap::new();

        let mut args = serde_yaml::Mapping::new();
        args.insert(
            serde_yaml::Value::String("name".to_string()),
            serde_yaml::Value::String("nginx".to_string()),
        );
        args.insert(
            serde_yaml::Value::String("state".to_string()),
            serde_yaml::Value::String("started".to_string()),
        );
        let service_value = serde_yaml::Value::Mapping(args);

        let desc = run_args.get_action_description("service", Some(&service_value), &vars);
        assert_eq!(desc, "will ensure service nginx is started");
    }

    #[test]
    fn test_get_action_description_copy() {
        let run_args = RunArgs::try_parse_from(["run", "playbook.yml"]).unwrap();
        let vars = IndexMap::new();

        let mut args = serde_yaml::Mapping::new();
        args.insert(
            serde_yaml::Value::String("src".to_string()),
            serde_yaml::Value::String("/tmp/source".to_string()),
        );
        args.insert(
            serde_yaml::Value::String("dest".to_string()),
            serde_yaml::Value::String("/tmp/dest".to_string()),
        );
        let copy_value = serde_yaml::Value::Mapping(args);

        let desc = run_args.get_action_description("copy", Some(&copy_value), &vars);
        assert_eq!(desc, "will copy /tmp/source to /tmp/dest");
    }

    #[test]
    fn test_get_action_description_debug() {
        let run_args = RunArgs::try_parse_from(["run", "playbook.yml"]).unwrap();
        let vars = IndexMap::new();

        let mut args = serde_yaml::Mapping::new();
        args.insert(
            serde_yaml::Value::String("msg".to_string()),
            serde_yaml::Value::String("Hello World".to_string()),
        );
        let debug_value = serde_yaml::Value::Mapping(args);

        let desc = run_args.get_action_description("debug", Some(&debug_value), &vars);
        assert_eq!(desc, "will display: Hello World");
    }

    #[test]
    fn test_get_action_description_with_variables() {
        let run_args = RunArgs::try_parse_from(["run", "playbook.yml"]).unwrap();
        let mut vars = IndexMap::new();
        vars.insert(
            "package_name".to_string(),
            serde_yaml::Value::String("nginx".to_string()),
        );

        let cmd_value = serde_yaml::Value::String("install {{ package_name }}".to_string());
        let desc = run_args.get_action_description("command", Some(&cmd_value), &vars);
        assert_eq!(desc, "will execute: install nginx");
    }
}
