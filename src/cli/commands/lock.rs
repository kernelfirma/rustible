//! Lock command for managing lockfiles
//!
//! This command creates and updates `rustible.lock` files for
//! reproducible playbook execution.
//!
//! Also provides checkpoint/rollback functionality for state management.

use super::CommandContext;
use anyhow::Context;
use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use rustible::inventory::{ConnectionType, Inventory};
use rustible::lockfile::Lockfile;
use rustible::modules::{ModuleContext, ModuleParams, ModuleRegistry};
use rustible::state::{
    PersistenceBackend, RollbackExecutor, RollbackPlan, StateConfig, StateManager, StateSnapshot,
    TaskStateRecord, TaskStatus,
};

/// A checkpoint representing a saved state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Checkpoint name
    pub name: String,
    /// Description of the checkpoint
    pub description: String,
    /// When the checkpoint was created
    pub created_at: DateTime<Utc>,
    /// Playbook associated with this checkpoint
    pub playbook: String,
    /// Snapshot ID capturing the baseline state at checkpoint creation time
    #[serde(default)]
    pub snapshot_id: Option<String>,
    /// Inventory path associated with this checkpoint, if any
    #[serde(default)]
    pub inventory_path: Option<String>,
    /// State files included in this checkpoint
    #[serde(default)]
    pub state_files: Vec<String>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Arguments for the lock command
#[derive(Args, Debug, Clone)]
pub struct LockArgs {
    #[command(subcommand)]
    pub subcommand: Option<LockSubcommand>,

    /// Path to the playbook file
    #[arg(default_value = "playbook.yml")]
    pub playbook: PathBuf,

    /// Custom lockfile path (default: rustible.lock in playbook directory)
    #[arg(long)]
    pub lockfile: Option<PathBuf>,

    /// Update existing lockfile with latest versions
    #[arg(long, short = 'u')]
    pub update: bool,

    /// Only verify if lockfile is up to date (exit with error if not)
    #[arg(long = "verify-only")]
    pub check: bool,
}

/// Lock subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum LockSubcommand {
    /// Show lockfile information
    Info,
    /// Verify lockfile integrity
    Verify,
    /// Remove lockfile
    Clean,
    /// Create a checkpoint before execution
    Checkpoint {
        /// Name for this checkpoint
        #[arg(short, long)]
        name: Option<String>,
        /// Description of the checkpoint
        #[arg(short = 'd', long)]
        description: Option<String>,
    },
    /// List available checkpoints
    ListCheckpoints,
    /// Rollback to a checkpoint
    Rollback {
        /// Checkpoint name or ID to rollback to
        checkpoint: String,
        /// Dry-run mode (show what would be rolled back)
        #[arg(long)]
        dry_run: bool,
    },
}

impl LockArgs {
    /// Execute the lock command
    pub async fn execute(&self) -> anyhow::Result<()> {
        self.execute_with_context(None).await
    }

    /// Execute the lock command with optional shared CLI context.
    pub async fn execute_with_context(&self, ctx: Option<&CommandContext>) -> anyhow::Result<()> {
        // Handle subcommands first
        if let Some(ref subcmd) = self.subcommand {
            return match subcmd {
                LockSubcommand::Info => self.show_info().await,
                LockSubcommand::Verify => self.verify_lockfile().await,
                LockSubcommand::Clean => self.clean_lockfile().await,
                LockSubcommand::Checkpoint { name, description } => {
                    self.create_checkpoint(ctx, name.clone(), description.clone())
                        .await
                }
                LockSubcommand::ListCheckpoints => self.list_checkpoints().await,
                LockSubcommand::Rollback {
                    checkpoint,
                    dry_run,
                } => self.rollback_to_checkpoint(ctx, checkpoint, *dry_run).await,
            };
        }

        // Default behavior: create or update lockfile
        if self.check {
            self.check_lockfile().await
        } else if self.update {
            self.update_lockfile().await
        } else {
            self.create_lockfile().await
        }
    }

    /// Create a new lockfile
    async fn create_lockfile(&self) -> anyhow::Result<()> {
        let lockfile_path = self.get_lockfile_path();

        if lockfile_path.exists() {
            println!(
                "Lockfile already exists at {}. Use --update to modify.",
                lockfile_path.display()
            );
            return Ok(());
        }

        println!("Creating lockfile for {}...", self.playbook.display());

        let mut lockfile = Lockfile::new(&self.playbook)?;

        // Scan playbook for dependencies
        self.scan_dependencies(&mut lockfile).await?;

        // Save lockfile
        lockfile.save(&lockfile_path)?;

        println!(
            "Created lockfile at {} with {} locked items.",
            lockfile_path.display(),
            lockfile.len()
        );

        Ok(())
    }

    /// Update existing lockfile
    async fn update_lockfile(&self) -> anyhow::Result<()> {
        let lockfile_path = self.get_lockfile_path();

        let mut lockfile = if lockfile_path.exists() {
            Lockfile::load(&lockfile_path)?
        } else {
            Lockfile::new(&self.playbook)?
        };

        println!("Updating lockfile for {}...", self.playbook.display());

        // Update playbook hash
        lockfile.update_playbook_hash(&self.playbook)?;

        // Re-scan dependencies
        self.scan_dependencies(&mut lockfile).await?;

        // Save lockfile
        lockfile.save(&lockfile_path)?;

        println!(
            "Updated lockfile at {} with {} locked items.",
            lockfile_path.display(),
            lockfile.len()
        );

        Ok(())
    }

    /// Check if lockfile is up to date
    async fn check_lockfile(&self) -> anyhow::Result<()> {
        let lockfile_path = self.get_lockfile_path();

        if !lockfile_path.exists() {
            anyhow::bail!(
                "Lockfile not found at {}. Run 'rustible lock' to create one.",
                lockfile_path.display()
            );
        }

        let lockfile = Lockfile::load(&lockfile_path)?;

        // Verify playbook hash
        lockfile.verify_playbook(&self.playbook)?;

        // Verify integrity
        lockfile.verify_integrity()?;

        println!("Lockfile is up to date.");
        Ok(())
    }

    /// Show lockfile information
    async fn show_info(&self) -> anyhow::Result<()> {
        let lockfile_path = self.get_lockfile_path();

        if !lockfile_path.exists() {
            println!("No lockfile found at {}", lockfile_path.display());
            return Ok(());
        }

        let lockfile = Lockfile::load(&lockfile_path)?;

        println!("Lockfile: {}", lockfile_path.display());
        println!("Version: {}", lockfile.version);
        println!("Created: {}", lockfile.created_at);
        println!("Updated: {}", lockfile.updated_at);
        println!("Playbook: {}", lockfile.playbook_path);
        println!();
        println!("Locked items:");
        println!("  Roles: {}", lockfile.roles.len());
        println!("  Collections: {}", lockfile.collections.len());
        println!("  Resources: {}", lockfile.resources.len());

        if !lockfile.roles.is_empty() {
            println!();
            println!("Roles:");
            for (name, role) in &lockfile.roles {
                println!("  {} @ {}", name, role.version);
            }
        }

        if !lockfile.collections.is_empty() {
            println!();
            println!("Collections:");
            for (name, coll) in &lockfile.collections {
                println!("  {} @ {}", name, coll.version);
            }
        }

        Ok(())
    }

    /// Verify lockfile integrity
    async fn verify_lockfile(&self) -> anyhow::Result<()> {
        let lockfile_path = self.get_lockfile_path();

        if !lockfile_path.exists() {
            anyhow::bail!("No lockfile found at {}", lockfile_path.display());
        }

        let lockfile = Lockfile::load(&lockfile_path)?;

        // Verify playbook hash
        match lockfile.verify_playbook(&self.playbook) {
            Ok(()) => println!("✓ Playbook hash matches"),
            Err(e) => {
                println!("✗ Playbook hash mismatch: {}", e);
                anyhow::bail!("Lockfile verification failed");
            }
        }

        // Verify integrity
        match lockfile.verify_integrity() {
            Ok(()) => println!("✓ Integrity check passed"),
            Err(e) => {
                println!("✗ Integrity check failed: {}", e);
                anyhow::bail!("Lockfile verification failed");
            }
        }

        println!("\nLockfile verification successful.");
        Ok(())
    }

    /// Remove lockfile
    async fn clean_lockfile(&self) -> anyhow::Result<()> {
        let lockfile_path = self.get_lockfile_path();

        if lockfile_path.exists() {
            std::fs::remove_file(&lockfile_path)?;
            println!("Removed lockfile at {}", lockfile_path.display());
        } else {
            println!("No lockfile found at {}", lockfile_path.display());
        }

        Ok(())
    }

    /// Create a checkpoint
    async fn create_checkpoint(
        &self,
        ctx: Option<&CommandContext>,
        name: Option<String>,
        description: Option<String>,
    ) -> anyhow::Result<()> {
        let checkpoint_dir = self.get_checkpoint_dir();
        std::fs::create_dir_all(&checkpoint_dir)?;

        let checkpoint_name =
            name.unwrap_or_else(|| format!("checkpoint-{}", Utc::now().format("%Y%m%d-%H%M%S")));

        let checkpoint_path = checkpoint_dir.join(format!("{}.json", checkpoint_name));

        if checkpoint_path.exists() {
            anyhow::bail!("Checkpoint '{}' already exists", checkpoint_name);
        }

        let assets_dir = self.get_checkpoint_assets_dir(&checkpoint_name);
        std::fs::create_dir_all(&assets_dir)?;
        let created_at = Utc::now();
        let snapshot_id = self.capture_checkpoint_snapshot(&checkpoint_name, created_at)?;
        let state_files = self.backup_checkpoint_state_files(&checkpoint_name)?;

        let mut metadata = HashMap::new();
        metadata.insert(
            "assets_dir".to_string(),
            serde_json::json!(assets_dir.display().to_string()),
        );
        metadata.insert(
            "state_dir".to_string(),
            serde_json::json!(self.get_state_dir().display().to_string()),
        );

        // Create checkpoint metadata
        let checkpoint = Checkpoint {
            name: checkpoint_name.clone(),
            description: description.unwrap_or_default(),
            created_at,
            playbook: self.playbook.display().to_string(),
            snapshot_id: Some(snapshot_id.clone()),
            inventory_path: ctx
                .and_then(|command_ctx| command_ctx.inventory())
                .map(|path| path.display().to_string()),
            state_files,
            metadata,
        };

        // Save checkpoint
        let content = serde_json::to_string_pretty(&checkpoint)?;
        std::fs::write(&checkpoint_path, content)?;

        println!("Created checkpoint '{}'", checkpoint_name);
        println!("  Path: {}", checkpoint_path.display());
        println!("  Playbook: {}", self.playbook.display());
        println!("  Snapshot ID: {}", snapshot_id);

        Ok(())
    }

    /// List available checkpoints
    async fn list_checkpoints(&self) -> anyhow::Result<()> {
        let checkpoint_dir = self.get_checkpoint_dir();

        if !checkpoint_dir.exists() {
            println!("No checkpoints found.");
            return Ok(());
        }

        let entries = std::fs::read_dir(&checkpoint_dir)?;
        let mut checkpoints: Vec<Checkpoint> = Vec::new();

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(cp) = serde_json::from_str::<Checkpoint>(&content) {
                        checkpoints.push(cp);
                    }
                }
            }
        }

        if checkpoints.is_empty() {
            println!("No checkpoints found.");
            return Ok(());
        }

        // Sort by creation date (newest first)
        checkpoints.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        println!("Available checkpoints:");
        println!();
        for cp in checkpoints {
            println!(
                "  {} ({})",
                cp.name,
                cp.created_at.format("%Y-%m-%d %H:%M:%S")
            );
            if !cp.description.is_empty() {
                println!("    {}", cp.description);
            }
            println!("    Playbook: {}", cp.playbook);
        }

        Ok(())
    }

    /// Rollback to a checkpoint
    async fn rollback_to_checkpoint(
        &self,
        ctx: Option<&CommandContext>,
        checkpoint_name: &str,
        dry_run: bool,
    ) -> anyhow::Result<()> {
        let checkpoint = self.load_checkpoint(checkpoint_name)?;
        let snapshot_id = checkpoint.snapshot_id.clone().with_context(|| {
            format!(
                "Checkpoint '{}' predates snapshot-backed rollback support. Recreate the checkpoint and try again.",
                checkpoint.name
            )
        })?;

        let state_manager = self.state_manager()?;
        let target_snapshot = state_manager.load_snapshot(&snapshot_id).with_context(|| {
            format!(
                "Failed to load checkpoint snapshot '{}' for '{}'",
                snapshot_id, checkpoint.name
            )
        })?;

        let current_snapshot = state_manager
            .get_latest_snapshot(&checkpoint.playbook)?
            .unwrap_or_else(|| target_snapshot.clone());

        let changed_tasks = self.changed_tasks_since_checkpoint(&current_snapshot, &checkpoint);
        let rollback_executor = RollbackExecutor::new(self.state_config());
        let plan = rollback_executor
            .create_plan(&changed_tasks)
            .context("failed to generate rollback plan from tracked task state")?;

        if dry_run {
            println!("=== Rollback Dry Run ===");
            println!();
            println!("Would rollback to checkpoint: {}", checkpoint.name);
            println!(
                "  Created: {}",
                checkpoint.created_at.format("%Y-%m-%d %H:%M:%S")
            );
            println!("  Playbook: {}", checkpoint.playbook);
            if !checkpoint.description.is_empty() {
                println!("  Description: {}", checkpoint.description);
            }
            println!("  Snapshot ID: {}", snapshot_id);
            println!("  State files to restore: {}", checkpoint.state_files.len());
            println!();
            if plan.is_empty() {
                println!("No rollback actions were generated from the tracked state.");
            } else {
                println!("{}", rollback_executor.dry_run(&plan));
            }
            println!("Run without --dry-run to execute rollback.");
            return Ok(());
        }

        println!("Rolling back to checkpoint '{}'...", checkpoint.name);
        println!("  Playbook: {}", checkpoint.playbook);

        self.execute_rollback_plan(ctx, &checkpoint, &plan).await?;
        self.restore_checkpoint_state_files(&checkpoint)?;
        self.save_post_rollback_snapshot(&state_manager, &checkpoint, &target_snapshot)?;

        if plan.is_empty() {
            println!("  No rollback actions were required; restored checkpoint state files only.");
        } else {
            println!("  Executed {} rollback action(s).", plan.actions.len());
        }
        println!("Rollback complete.");

        Ok(())
    }

    /// Get the checkpoint directory
    fn get_checkpoint_dir(&self) -> PathBuf {
        self.playbook
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join(".rustible")
            .join("checkpoints")
    }

    /// Get the assets directory for a checkpoint.
    fn get_checkpoint_assets_dir(&self, checkpoint_name: &str) -> PathBuf {
        self.get_checkpoint_dir().join(checkpoint_name)
    }

    /// Load a checkpoint file from disk.
    fn load_checkpoint(&self, checkpoint_name: &str) -> anyhow::Result<Checkpoint> {
        let checkpoint_path = self
            .get_checkpoint_dir()
            .join(format!("{}.json", checkpoint_name));

        if !checkpoint_path.exists() {
            anyhow::bail!(
                "Checkpoint '{}' not found. Use 'rustible lock list-checkpoints' to see available checkpoints.",
                checkpoint_name
            );
        }

        let content = std::fs::read_to_string(&checkpoint_path)?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Get the state storage directory used for checkpoint-backed snapshots.
    fn get_state_dir(&self) -> PathBuf {
        self.playbook
            .parent()
            .unwrap_or(Path::new("."))
            .join(".rustible")
            .join("state")
    }

    fn state_config(&self) -> StateConfig {
        let state_dir = self.get_state_dir();
        StateConfig::builder()
            .persistence(PersistenceBackend::Json(state_dir.clone()))
            .state_dir(state_dir)
            .enable_rollback(true)
            .build()
    }

    fn state_manager(&self) -> anyhow::Result<StateManager> {
        Ok(StateManager::new(self.state_config())?)
    }

    fn capture_checkpoint_snapshot(
        &self,
        checkpoint_name: &str,
        created_at: DateTime<Utc>,
    ) -> anyhow::Result<String> {
        let state_manager = self.state_manager()?;
        let playbook = self.playbook.display().to_string();
        let latest = state_manager.get_latest_snapshot(&playbook)?;

        let mut snapshot = if let Some(existing) = latest {
            let parent_id = existing.id.clone();
            let mut cloned = existing;
            cloned.parent_id = Some(parent_id);
            cloned
        } else {
            StateSnapshot::new(format!("checkpoint:{}", checkpoint_name), playbook.clone())
        };

        snapshot.id = Uuid::new_v4().to_string();
        snapshot.session_id = format!("checkpoint:{}", checkpoint_name);
        snapshot.playbook = playbook;
        snapshot.created_at = created_at;
        snapshot.description = Some(format!("Checkpoint '{}'", checkpoint_name));
        snapshot.metadata.insert(
            "checkpoint_name".to_string(),
            serde_json::json!(checkpoint_name),
        );
        snapshot
            .metadata
            .insert("checkpoint_kind".to_string(), serde_json::json!("lock"));
        snapshot.calculate_stats();
        state_manager.save_snapshot(&snapshot)?;

        Ok(snapshot.id.clone())
    }

    fn backup_checkpoint_state_files(&self, checkpoint_name: &str) -> anyhow::Result<Vec<String>> {
        let assets_dir = self.get_checkpoint_assets_dir(checkpoint_name);
        std::fs::create_dir_all(&assets_dir)?;

        let mut state_files = Vec::new();
        let lockfile_path = self.get_lockfile_path();
        if lockfile_path.exists() {
            let backup_path = assets_dir.join("0.bak");
            std::fs::copy(&lockfile_path, &backup_path).with_context(|| {
                format!(
                    "Failed to back up '{}' into checkpoint assets",
                    lockfile_path.display()
                )
            })?;
            state_files.push(lockfile_path.display().to_string());
        }

        Ok(state_files)
    }

    fn restore_checkpoint_state_files(&self, checkpoint: &Checkpoint) -> anyhow::Result<()> {
        let assets_dir = self.get_checkpoint_assets_dir(&checkpoint.name);
        for (idx, state_file) in checkpoint.state_files.iter().enumerate() {
            let backup_path = assets_dir.join(format!("{}.bak", idx));
            if !backup_path.exists() {
                continue;
            }

            let original_path = PathBuf::from(state_file);
            if let Some(parent) = original_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            std::fs::copy(&backup_path, &original_path).with_context(|| {
                format!(
                    "Failed to restore state file '{}' from checkpoint '{}'",
                    original_path.display(),
                    checkpoint.name
                )
            })?;
            println!("  Restored: {}", original_path.display());
        }

        Ok(())
    }

    fn changed_tasks_since_checkpoint(
        &self,
        current_snapshot: &StateSnapshot,
        checkpoint: &Checkpoint,
    ) -> Vec<TaskStateRecord> {
        current_snapshot
            .tasks
            .iter()
            .filter(|task| {
                task.status == TaskStatus::Changed
                    && task.rollback_available
                    && task.completed_at.unwrap_or(task.started_at) > checkpoint.created_at
            })
            .cloned()
            .collect()
    }

    fn load_inventory_for_checkpoint(
        &self,
        ctx: Option<&CommandContext>,
        checkpoint: &Checkpoint,
    ) -> anyhow::Result<Option<Inventory>> {
        let inventory_path = ctx
            .and_then(|command_ctx| command_ctx.inventory().cloned())
            .or_else(|| checkpoint.inventory_path.as_ref().map(PathBuf::from));

        let Some(inventory_path) = inventory_path else {
            return Ok(None);
        };

        if !inventory_path.exists() {
            anyhow::bail!(
                "Inventory '{}' referenced by checkpoint '{}' does not exist",
                inventory_path.display(),
                checkpoint.name
            );
        }

        Ok(Some(Inventory::load(&inventory_path)?))
    }

    async fn execute_rollback_plan(
        &self,
        ctx: Option<&CommandContext>,
        checkpoint: &Checkpoint,
        plan: &RollbackPlan,
    ) -> anyhow::Result<()> {
        let inventory = self.load_inventory_for_checkpoint(ctx, checkpoint)?;
        let registry = ModuleRegistry::with_builtins();

        for action in &plan.actions {
            let params: ModuleParams = match &action.args {
                serde_json::Value::Object(map) => {
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                }
                _ => anyhow::bail!(
                    "Rollback action '{}' has non-object module parameters",
                    action.description
                ),
            };

            let mut module_context = ModuleContext::default();
            if let Some(connection) = self
                .rollback_connection_for_host(ctx, inventory.as_ref(), &action.host)
                .await?
            {
                module_context = module_context.with_connection(connection);
            }

            let output = registry
                .execute(&action.module, &params, &module_context)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "Rollback action '{}' failed on host '{}': {}",
                        action.description,
                        action.host,
                        error
                    )
                })?;

            println!("  [{}] {}", action.host, output.msg);
        }

        if let Some(command_ctx) = ctx {
            command_ctx.close_connections().await;
        }

        Ok(())
    }

    async fn rollback_connection_for_host(
        &self,
        ctx: Option<&CommandContext>,
        inventory: Option<&Inventory>,
        host_name: &str,
    ) -> anyhow::Result<Option<std::sync::Arc<dyn rustible::connection::Connection + Send + Sync>>>
    {
        if Self::is_local_host(host_name) {
            return Ok(None);
        }

        let inventory = inventory.with_context(|| {
            format!(
                "Rollback action targets host '{}' but no inventory was supplied",
                host_name
            )
        })?;
        let host = inventory.get_host(host_name).with_context(|| {
            format!(
                "Rollback action targets host '{}' which is missing from the active inventory",
                host_name
            )
        })?;

        match host.connection.connection {
            ConnectionType::Local => Ok(None),
            ConnectionType::Ssh => {
                let ctx = ctx.with_context(|| {
                    format!(
                        "Rollback of remote SSH host '{}' requires CLI command context",
                        host_name
                    )
                })?;
                let ansible_user = host.connection.ssh.user.clone().unwrap_or_else(|| {
                    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
                });

                let conn = ctx
                    .get_connection(
                        host_name,
                        host.address(),
                        &ansible_user,
                        host.connection.ssh.port,
                        host.connection.ssh.private_key_file.as_deref(),
                    )
                    .await?;
                Ok(Some(conn))
            }
            ConnectionType::Winrm => self.build_winrm_connection(host).await.map(Some),
            ConnectionType::Docker | ConnectionType::Podman => anyhow::bail!(
                "Rollback does not yet support {} transport for host '{}'",
                host.connection.connection,
                host_name
            ),
        }
    }

    #[cfg(feature = "winrm")]
    async fn build_winrm_connection(
        &self,
        host: &rustible::inventory::Host,
    ) -> anyhow::Result<std::sync::Arc<dyn rustible::connection::Connection + Send + Sync>> {
        use rustible::connection::winrm::{WinRmAuth, WinRmConnectionBuilder};

        let username = host
            .connection
            .ssh
            .user
            .clone()
            .or_else(|| Self::host_var_string(host, "ansible_user"))
            .unwrap_or_else(|| "Administrator".to_string());
        let password = Self::host_var_string(host, "ansible_password")
            .or_else(|| Self::host_var_string(host, "ansible_pass"))
            .or_else(|| std::env::var("RUSTIBLE_WINRM_PASS").ok())
            .with_context(|| {
                format!(
                    "WinRM rollback for host '{}' requires ansible_password/ansible_pass or RUSTIBLE_WINRM_PASS",
                    host.name
                )
            })?;
        let port = if host.connection.ssh.port == 22 {
            5985
        } else {
            host.connection.ssh.port
        };
        let use_ssl = Self::host_var_string(host, "ansible_winrm_scheme")
            .map(|scheme| scheme.eq_ignore_ascii_case("https"))
            .or_else(|| Self::host_var_bool(host, "ansible_winrm_use_ssl"))
            .unwrap_or(port == 5986);
        let verify_ssl = !matches!(
            Self::host_var_string(host, "ansible_winrm_server_cert_validation").as_deref(),
            Some("ignore")
        );
        let transport = Self::host_var_string(host, "ansible_winrm_transport")
            .unwrap_or_else(|| "ntlm".to_string());
        let auth = match transport.to_lowercase().as_str() {
            "basic" => WinRmAuth::basic(&username, &password),
            "ntlm" | "negotiate" => WinRmAuth::ntlm(&username, &password),
            other => anyhow::bail!(
                "Unsupported WinRM transport '{}' for rollback host '{}'",
                other,
                host.name
            ),
        };

        let connection = WinRmConnectionBuilder::new(host.address())
            .port(port)
            .use_ssl(use_ssl)
            .verify_ssl(verify_ssl)
            .auth(auth)
            .connect()
            .await
            .with_context(|| format!("Failed to connect to '{}' via WinRM", host.name))?;

        Ok(std::sync::Arc::new(connection))
    }

    #[cfg(not(feature = "winrm"))]
    async fn build_winrm_connection(
        &self,
        host: &rustible::inventory::Host,
    ) -> anyhow::Result<std::sync::Arc<dyn rustible::connection::Connection + Send + Sync>> {
        anyhow::bail!(
            "Rollback for WinRM host '{}' requires building Rustible with the 'winrm' feature",
            host.name
        )
    }

    fn save_post_rollback_snapshot(
        &self,
        state_manager: &StateManager,
        checkpoint: &Checkpoint,
        target_snapshot: &StateSnapshot,
    ) -> anyhow::Result<()> {
        let original_snapshot_id = target_snapshot.id.clone();
        let mut snapshot = target_snapshot.clone();
        snapshot.id = Uuid::new_v4().to_string();
        snapshot.session_id = format!("rollback:{}", checkpoint.name);
        snapshot.created_at = Utc::now();
        snapshot.description = Some(format!("Rolled back to checkpoint '{}'", checkpoint.name));
        snapshot.parent_id = Some(original_snapshot_id);
        snapshot.metadata.insert(
            "restored_from_checkpoint".to_string(),
            serde_json::json!(checkpoint.name),
        );
        if let Some(snapshot_id) = &checkpoint.snapshot_id {
            snapshot.metadata.insert(
                "checkpoint_snapshot_id".to_string(),
                serde_json::json!(snapshot_id),
            );
        }
        snapshot.calculate_stats();
        state_manager.save_snapshot(&snapshot)?;
        Ok(())
    }

    fn is_local_host(host: &str) -> bool {
        matches!(host, "localhost" | "127.0.0.1" | "::1")
    }

    fn host_var_string(host: &rustible::inventory::Host, key: &str) -> Option<String> {
        host.get_var(key).and_then(|value| match value {
            serde_yaml::Value::String(v) => Some(v.clone()),
            serde_yaml::Value::Number(v) => Some(v.to_string()),
            serde_yaml::Value::Bool(v) => Some(v.to_string()),
            _ => None,
        })
    }

    fn host_var_bool(host: &rustible::inventory::Host, key: &str) -> Option<bool> {
        host.get_var(key).and_then(|value| match value {
            serde_yaml::Value::Bool(v) => Some(*v),
            serde_yaml::Value::String(v) => match v.to_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            },
            _ => None,
        })
    }

    /// Get the lockfile path
    fn get_lockfile_path(&self) -> PathBuf {
        self.lockfile
            .clone()
            .unwrap_or_else(|| Lockfile::default_path(&self.playbook))
    }

    /// Scan playbook for dependencies and add them to lockfile
    async fn scan_dependencies(&self, _lockfile: &mut Lockfile) -> anyhow::Result<()> {
        // In a real implementation, this would:
        // 1. Parse the playbook
        // 2. Find role and collection references
        // 3. Query Galaxy for versions and checksums
        // 4. Add them to the lockfile

        // For now, we just log what would happen
        println!("  Scanning for roles and collections...");

        // Example: detect roles from requirements.yml if present
        let req_path = self
            .playbook
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("requirements.yml");
        if req_path.exists() {
            println!("  Found requirements.yml, scanning...");
            // Would parse requirements.yml and lock versions
        }

        // Example: detect roles from playbook
        // This is a placeholder - real implementation would parse YAML
        println!("  Detected 0 roles, 0 collections (stub implementation)");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rustible::state::{StateSnapshot, TaskStateRecord, TaskStatus};
    use tempfile::tempdir;

    fn create_playbook(path: &std::path::Path) {
        std::fs::write(
            path,
            r#"---
- name: Test playbook
  hosts: localhost
  tasks:
    - name: noop
      debug:
        msg: "ok"
"#,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_lock_create_update_verify_clean() {
        let temp = tempdir().unwrap();
        let playbook = temp.path().join("playbook.yml");
        create_playbook(&playbook);

        let create_args = LockArgs {
            subcommand: None,
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        create_args.execute().await.unwrap();
        let lockfile_path = Lockfile::default_path(&playbook);
        assert!(lockfile_path.exists());

        let check_args = LockArgs {
            subcommand: None,
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: true,
        };
        check_args.execute().await.unwrap();

        let update_args = LockArgs {
            subcommand: None,
            playbook: playbook.clone(),
            lockfile: None,
            update: true,
            check: false,
        };
        update_args.execute().await.unwrap();

        let info_args = LockArgs {
            subcommand: Some(LockSubcommand::Info),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        info_args.execute().await.unwrap();

        let verify_args = LockArgs {
            subcommand: Some(LockSubcommand::Verify),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        verify_args.execute().await.unwrap();

        let clean_args = LockArgs {
            subcommand: Some(LockSubcommand::Clean),
            playbook,
            lockfile: None,
            update: false,
            check: false,
        };
        clean_args.execute().await.unwrap();
        assert!(!lockfile_path.exists());
    }

    #[tokio::test]
    async fn test_lock_check_missing_lockfile_errors() {
        let temp = tempdir().unwrap();
        let playbook = temp.path().join("playbook.yml");
        create_playbook(&playbook);

        let args = LockArgs {
            subcommand: None,
            playbook,
            lockfile: None,
            update: false,
            check: true,
        };

        let result = args.execute().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_checkpoint_create_list() {
        let temp = tempdir().unwrap();
        let playbook = temp.path().join("playbook.yml");
        create_playbook(&playbook);

        // Create a checkpoint
        let checkpoint_args = LockArgs {
            subcommand: Some(LockSubcommand::Checkpoint {
                name: Some("test-checkpoint".to_string()),
                description: Some("Test description".to_string()),
            }),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        checkpoint_args.execute().await.unwrap();

        // Verify checkpoint was created
        let checkpoint_dir = temp.path().join(".rustible").join("checkpoints");
        let checkpoint_path = checkpoint_dir.join("test-checkpoint.json");
        assert!(checkpoint_path.exists());
        let checkpoint: Checkpoint =
            serde_json::from_str(&std::fs::read_to_string(&checkpoint_path).unwrap()).unwrap();
        assert!(checkpoint.snapshot_id.is_some());

        // List checkpoints
        let list_args = LockArgs {
            subcommand: Some(LockSubcommand::ListCheckpoints),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        list_args.execute().await.unwrap();
    }

    #[tokio::test]
    async fn test_checkpoint_rollback_dry_run() {
        let temp = tempdir().unwrap();
        let playbook = temp.path().join("playbook.yml");
        create_playbook(&playbook);

        // Create a checkpoint first
        let checkpoint_args = LockArgs {
            subcommand: Some(LockSubcommand::Checkpoint {
                name: Some("rollback-test".to_string()),
                description: None,
            }),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        checkpoint_args.execute().await.unwrap();

        // Rollback in dry-run mode
        let rollback_args = LockArgs {
            subcommand: Some(LockSubcommand::Rollback {
                checkpoint: "rollback-test".to_string(),
                dry_run: true,
            }),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        rollback_args.execute().await.unwrap();
    }

    #[tokio::test]
    async fn test_checkpoint_rollback_nonexistent_fails() {
        let temp = tempdir().unwrap();
        let playbook = temp.path().join("playbook.yml");
        create_playbook(&playbook);

        // Try to rollback to non-existent checkpoint
        let rollback_args = LockArgs {
            subcommand: Some(LockSubcommand::Rollback {
                checkpoint: "nonexistent".to_string(),
                dry_run: false,
            }),
            playbook,
            lockfile: None,
            update: false,
            check: false,
        };

        let result = rollback_args.execute().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_checkpoint_rollback_executes_local_file_rollback() {
        let temp = tempdir().unwrap();
        let playbook = temp.path().join("playbook.yml");
        create_playbook(&playbook);

        let checkpoint_args = LockArgs {
            subcommand: Some(LockSubcommand::Checkpoint {
                name: Some("live-rollback".to_string()),
                description: Some("baseline".to_string()),
            }),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        checkpoint_args.execute().await.unwrap();

        let checkpoint = checkpoint_args.load_checkpoint("live-rollback").unwrap();
        let mut changed_snapshot =
            StateSnapshot::new("session-after", playbook.display().to_string());
        changed_snapshot.description = Some("Changed after checkpoint".to_string());

        let target_file = temp.path().join("generated.txt");
        std::fs::write(&target_file, "created after checkpoint").unwrap();

        let change_time = checkpoint.created_at + Duration::seconds(1);
        let mut task = TaskStateRecord::new("create-generated", "localhost", "file")
            .with_name("Create generated file")
            .with_args(serde_json::json!({
                "path": target_file.display().to_string(),
                "state": "file"
            }));
        task.started_at = change_time;
        task.completed_at = Some(change_time);
        task.status = TaskStatus::Changed;
        task.rollback_available = true;
        task.before_state = Some(serde_json::json!({
            "exists": false
        }));

        changed_snapshot.tasks.push(task);
        changed_snapshot.calculate_stats();

        let state_manager = checkpoint_args.state_manager().unwrap();
        state_manager.save_snapshot(&changed_snapshot).unwrap();

        let rollback_args = LockArgs {
            subcommand: Some(LockSubcommand::Rollback {
                checkpoint: "live-rollback".to_string(),
                dry_run: false,
            }),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };
        rollback_args.execute().await.unwrap();

        assert!(!target_file.exists());

        let latest = rollback_args
            .state_manager()
            .unwrap()
            .get_latest_snapshot(&playbook.display().to_string())
            .unwrap()
            .unwrap();
        assert_eq!(
            latest.metadata.get("restored_from_checkpoint"),
            Some(&serde_json::json!("live-rollback"))
        );
    }

    #[tokio::test]
    async fn test_checkpoint_legacy_format_requires_recreation() {
        let temp = tempdir().unwrap();
        let playbook = temp.path().join("playbook.yml");
        create_playbook(&playbook);

        let args = LockArgs {
            subcommand: Some(LockSubcommand::Rollback {
                checkpoint: "legacy".to_string(),
                dry_run: false,
            }),
            playbook: playbook.clone(),
            lockfile: None,
            update: false,
            check: false,
        };

        let checkpoint_dir = args.get_checkpoint_dir();
        std::fs::create_dir_all(&checkpoint_dir).unwrap();
        let checkpoint = Checkpoint {
            name: "legacy".to_string(),
            description: String::new(),
            created_at: Utc::now(),
            playbook: playbook.display().to_string(),
            snapshot_id: None,
            inventory_path: None,
            state_files: Vec::new(),
            metadata: HashMap::new(),
        };
        std::fs::write(
            checkpoint_dir.join("legacy.json"),
            serde_json::to_string_pretty(&checkpoint).unwrap(),
        )
        .unwrap();

        let result = args.execute().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("predates snapshot-backed rollback support"));
    }
}
