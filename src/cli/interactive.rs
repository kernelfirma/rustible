//! Interactive mode module for Rustible
//!
//! Provides interactive prompts for common operations using dialoguer.

use anyhow::{Context, Result};
use colored::Colorize;
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, MultiSelect, Select};
use std::path::PathBuf;

/// Interactive session state
pub struct InteractiveSession {
    term: Term,
    theme: ColorfulTheme,
}

impl Default for InteractiveSession {
    fn default() -> Self {
        Self::new()
    }
}

impl InteractiveSession {
    /// Create a new interactive session
    pub fn new() -> Self {
        Self {
            term: Term::stderr(),
            theme: ColorfulTheme::default(),
        }
    }

    /// Clear the screen
    pub fn clear(&self) -> Result<()> {
        self.term.clear_screen()?;
        Ok(())
    }

    /// Display a welcome banner
    pub fn show_banner(&self) {
        println!();
        println!(
            "{}",
            "  ╔═══════════════════════════════════════════════════════╗"
                .bright_blue()
                .bold()
        );
        println!(
            "{}",
            "  ║         RUSTIBLE - Interactive Mode                   ║"
                .bright_blue()
                .bold()
        );
        println!(
            "{}",
            "  ║      An Ansible substitute written in Rust            ║"
                .bright_blue()
                .bold()
        );
        println!(
            "{}",
            "  ╚═══════════════════════════════════════════════════════╝"
                .bright_blue()
                .bold()
        );
        println!();
    }

    /// Prompt for main menu action
    pub fn main_menu(&self) -> Result<MainMenuAction> {
        let items = vec![
            "Run a playbook",
            "Check playbook (dry-run)",
            "List hosts",
            "List tasks",
            "Vault operations",
            "Initialize project",
            "Validate playbook",
            "Settings",
            "Exit",
        ];

        let selection = Select::with_theme(&self.theme)
            .with_prompt("What would you like to do?")
            .items(&items)
            .default(0)
            .interact_on(&self.term)?;

        Ok(match selection {
            0 => MainMenuAction::RunPlaybook,
            1 => MainMenuAction::CheckPlaybook,
            2 => MainMenuAction::ListHosts,
            3 => MainMenuAction::ListTasks,
            4 => MainMenuAction::Vault,
            5 => MainMenuAction::Init,
            6 => MainMenuAction::Validate,
            7 => MainMenuAction::Settings,
            _ => MainMenuAction::Exit,
        })
    }

    /// Prompt for playbook selection
    pub fn select_playbook(&self, playbooks: &[PathBuf]) -> Result<Option<PathBuf>> {
        if playbooks.is_empty() {
            println!("{}", "No playbooks found in current directory.".yellow());
            return Ok(None);
        }

        let items: Vec<String> = playbooks.iter().map(|p| p.display().to_string()).collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select a playbook")
            .items(&items)
            .default(0)
            .interact_on(&self.term)?;

        Ok(Some(playbooks[selection].clone()))
    }

    /// Prompt for inventory selection
    pub fn select_inventory(&self, inventories: &[PathBuf]) -> Result<Option<PathBuf>> {
        if inventories.is_empty() {
            let custom: String = Input::with_theme(&self.theme)
                .with_prompt("Enter inventory path (or 'localhost' for local)")
                .default("localhost".to_string())
                .interact_on(&self.term)?;

            if custom == "localhost" {
                return Ok(None);
            }
            return Ok(Some(PathBuf::from(custom)));
        }

        let mut items: Vec<String> = inventories
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        items.push("Enter custom path...".to_string());
        items.push("Use localhost (no inventory)".to_string());

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select inventory")
            .items(&items)
            .default(0)
            .interact_on(&self.term)?;

        if selection == items.len() - 1 {
            return Ok(None); // localhost
        }

        if selection == items.len() - 2 {
            let custom: String = Input::with_theme(&self.theme)
                .with_prompt("Enter inventory path")
                .interact_on(&self.term)?;
            return Ok(Some(PathBuf::from(custom)));
        }

        Ok(Some(inventories[selection].clone()))
    }

    /// Prompt for tags selection
    pub fn select_tags(&self, available_tags: &[String]) -> Result<Vec<String>> {
        if available_tags.is_empty() {
            return Ok(vec![]);
        }

        let use_tags = Confirm::with_theme(&self.theme)
            .with_prompt("Filter by tags?")
            .default(false)
            .interact_on(&self.term)?;

        if !use_tags {
            return Ok(vec![]);
        }

        let selections = MultiSelect::with_theme(&self.theme)
            .with_prompt("Select tags to run (space to select, enter to confirm)")
            .items(available_tags)
            .interact_on(&self.term)?;

        Ok(selections
            .iter()
            .map(|&i| available_tags[i].clone())
            .collect())
    }

    /// Prompt for extra variables
    pub fn get_extra_vars(&self) -> Result<Vec<String>> {
        let add_vars = Confirm::with_theme(&self.theme)
            .with_prompt("Add extra variables?")
            .default(false)
            .interact_on(&self.term)?;

        if !add_vars {
            return Ok(vec![]);
        }

        let mut vars = Vec::new();

        loop {
            let var: String = Input::with_theme(&self.theme)
                .with_prompt("Enter variable (key=value, or empty to finish)")
                .allow_empty(true)
                .interact_on(&self.term)?;

            if var.is_empty() {
                break;
            }

            if var.contains('=') || var.starts_with('@') {
                vars.push(var);
            } else {
                println!("{}", "Invalid format. Use key=value or @file.yml".yellow());
            }
        }

        Ok(vars)
    }

    /// Prompt for run options
    pub fn get_run_options(&self) -> Result<RunOptions> {
        let check_mode = Confirm::with_theme(&self.theme)
            .with_prompt("Run in check mode (dry-run)?")
            .default(false)
            .interact_on(&self.term)?;

        let diff_mode = Confirm::with_theme(&self.theme)
            .with_prompt("Show diffs?")
            .default(false)
            .interact_on(&self.term)?;

        let verbosity_items = vec![
            "Normal (no extra verbosity)",
            "Verbose (-v)",
            "More verbose (-vv)",
            "Debug (-vvv)",
            "Connection debug (-vvvv)",
        ];

        let verbosity = Select::with_theme(&self.theme)
            .with_prompt("Verbosity level")
            .items(&verbosity_items)
            .default(0)
            .interact_on(&self.term)? as u8;

        let limit: String = Input::with_theme(&self.theme)
            .with_prompt("Limit to hosts (pattern, or empty for all)")
            .allow_empty(true)
            .interact_on(&self.term)?;

        Ok(RunOptions {
            check_mode,
            diff_mode,
            verbosity,
            limit: if limit.is_empty() { None } else { Some(limit) },
        })
    }

    /// Prompt for vault action
    pub fn vault_menu(&self) -> Result<VaultAction> {
        let items = vec![
            "Encrypt a file",
            "Decrypt a file",
            "View encrypted file",
            "Edit encrypted file",
            "Create new encrypted file",
            "Rekey (change password)",
            "Encrypt a string",
            "Back to main menu",
        ];

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Vault operation")
            .items(&items)
            .default(0)
            .interact_on(&self.term)?;

        Ok(match selection {
            0 => VaultAction::Encrypt,
            1 => VaultAction::Decrypt,
            2 => VaultAction::View,
            3 => VaultAction::Edit,
            4 => VaultAction::Create,
            5 => VaultAction::Rekey,
            6 => VaultAction::EncryptString,
            _ => VaultAction::Back,
        })
    }

    /// Prompt for file path
    pub fn get_file_path(&self, prompt: &str) -> Result<PathBuf> {
        let path: String = Input::with_theme(&self.theme)
            .with_prompt(prompt)
            .interact_on(&self.term)?;
        Ok(PathBuf::from(path))
    }

    /// Prompt for confirmation
    pub fn confirm(&self, message: &str) -> Result<bool> {
        Ok(Confirm::with_theme(&self.theme)
            .with_prompt(message)
            .default(false)
            .interact_on(&self.term)?)
    }

    /// Show a success message
    pub fn success(&self, message: &str) {
        println!("{} {}", "[OK]".green().bold(), message);
    }

    /// Show an error message
    pub fn error(&self, message: &str) {
        println!("{} {}", "[ERROR]".red().bold(), message);
    }

    /// Show a warning message
    pub fn warning(&self, message: &str) {
        println!("{} {}", "[WARN]".yellow().bold(), message);
    }

    /// Show an info message
    pub fn info(&self, message: &str) {
        println!("{} {}", "[INFO]".blue().bold(), message);
    }

    /// Wait for user to press enter
    pub fn wait_for_enter(&self) -> Result<()> {
        println!();
        println!("{}", "Press Enter to continue...".dimmed());
        self.term.read_line()?;
        Ok(())
    }
}

/// Main menu actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuAction {
    RunPlaybook,
    CheckPlaybook,
    ListHosts,
    ListTasks,
    Vault,
    Init,
    Validate,
    Settings,
    Exit,
}

/// Vault actions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultAction {
    Encrypt,
    Decrypt,
    View,
    Edit,
    Create,
    Rekey,
    EncryptString,
    Back,
}

/// Run options from interactive prompts
#[derive(Debug, Clone)]
pub struct RunOptions {
    pub check_mode: bool,
    pub diff_mode: bool,
    pub verbosity: u8,
    pub limit: Option<String>,
}

/// Find playbooks in the current directory and common locations
pub fn find_playbooks() -> Vec<PathBuf> {
    let mut playbooks = Vec::new();

    // Check current directory
    if let Ok(entries) = std::fs::read_dir(".") {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_playbook(&path) {
                playbooks.push(path);
            }
        }
    }

    // Check playbooks directory
    if let Ok(entries) = std::fs::read_dir("playbooks") {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_playbook(&path) {
                playbooks.push(path);
            }
        }
    }

    playbooks.sort();
    playbooks
}

/// Find inventory files
pub fn find_inventories() -> Vec<PathBuf> {
    let mut inventories = Vec::new();

    // Check common locations
    let locations = [
        "inventory",
        "inventory/hosts.yml",
        "inventory/hosts.yaml",
        "hosts",
        "hosts.yml",
        "hosts.yaml",
        "inventory.yml",
        "inventory.yaml",
    ];

    for loc in &locations {
        let path = PathBuf::from(loc);
        if path.exists() {
            inventories.push(path);
        }
    }

    // Check inventory directory for files
    if let Ok(entries) = std::fs::read_dir("inventory") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if (ext == "yml" || ext == "yaml" || ext == "ini")
                        && !inventories.contains(&path)
                    {
                        inventories.push(path);
                    }
                }
            }
        }
    }

    inventories
}

/// Check if a file looks like a playbook
fn is_playbook(path: &PathBuf) -> bool {
    if !path.is_file() {
        return false;
    }

    // Check extension
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "yml" && ext != "yaml" {
        return false;
    }

    // Check filename patterns
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with('.') {
        return false;
    }

    // Try to read and check for playbook structure
    if let Ok(content) = std::fs::read_to_string(path) {
        // Basic check for playbook structure
        return content.contains("hosts:")
            || content.contains("- name:")
            || content.contains("tasks:");
    }

    false
}

/// Extract tags from a playbook file
pub fn extract_tags_from_playbook(playbook: &PathBuf) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(playbook)
        .with_context(|| format!("Failed to read playbook: {}", playbook.display()))?;

    let mut tags = std::collections::HashSet::new();

    // Simple regex-like search for tags
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("tags:") {
            // Parse inline tags
            let tag_part = line.strip_prefix("tags:").unwrap().trim();
            if tag_part.starts_with('[') && tag_part.ends_with(']') {
                // Inline list: tags: [tag1, tag2]
                let inner = &tag_part[1..tag_part.len() - 1];
                for tag in inner.split(',') {
                    let tag = tag.trim().trim_matches('"').trim_matches('\'');
                    if !tag.is_empty() {
                        tags.insert(tag.to_string());
                    }
                }
            } else if !tag_part.is_empty() && !tag_part.starts_with('-') {
                // Single tag: tags: mytag
                let tag = tag_part.trim_matches('"').trim_matches('\'');
                if !tag.is_empty() {
                    tags.insert(tag.to_string());
                }
            }
        } else if line.starts_with("- ") && line.contains("tags") {
            // List format under tags:
            // Try to extract tag name from lines like "- deploy" under tags:
            if let Some(tag) = line.strip_prefix("- ") {
                let tag = tag.trim().trim_matches('"').trim_matches('\'');
                if !tag.contains(':') && !tag.is_empty() {
                    // This might be a tag value
                    // We'll be conservative and only add it if it looks like a simple tag
                    if tag
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
                    {
                        tags.insert(tag.to_string());
                    }
                }
            }
        }
    }

    let mut result: Vec<String> = tags.into_iter().collect();
    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_options_default() {
        let opts = RunOptions {
            check_mode: false,
            diff_mode: false,
            verbosity: 0,
            limit: None,
        };
        assert!(!opts.check_mode);
        assert!(!opts.diff_mode);
    }

    #[test]
    fn test_is_playbook() {
        // These tests would require actual files, so we test the logic path
        let path = PathBuf::from("nonexistent.yml");
        assert!(!is_playbook(&path)); // File doesn't exist
    }
}
