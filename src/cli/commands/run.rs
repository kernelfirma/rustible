//! Run command - Execute a playbook
//!
//! This module implements the `run` subcommand for executing Ansible-like playbooks.

use super::{CommandContext, Runnable};
use crate::cli::output::RecapStats;
use anyhow::Result;
use clap::Parser;
use dialoguer::theme::ColorfulTheme;
use indexmap::IndexMap;
use regex::Regex;
use rustible::diagnostics::yaml_syntax_error;
use std::path::PathBuf;
use std::time::Instant;

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

    /// Ask for SSH password
    #[arg(short = 'k', long = "ask-pass")]
    pub ask_pass: bool,

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

    /// Agent mode - use persistent agent on targets for faster execution
    #[arg(long)]
    pub agent_mode: bool,

    /// Agent socket path on remote hosts
    #[arg(long, default_value = "/var/run/rustible-agent.sock")]
    pub agent_socket: String,
}

impl RunArgs {
    /// Execute the run command using the executor as the single runtime
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        use rustible::executor::runtime::RuntimeContext;
        use rustible::executor::{ExecutionStrategy, Executor, ExecutorConfig, Playbook};
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
        let spinner = ctx.output.create_spinner("Loading playbook...");
        let playbook = match Playbook::load(&self.playbook) {
            Ok(pb) => {
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                pb
            }
            Err(e) => {
                if let Some(sp) = spinner {
                    sp.finish_and_clear();
                }
                if let Some(rendered) = e.render_diagnostic() {
                    ctx.output.diagnostic(&rendered);
                } else {
                    ctx.output
                        .error(&format!("Failed to parse playbook: {}", e));
                }
                return Ok(1);
            }
        };

        // Setup event callback for real-time output
        let output = ctx.output.clone();
        let current_task_start = std::sync::Arc::new(std::sync::Mutex::new(None));
        let task_start_clone = current_task_start.clone();

        let callback = std::sync::Arc::new(move |event: rustible::executor::ExecutionEvent| {
            use crate::cli::output::TaskStatus as CliTaskStatus;
            use rustible::executor::task::TaskStatus as ExecutorTaskStatus;
            use rustible::executor::ExecutionEvent;

            match event {
                ExecutionEvent::PlayStart(name) => {
                    let play_name = if name.is_empty() {
                        "Unnamed play"
                    } else {
                        &name
                    };
                    output.play_header(play_name);
                }
                ExecutionEvent::TaskStart(name) => {
                    output.task_header(&name);
                    if let Ok(mut start) = task_start_clone.lock() {
                        *start = Some(std::time::Instant::now());
                    }
                }
                ExecutionEvent::HostTaskComplete(host, _, result) => {
                    let status = match result.status {
                        ExecutorTaskStatus::Ok => CliTaskStatus::Ok,
                        ExecutorTaskStatus::Changed => CliTaskStatus::Changed,
                        ExecutorTaskStatus::Failed => CliTaskStatus::Failed,
                        ExecutorTaskStatus::Skipped => CliTaskStatus::Skipped,
                        ExecutorTaskStatus::Unreachable => CliTaskStatus::Unreachable,
                    };

                    let duration = if let Ok(start) = task_start_clone.lock() {
                        start.map(|s| s.elapsed())
                    } else {
                        None
                    };

                    output.task_result(&host, status, result.msg.as_deref(), duration);
                }
                _ => {}
            }
        });

        // Get inventory path and load inventory
        let inventory_path = ctx.inventory().cloned();
        let runtime = if let Some(inv_path) = &inventory_path {
            if inv_path.exists() {
                match Inventory::load(inv_path) {
                    Ok(inventory) => {
                        ctx.output
                            .debug(&format!("Loaded inventory from: {}", inv_path.display()));
                        RuntimeContext::from_inventory(&inventory)
                    }
                    Err(e) => {
                        ctx.output
                            .warning(&format!("Failed to load inventory: {}", e));
                        RuntimeContext::new()
                    }
                }
            } else {
                ctx.output
                    .warning(&format!("Inventory file not found: {}", inv_path.display()));
                RuntimeContext::new()
            }
        } else {
            ctx.output
                .warning("No inventory specified, using localhost");
            ctx.output
                .hint("Use -i <inventory_file> to specify an inventory");
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
            ctx.output
                .plan("WARNING: Running in PLAN MODE - showing execution plan only");
            let playbook_content = std::fs::read_to_string(&self.playbook)?;
            let playbook_yaml: serde_yaml::Value = match serde_yaml::from_str(&playbook_content) {
                Ok(value) => value,
                Err(e) => {
                    let (line, col) = e
                        .location()
                        .map_or((1, 1), |loc| (loc.line(), loc.column()));
                    let diagnostic = yaml_syntax_error(
                        &self.playbook,
                        &playbook_content,
                        line,
                        col,
                        &e.to_string(),
                    );
                    ctx.output
                        .diagnostic(&diagnostic.render_with_source(Some(&playbook_content)));
                    return Ok(1);
                }
            };
            if let Some(plays) = playbook_yaml.as_sequence() {
                let extra_vars_for_plan: std::collections::HashMap<String, serde_yaml::Value> =
                    ctx.parse_extra_vars()?;
                self.show_plan(ctx, plays, &extra_vars_for_plan).await?;
            }
            let duration = start_time.elapsed();
            ctx.output
                .info(&format!("Plan completed in {:.2}s", duration.as_secs_f64()));
            return Ok(0);
        }

        // Check mode notice
        if ctx.check_mode {
            ctx.output
                .warning("Running in CHECK MODE - no changes will be made");
        }

        let has_extra_ssh_pass = extra_vars.contains_key("ansible_ssh_pass");
        let has_inventory_ssh_pass = runtime
            .hosts()
            .iter()
            .any(|host| runtime.get_var("ansible_ssh_pass", Some(host)).is_some());
        let ssh_password = if !has_extra_ssh_pass && !has_inventory_ssh_pass {
            if self.ask_pass {
                Some(Self::prompt_ssh_password(ctx, self.user.as_deref())?)
            } else {
                crate::cli::env::ssh_password()
            }
        } else {
            None
        };
        if let Some(password) = ssh_password {
            extra_vars.insert("ansible_ssh_pass".to_string(), serde_json::json!(password));
        }

        let ask_become_pass = Self::should_prompt_become_password(
            self.ask_become_pass,
            ctx.config.privilege_escalation.become_ask_pass,
        );
        let become_password = if ask_become_pass {
            Some(Self::prompt_become_password(ctx, &self.become_user)?)
        } else {
            None
        };

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
            r#become: self.r#become,
            become_method: self.become_method.clone(),
            become_user: self.become_user.clone(),
            become_password,
        };

        // Create executor with runtime context and event callback
        let executor =
            Executor::with_runtime(executor_config, runtime).with_event_callback(callback);

        // Run playbook using executor
        ctx.output
            .info(&format!("Running playbook: {}", playbook.name));
        let results = match executor.run_playbook(&playbook).await {
            Ok(results) => results,
            Err(e) => {
                if let Some(rendered) = e.render_diagnostic() {
                    ctx.output.diagnostic(&rendered);
                } else {
                    ctx.output
                        .error(&format!("Playbook execution failed: {}", e));
                }
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

        // Return exit code
        if has_failures {
            if ctx.verbosity == 0 {
                ctx.output.hint("Run with -v for more details on failures");
            }
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
                "*",
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
                    ">", task_num, total, task_name
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

    /// Check if a task should run based on tags
    fn should_run_task(&self, task: &serde_yaml::Value) -> bool {
        let tags = Self::normalize_tags(&self.tags);
        let skip_tags = Self::normalize_tags(&self.skip_tags);

        // If no tags specified, run everything
        if tags.is_empty() && skip_tags.is_empty() {
            return true;
        }

        let task_tags: Vec<String> = task
            .get("tags")
            .and_then(|t| {
                if let Some(s) = t.as_str() {
                    Some(vec![s.to_string()])
                } else {
                    t.as_sequence().map(|seq| {
                        seq.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                }
            })
            .unwrap_or_default();

        // Check skip_tags first
        for skip_tag in &skip_tags {
            if task_tags.contains(skip_tag) {
                return false;
            }
        }

        // Check tags
        if !tags.is_empty() {
            for tag in &tags {
                if task_tags.contains(tag) || tag == "all" {
                    return true;
                }
            }
            return false;
        }

        true
    }

    fn normalize_tags(tags: &[String]) -> Vec<String> {
        tags.iter()
            .flat_map(|tag| tag.split(','))
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
            .map(|tag| tag.to_string())
            .collect()
    }

    fn should_prompt_become_password(ask_become_pass: bool, config_ask_pass: bool) -> bool {
        ask_become_pass || config_ask_pass
    }

    fn prompt_ssh_password(ctx: &CommandContext, user: Option<&str>) -> Result<String> {
        ctx.output.flush();
        let prompt = if let Some(u) = user {
            format!("🔑 SSH Password for '{}'", u)
        } else {
            "🔑 SSH Password".to_string()
        };

        let password = dialoguer::Password::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .interact()?;
        Ok(password)
    }

    fn prompt_become_password(ctx: &CommandContext, user: &str) -> Result<String> {
        ctx.output.flush();
        let prompt = format!("⚡ Privilege Escalation Password for '{}'", user);

        let password = dialoguer::Password::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .interact()?;
        Ok(password)
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
    fn test_normalize_tags_with_commas() {
        let args = RunArgs::try_parse_from([
            "run",
            "playbook.yml",
            "--tags",
            "install, configure",
            "--skip-tags",
            "slow, noisy",
        ])
        .unwrap();

        assert_eq!(
            RunArgs::normalize_tags(&args.tags),
            vec!["install", "configure"]
        );
        assert_eq!(
            RunArgs::normalize_tags(&args.skip_tags),
            vec!["slow", "noisy"]
        );
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
    fn test_run_args_ask_become_pass_parsing() {
        let args = RunArgs::try_parse_from(["run", "playbook.yml", "--ask-become-pass"]).unwrap();
        assert!(args.ask_become_pass);

        let args = RunArgs::try_parse_from(["run", "playbook.yml", "-K"]).unwrap();
        assert!(args.ask_become_pass);
    }

    #[test]
    fn test_run_args_plan_flag() {
        let args = RunArgs::try_parse_from(["run", "playbook.yml", "--plan"]).unwrap();
        assert!(args.plan);
    }

    #[test]
    fn test_should_prompt_become_password_gating() {
        assert!(RunArgs::should_prompt_become_password(true, false));
        assert!(RunArgs::should_prompt_become_password(false, true));
        assert!(!RunArgs::should_prompt_become_password(false, false));
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
