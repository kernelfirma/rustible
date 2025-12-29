//! Rustible - An Ansible substitute written in Rust
//!
//! A fast, safe, and modern configuration management and automation tool.
//!
//! This is the main entry point for the Rustible CLI.

// Development-time allowances
#![allow(dead_code)]
#![allow(unused_variables)]

mod cli;
mod config;

use anyhow::Result;
use cli::commands::CommandContext;
use cli::{Cli, Commands};
use config::Config;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Application version information
const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let cli = Cli::parse_args();

    // Initialize logging based on verbosity
    init_logging(cli.verbosity());

    // Display version if verbose
    if cli.verbosity() >= 2 {
        eprintln!("Rustible v{} by {}", VERSION, AUTHORS);
    }

    // Load configuration
    let config = Config::load(cli.config.as_ref()).unwrap_or_else(|e| {
        if cli.verbosity() >= 1 {
            eprintln!("Warning: Failed to load config: {}", e);
        }
        Config::default()
    });

    // Create command context
    let mut ctx = CommandContext::new(&cli, config);

    // Execute the appropriate command
    let exit_code = match &cli.command {
        Commands::Run(args) => args.execute(&mut ctx).await?,
        Commands::Check(args) => args.execute(&mut ctx).await?,
        Commands::ListHosts(args) => args.execute(&mut ctx).await?,
        Commands::ListTasks(args) => args.execute(&mut ctx).await?,
        Commands::Vault(args) => args.execute(&mut ctx).await?,
        Commands::Galaxy(args) => cli::commands::galaxy::execute(args, &ctx).await?,
        Commands::Init(args) => init_project(&args.path, &args.template, &mut ctx).await?,
        Commands::Validate(args) => validate_playbook(&args.playbook, &mut ctx).await?,
        Commands::Provision(args) => execute_provision(&args.command, &mut ctx).await?,
    };

    std::process::exit(exit_code);
}

/// Initialize logging based on verbosity level
fn init_logging(verbosity: u8) {
    let filter = match verbosity {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(verbosity >= 3))
        .with(env_filter)
        .init();
}

/// Initialize a new Rustible project
async fn init_project(
    path: &std::path::Path,
    template: &str,
    ctx: &mut CommandContext,
) -> Result<i32> {
    use std::fs;

    ctx.output.banner("RUSTIBLE INIT");
    ctx.output.info(&format!(
        "Initializing Rustible project in: {}",
        path.display()
    ));

    // Ensure the base path exists
    if !path.exists() {
        fs::create_dir_all(path)?;
    }

    // Create directory structure
    let dirs = [
        "inventory",
        "playbooks",
        "roles",
        "group_vars",
        "host_vars",
        "files",
        "templates",
    ];

    for dir in &dirs {
        let dir_path = path.join(dir);
        if !dir_path.exists() {
            fs::create_dir_all(&dir_path)?;
            ctx.output.info(&format!("Created: {}/", dir));
        }
    }

    // Create sample inventory
    let inventory_content = r#"# Rustible Inventory File
# Define your hosts and groups here

all:
  hosts:
    localhost:
      ansible_connection: local

  children:
    webservers:
      hosts: {}
    dbservers:
      hosts: {}
"#;
    let inventory_path = path.join("inventory/hosts.yml");
    if !inventory_path.exists() {
        fs::write(&inventory_path, inventory_content)?;
        ctx.output.info("Created: inventory/hosts.yml");
    }

    // Create sample playbook based on template
    let playbook_content = match template {
        "webserver" => {
            r#"---
# Web Server Playbook
- name: Configure web servers
  hosts: webservers
  r#become: true
  gather_facts: true

  vars:
    http_port: 80
    document_root: /var/www/html

  tasks:
    - name: Install web server packages
      package:
        name:
          - nginx
        state: present

    - name: Ensure nginx is running
      service:
        name: nginx
        state: started
        enabled: true

    - name: Create document root
      file:
        path: "{{ document_root }}"
        state: directory
        mode: '0755'

  handlers:
    - name: Restart nginx
      service:
        name: nginx
        state: restarted
"#
        }
        "docker" => {
            r#"---
# Docker Playbook
- name: Setup Docker
  hosts: all
  r#become: true
  gather_facts: true

  tasks:
    - name: Install Docker dependencies
      package:
        name:
          - apt-transport-https
          - ca-certificates
          - curl
          - gnupg
        state: present

    - name: Install Docker
      package:
        name: docker.io
        state: present

    - name: Ensure Docker is running
      service:
        name: docker
        state: started
        enabled: true

    - name: Add user to docker group
      user:
        name: "{{ ansible_user }}"
        groups: docker
        append: true
"#
        }
        _ => {
            r#"---
# Sample Rustible Playbook
- name: Sample playbook
  hosts: localhost
  gather_facts: true

  vars:
    greeting: "Hello from Rustible!"

  tasks:
    - name: Print greeting message
      debug:
        msg: "{{ greeting }}"

    - name: Gather system information
      debug:
        msg: "Running on {{ ansible_os_family }} {{ ansible_distribution_version }}"
      when: ansible_os_family is defined

    - name: Create a test file
      file:
        path: /tmp/rustible_test
        state: touch
        mode: '0644'
"#
        }
    };

    let playbook_path = path.join("playbooks/site.yml");
    if !playbook_path.exists() {
        fs::write(&playbook_path, playbook_content)?;
        ctx.output.info("Created: playbooks/site.yml");
    }

    // Create config file
    let config_content = r#"# Rustible Configuration File
# This file uses TOML format

[defaults]
inventory = "inventory/hosts.yml"
forks = 5
timeout = 30
gathering = true
host_key_checking = true
retry_files_enabled = false

[ssh]
pipelining = true
retries = 3

[privilege_escalation]
become = false
become_method = "sudo"
become_user = "root"

[colors]
enabled = true
ok = "green"
changed = "yellow"
error = "red"
skipped = "cyan"

[logging]
log_level = "info"
log_timestamp = true
"#;
    let config_path = path.join("rustible.cfg");
    if !config_path.exists() {
        fs::write(&config_path, config_content)?;
        ctx.output.info("Created: rustible.cfg");
    }

    // Create .gitignore
    let gitignore_content = r#"# Rustible
*.retry
.vault_pass
*.pyc
__pycache__/

# Editor
*.swp
*.swo
*~
.idea/
.vscode/

# OS
.DS_Store
Thumbs.db
"#;
    let gitignore_path = path.join(".gitignore");
    if !gitignore_path.exists() {
        fs::write(&gitignore_path, gitignore_content)?;
        ctx.output.info("Created: .gitignore");
    }

    ctx.output.section("Project initialized successfully!");
    ctx.output.info(&format!("Template: '{}'", template));
    ctx.output
        .info("Run 'rustible run playbooks/site.yml' to test your setup.");

    Ok(0)
}

/// Execute a provisioning command
async fn execute_provision(
    command: &cli::commands::provision::ProvisionCommands,
    ctx: &mut CommandContext,
) -> Result<i32> {
    use cli::commands::provision::ProvisionCommands;

    match command {
        ProvisionCommands::Plan(args) => args.execute(ctx).await,
        ProvisionCommands::Apply(args) => args.execute(ctx).await,
        ProvisionCommands::Destroy(args) => args.execute(ctx).await,
        ProvisionCommands::Import(args) => args.execute(ctx).await,
        ProvisionCommands::Show(args) => args.execute(ctx).await,
        ProvisionCommands::Refresh(args) => args.execute(ctx).await,
        ProvisionCommands::Init(args) => args.execute(ctx).await,
    }
}

/// Validate a playbook
async fn validate_playbook(playbook: &std::path::Path, ctx: &mut CommandContext) -> Result<i32> {
    ctx.output.banner("PLAYBOOK VALIDATION");
    ctx.output
        .info(&format!("Validating: {}", playbook.display()));

    if !playbook.exists() {
        ctx.output
            .error(&format!("Playbook not found: {}", playbook.display()));
        return Ok(1);
    }

    // Read and parse the playbook
    let content = std::fs::read_to_string(playbook)?;

    match serde_yaml::from_str::<serde_yaml::Value>(&content) {
        Ok(value) => {
            // Basic structure validation
            if let Some(plays) = value.as_sequence() {
                let mut errors = 0;
                let mut warnings = 0;

                for (i, play) in plays.iter().enumerate() {
                    let play_num = i + 1;
                    let play_name = play
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("unnamed");

                    ctx.output
                        .debug(&format!("Validating play {}: {}", play_num, play_name));

                    // Check required 'hosts' field
                    if play.get("hosts").is_none() {
                        ctx.output.error(&format!(
                            "Play {} '{}': missing required 'hosts' field",
                            play_num, play_name
                        ));
                        errors += 1;
                    }

                    // Check for tasks or roles
                    let has_tasks = play.get("tasks").is_some();
                    let has_roles = play.get("roles").is_some();
                    let has_pre_tasks = play.get("pre_tasks").is_some();
                    let has_post_tasks = play.get("post_tasks").is_some();

                    if !has_tasks && !has_roles && !has_pre_tasks && !has_post_tasks {
                        ctx.output.warning(&format!(
                            "Play {} '{}': no tasks, roles, pre_tasks, or post_tasks defined",
                            play_num, play_name
                        ));
                        warnings += 1;
                    }

                    // Validate tasks
                    if let Some(tasks) = play.get("tasks").and_then(|t| t.as_sequence()) {
                        for (j, task) in tasks.iter().enumerate() {
                            let task_name = task
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("unnamed");

                            // Check that task has at least one module
                            let has_module = task.as_mapping().map_or(false, |m| {
                                m.keys().any(|k| {
                                    let key = k.as_str().unwrap_or("");
                                    !matches!(
                                        key,
                                        "name"
                                            | "when"
                                            | "tags"
                                            | "register"
                                            | "ignore_errors"
                                            | "become"
                                            | "become_user"
                                            | "delegate_to"
                                            | "notify"
                                            | "loop"
                                            | "with_items"
                                            | "vars"
                                    )
                                })
                            });

                            if !has_module {
                                ctx.output.warning(&format!(
                                    "Task {} in play {}: '{}' has no module defined",
                                    j + 1,
                                    play_num,
                                    task_name
                                ));
                                warnings += 1;
                            }
                        }
                    }

                    // Validate handlers
                    if let Some(handlers) = play.get("handlers").and_then(|h| h.as_sequence()) {
                        for (j, handler) in handlers.iter().enumerate() {
                            if handler.get("name").is_none() {
                                ctx.output.warning(&format!(
                                    "Handler {} in play {}: missing 'name' field",
                                    j + 1,
                                    play_num
                                ));
                                warnings += 1;
                            }
                        }
                    }
                }

                // Print summary
                ctx.output.section("Validation Results");

                if errors == 0 && warnings == 0 {
                    ctx.output
                        .info("Playbook syntax is valid. No issues found.");
                    Ok(0)
                } else if errors == 0 {
                    ctx.output
                        .warning(&format!("Playbook is valid with {} warning(s)", warnings));
                    Ok(0)
                } else {
                    ctx.output.error(&format!(
                        "Playbook has {} error(s) and {} warning(s)",
                        errors, warnings
                    ));
                    Ok(1)
                }
            } else {
                ctx.output.error("Playbook must be a list of plays");
                Ok(1)
            }
        }
        Err(e) => {
            ctx.output.error(&format!("YAML parse error: {}", e));
            Ok(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
