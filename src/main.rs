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
use rustible::playbook::Playbook;
use rustible::schema::{SchemaValidator, ValidatorConfig};
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
        Commands::Drift(args) => args.execute(&mut ctx).await?,
        Commands::Lock(args) => {
            args.execute().await?;
            0
        }
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

/// Validate a playbook using SchemaValidator and typed parsing
async fn validate_playbook(
    playbook_path: &std::path::Path,
    ctx: &mut CommandContext,
) -> Result<i32> {
    ctx.output.banner("PLAYBOOK VALIDATION");
    ctx.output
        .info(&format!("Validating: {}", playbook_path.display()));

    if !playbook_path.exists() {
        ctx.output
            .error(&format!("Playbook not found: {}", playbook_path.display()));
        return Ok(1);
    }

    // Read the playbook content
    let content = std::fs::read_to_string(playbook_path)?;

    // Phase 1: Try typed parsing with Playbook::from_yaml for better error messages
    ctx.output.section("Syntax Check");
    let playbook = match Playbook::from_yaml(&content, Some(playbook_path.to_path_buf())) {
        Ok(pb) => pb,
        Err(e) => {
            let err = e.to_string();

            // Provide a friendlier, stable error message for common structural mistakes.
            if err.contains("missing field `hosts`") {
                ctx.output
                    .error(&format!("missing required 'hosts' field ({})", e));
            } else {
                ctx.output.error(&format!("Playbook parse error: {}", e));
            }
            return Ok(1);
        }
    };

    // Structural warnings (non-fatal)
    for play in &playbook.plays {
        // Warn on plays that have no tasks *and* no roles.
        if play.task_count() == 0 && play.roles.is_empty() {
            let play_name = if play.name.is_empty() {
                "<unnamed>"
            } else {
                &play.name
            };
            ctx.output.warning(&format!(
                "Play '{}' has no tasks (nothing to execute)",
                play_name
            ));
        }

        // Warn on handlers that rely only on `listen` without a `name`.
        for handler in &play.handlers {
            if handler.name.is_empty() && !handler.listen.is_empty() {
                ctx.output.warning(&format!(
                    "Handler with listen [{}] has no name",
                    handler.listen.join(", ")
                ));
            }
        }
    }

    ctx.output.debug("Playbook syntax is valid");

    // Phase 2: Schema validation for module arguments
    ctx.output.section("Schema Validation");
    let validator_config = ValidatorConfig {
        strict_mode: false,
        check_deprecations: true,
        check_undefined_vars: false, // Templates may have dynamic vars
        max_depth: 50,
        custom_schema_dir: None,
    };
    let validator = SchemaValidator::with_config(validator_config);

    let result = match validator.validate_file(playbook_path) {
        Ok(r) => r,
        Err(e) => {
            ctx.output
                .error(&format!("Schema validation failed: {}", e));
            return Ok(1);
        }
    };

    // Output validation results
    let mut error_count = 0;
    let mut warning_count = 0;
    let mut _info_count = 0;

    // Print errors
    for error in &result.errors {
        error_count += 1;
        let location = if let (Some(line), Some(col)) = (error.line, error.column) {
            format!("{}:{}", line, col)
        } else {
            error.path.clone()
        };

        ctx.output
            .error(&format!("[{}] {}", location, error.message));

        if let Some(ref suggestion) = error.suggestion {
            ctx.output.info(&format!("  suggestion: {}", suggestion));
        }
    }

    // Print warnings
    for warning in &result.warnings {
        warning_count += 1;
        let location = if let (Some(line), Some(col)) = (warning.line, warning.column) {
            format!("{}:{}", line, col)
        } else {
            warning.path.clone()
        };

        ctx.output
            .warning(&format!("[{}] {}", location, warning.message));

        if let Some(ref suggestion) = warning.suggestion {
            ctx.output.info(&format!("  suggestion: {}", suggestion));
        }
    }

    // Print info (only in verbose mode)
    for info in &result.info {
        _info_count += 1;
        ctx.output
            .debug(&format!("[{}] {}", info.path, info.message));
    }

    // Print summary
    ctx.output.section("Validation Results");

    if result.valid && warning_count == 0 {
        ctx.output.info("Playbook is valid. No issues found.");
        Ok(0)
    } else if result.valid {
        ctx.output.warning(&format!(
            "Playbook is valid with {} warning(s)",
            warning_count
        ));
        Ok(0)
    } else {
        ctx.output.error(&format!(
            "Playbook validation failed: {} error(s), {} warning(s)",
            error_count, warning_count
        ));
        Ok(1)
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
