//! Lock command for managing lockfiles
//!
//! This command creates and updates `rustible.lock` files for
//! reproducible playbook execution.

use clap::{Args, Subcommand};
use std::path::PathBuf;

use crate::lockfile::{DependencySource, LockedCollection, LockedRole, Lockfile, LockfileManager};

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

    /// Only check if lockfile is up to date (exit with error if not)
    #[arg(long)]
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

    /// Get the lockfile path
    fn get_lockfile_path(&self) -> PathBuf {
        self.lockfile
            .clone()
            .unwrap_or_else(|| Lockfile::default_path(&self.playbook))
    }

    /// Scan playbook for dependencies and add them to lockfile
    async fn scan_dependencies(&self, lockfile: &mut Lockfile) -> anyhow::Result<()> {
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
