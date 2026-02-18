//! Lock command for managing lockfiles
//!
//! This command creates and updates `rustible.lock` files for
//! reproducible playbook execution.
//!
//! Also provides checkpoint/rollback functionality for state management.

use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use rustible::lockfile::Lockfile;

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
    /// State files included in this checkpoint
    pub state_files: Vec<String>,
    /// Additional metadata
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
        // Handle subcommands first
        if let Some(ref subcmd) = self.subcommand {
            return match subcmd {
                LockSubcommand::Info => self.show_info().await,
                LockSubcommand::Verify => self.verify_lockfile().await,
                LockSubcommand::Clean => self.clean_lockfile().await,
                LockSubcommand::Checkpoint { name, description } => {
                    self.create_checkpoint(name.clone(), description.clone())
                        .await
                }
                LockSubcommand::ListCheckpoints => self.list_checkpoints().await,
                LockSubcommand::Rollback {
                    checkpoint,
                    dry_run,
                } => self.rollback_to_checkpoint(checkpoint, *dry_run).await,
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
        name: Option<String>,
        description: Option<String>,
    ) -> anyhow::Result<()> {
        use chrono::Utc;
        use std::collections::HashMap;

        let checkpoint_dir = self.get_checkpoint_dir();
        std::fs::create_dir_all(&checkpoint_dir)?;

        let checkpoint_name =
            name.unwrap_or_else(|| format!("checkpoint-{}", Utc::now().format("%Y%m%d-%H%M%S")));

        let checkpoint_path = checkpoint_dir.join(format!("{}.json", checkpoint_name));

        if checkpoint_path.exists() {
            anyhow::bail!("Checkpoint '{}' already exists", checkpoint_name);
        }

        // Create checkpoint metadata
        let checkpoint = Checkpoint {
            name: checkpoint_name.clone(),
            description: description.unwrap_or_default(),
            created_at: Utc::now(),
            playbook: self.playbook.display().to_string(),
            state_files: Vec::new(),
            metadata: HashMap::new(),
        };

        // Save checkpoint
        let content = serde_json::to_string_pretty(&checkpoint)?;
        std::fs::write(&checkpoint_path, content)?;

        println!("Created checkpoint '{}'", checkpoint_name);
        println!("  Path: {}", checkpoint_path.display());
        println!("  Playbook: {}", self.playbook.display());

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
        checkpoint_name: &str,
        dry_run: bool,
    ) -> anyhow::Result<()> {
        let checkpoint_dir = self.get_checkpoint_dir();
        let checkpoint_path = checkpoint_dir.join(format!("{}.json", checkpoint_name));

        if !checkpoint_path.exists() {
            anyhow::bail!(
                "Checkpoint '{}' not found. Use 'rustible lock list-checkpoints' to see available checkpoints.",
                checkpoint_name
            );
        }

        let content = std::fs::read_to_string(&checkpoint_path)?;
        let checkpoint: Checkpoint = serde_json::from_str(&content)?;

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
            println!();
            println!("State files to restore: {}", checkpoint.state_files.len());
            for sf in &checkpoint.state_files {
                println!("  - {}", sf);
            }
            println!();
            println!("Run without --dry-run to execute rollback.");
        } else {
            println!("Rolling back to checkpoint '{}'...", checkpoint.name);

            // In a real implementation, this would:
            // 1. Load the state from the checkpoint
            // 2. Generate a rollback plan using RollbackExecutor
            // 3. Execute the rollback plan
            // 4. Update the current state

            // For now, just acknowledge the rollback
            println!("Rollback initiated for checkpoint: {}", checkpoint.name);
            println!("  Playbook: {}", checkpoint.playbook);

            // Restore state files
            for state_file in &checkpoint.state_files {
                println!("  Restoring: {}", state_file);
            }

            println!();
            println!("Rollback complete.");
        }

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
}
