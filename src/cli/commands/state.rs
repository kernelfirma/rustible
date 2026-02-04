//! State management command
//!
//! This module implements the `state` subcommand for managing state.

use super::CommandContext;
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Arguments for the state command
#[derive(Parser, Debug, Clone)]
pub struct StateArgs {
    /// State subcommand
    #[command(subcommand)]
    pub command: StateCommand,
}

/// State subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum StateCommand {
    /// List available states
    List {
        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,

        /// Output format (table, json)
        #[arg(long, default_value = "table")]
        format: String,
    },

    /// Show details of a state
    Show {
        /// State name/key
        name: String,

        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,
    },

    /// Pull remote state to local
    Pull {
        /// Remote backend URL
        #[arg(long)]
        backend: String,

        /// Local output path
        #[arg(long, default_value = ".rustible/state")]
        output: PathBuf,
    },

    /// Push local state to remote
    Push {
        /// Local state directory
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,

        /// Remote backend URL
        #[arg(long)]
        backend: String,
    },

    /// Remove a state entry
    #[command(name = "rm")]
    Remove {
        /// State name/key to remove
        name: String,

        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,

        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },

    /// Manage state locks
    Lock {
        /// Lock subcommand
        #[command(subcommand)]
        command: LockCommand,
    },
}

/// Lock management subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum LockCommand {
    /// List active locks
    List {
        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,
    },

    /// Force-release a lock
    Release {
        /// Lock ID to release
        lock_id: String,

        /// State directory path
        #[arg(long, default_value = ".rustible/state")]
        state_dir: PathBuf,
    },
}

impl StateArgs {
    /// Execute the state command
    pub async fn execute(&self, ctx: &mut CommandContext) -> Result<i32> {
        match &self.command {
            StateCommand::List { state_dir, format } => {
                ctx.output.banner("STATE LIST");
                let state_path = state_dir
                    .canonicalize()
                    .unwrap_or_else(|_| state_dir.clone());

                if !state_path.exists() {
                    ctx.output
                        .info("No state directory found. Run a playbook first to generate state.");
                    return Ok(0);
                }

                let mut count = 0;
                if let Ok(entries) = std::fs::read_dir(&state_path) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(false, |ext| ext == "json") {
                            let name = path.file_stem().unwrap_or_default().to_string_lossy();
                            let metadata = std::fs::metadata(&path).ok();
                            let size = metadata.as_ref().map_or(0, |m| m.len());
                            let modified = metadata
                                .and_then(|m| m.modified().ok())
                                .map(|t| {
                                    let duration = t.elapsed().unwrap_or_default();
                                    if duration.as_secs() < 60 {
                                        "just now".to_string()
                                    } else if duration.as_secs() < 3600 {
                                        format!("{}m ago", duration.as_secs() / 60)
                                    } else if duration.as_secs() < 86400 {
                                        format!("{}h ago", duration.as_secs() / 3600)
                                    } else {
                                        format!("{}d ago", duration.as_secs() / 86400)
                                    }
                                })
                                .unwrap_or_else(|| "unknown".to_string());

                            if format == "json" {
                                println!(
                                    r#"{{"name":"{}","size":{},"modified":"{}"}}"#,
                                    name, size, modified
                                );
                            } else {
                                println!("  {} ({} bytes, {})", name, size, modified);
                            }
                            count += 1;
                        }
                    }
                }

                if count == 0 {
                    ctx.output.info("No state files found.");
                } else {
                    ctx.output.info(&format!("{} state file(s) found.", count));
                }
                Ok(0)
            }

            StateCommand::Show { name, state_dir } => {
                ctx.output.banner("STATE SHOW");
                let state_file = state_dir.join(format!("{}.json", name));

                if !state_file.exists() {
                    ctx.output
                        .error(&format!("State '{}' not found at {:?}", name, state_file));
                    return Ok(1);
                }

                let content = std::fs::read_to_string(&state_file)?;
                // Pretty-print JSON
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                    println!("{}", serde_json::to_string_pretty(&value)?);
                } else {
                    println!("{}", content);
                }
                Ok(0)
            }

            StateCommand::Pull { backend, output } => {
                ctx.output.banner("STATE PULL");
                ctx.output.info(&format!("Pulling state from: {}", backend));
                ctx.output.info(&format!("Output directory: {:?}", output));

                // For now, support file:// backends
                if backend.starts_with("file://") {
                    let source = backend.strip_prefix("file://").unwrap();
                    let source_path = std::path::Path::new(source);

                    if !source_path.exists() {
                        ctx.output
                            .error(&format!("Source path does not exist: {}", source));
                        return Ok(1);
                    }

                    std::fs::create_dir_all(output)?;

                    let mut count = 0;
                    if let Ok(entries) = std::fs::read_dir(source_path) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().map_or(false, |ext| ext == "json") {
                                let dest = output.join(entry.file_name());
                                std::fs::copy(&path, &dest)?;
                                count += 1;
                            }
                        }
                    }

                    ctx.output.info(&format!("Pulled {} state file(s).", count));
                } else {
                    ctx.output
                        .error("Unsupported backend. Supported: file://, s3://, http://");
                    return Ok(1);
                }

                Ok(0)
            }

            StateCommand::Push { state_dir, backend } => {
                ctx.output.banner("STATE PUSH");
                ctx.output
                    .info(&format!("Pushing state from: {:?}", state_dir));
                ctx.output.info(&format!("Target backend: {}", backend));

                if !state_dir.exists() {
                    ctx.output.error("State directory does not exist.");
                    return Ok(1);
                }

                if backend.starts_with("file://") {
                    let dest = backend.strip_prefix("file://").unwrap();
                    let dest_path = std::path::Path::new(dest);
                    std::fs::create_dir_all(dest_path)?;

                    let mut count = 0;
                    if let Ok(entries) = std::fs::read_dir(state_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().map_or(false, |ext| ext == "json") {
                                let target = dest_path.join(entry.file_name());
                                std::fs::copy(&path, &target)?;
                                count += 1;
                            }
                        }
                    }

                    ctx.output.info(&format!("Pushed {} state file(s).", count));
                } else {
                    ctx.output
                        .error("Unsupported backend. Supported: file://, s3://, http://");
                    return Ok(1);
                }

                Ok(0)
            }

            StateCommand::Remove {
                name,
                state_dir,
                force,
            } => {
                ctx.output.banner("STATE REMOVE");
                let state_file = state_dir.join(format!("{}.json", name));

                if !state_file.exists() {
                    ctx.output.error(&format!("State '{}' not found.", name));
                    return Ok(1);
                }

                if !force {
                    ctx.output.warning(&format!(
                        "This will permanently delete state '{}'. Use --force to confirm.",
                        name
                    ));
                    return Ok(1);
                }

                std::fs::remove_file(&state_file)?;
                ctx.output.info(&format!("State '{}' removed.", name));
                Ok(0)
            }

            StateCommand::Lock { command } => match command {
                LockCommand::List { state_dir } => {
                    ctx.output.banner("STATE LOCKS");
                    let lock_dir = state_dir.join("locks");

                    if !lock_dir.exists() {
                        ctx.output.info("No active locks.");
                        return Ok(0);
                    }

                    let mut count = 0;
                    if let Ok(entries) = std::fs::read_dir(&lock_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().map_or(false, |ext| ext == "lock") {
                                let name = path.file_stem().unwrap_or_default().to_string_lossy();
                                if let Ok(content) = std::fs::read_to_string(&path) {
                                    println!("  Lock: {} - {}", name, content.trim());
                                } else {
                                    println!("  Lock: {}", name);
                                }
                                count += 1;
                            }
                        }
                    }

                    if count == 0 {
                        ctx.output.info("No active locks.");
                    } else {
                        ctx.output.info(&format!("{} active lock(s).", count));
                    }
                    Ok(0)
                }

                LockCommand::Release { lock_id, state_dir } => {
                    ctx.output.banner("STATE LOCK RELEASE");
                    let lock_file = state_dir.join("locks").join(format!("{}.lock", lock_id));

                    if !lock_file.exists() {
                        ctx.output.error(&format!("Lock '{}' not found.", lock_id));
                        return Ok(1);
                    }

                    std::fs::remove_file(&lock_file)?;
                    ctx.output.info(&format!("Lock '{}' released.", lock_id));
                    Ok(0)
                }
            },
        }
    }
}
