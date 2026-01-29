//! Plan output formatter with per-host diff summary
//!
//! This module provides structured plan output formatting for the `--plan` mode,
//! showing what changes would be made to each host with detailed diffs where available.
//!
//! ## Features
//!
//! - Per-host action summaries (add/change/delete counts)
//! - Colorized diff output for file changes
//! - Action type classification (create, modify, delete, configure)
//! - Summary statistics

use colored::Colorize;
use std::collections::HashMap;
use std::fmt;

use super::diff::{ColorizedDiff, DiffOptions};

/// Type of action a task would perform
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionType {
    /// Create a new resource
    Create,
    /// Modify an existing resource
    Modify,
    /// Delete a resource
    Delete,
    /// No change expected
    NoChange,
    /// Unable to determine
    Unknown,
}

impl ActionType {
    /// Get the colored symbol for this action type
    pub fn symbol(&self) -> String {
        match self {
            ActionType::Create => "+".green().to_string(),
            ActionType::Modify => "~".yellow().to_string(),
            ActionType::Delete => "-".red().to_string(),
            ActionType::NoChange => " ".to_string(),
            ActionType::Unknown => "?".dimmed().to_string(),
        }
    }

    /// Get the colored label for this action type
    pub fn label(&self) -> String {
        match self {
            ActionType::Create => "create".green().to_string(),
            ActionType::Modify => "change".yellow().to_string(),
            ActionType::Delete => "destroy".red().to_string(),
            ActionType::NoChange => "no change".dimmed().to_string(),
            ActionType::Unknown => "unknown".dimmed().to_string(),
        }
    }
}

impl fmt::Display for ActionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

/// A planned change for a specific host
#[derive(Debug, Clone)]
pub struct PlannedChange {
    /// The host this change applies to
    pub host: String,
    /// The task name
    pub task_name: String,
    /// The module being used
    pub module: String,
    /// Type of action
    pub action_type: ActionType,
    /// Resource being changed (file path, package name, service name, etc.)
    pub resource: String,
    /// Optional diff showing the change
    pub diff: Option<PlanDiff>,
    /// Description of the change
    pub description: String,
}

/// A diff showing before/after state
#[derive(Debug, Clone)]
pub struct PlanDiff {
    /// Content before the change
    pub before: String,
    /// Content after the change
    pub after: String,
    /// Label for before content
    pub before_label: String,
    /// Label for after content
    pub after_label: String,
}

impl PlanDiff {
    /// Create a new plan diff
    pub fn new(before: impl Into<String>, after: impl Into<String>) -> Self {
        Self {
            before: before.into(),
            after: after.into(),
            before_label: "before".to_string(),
            after_label: "after".to_string(),
        }
    }

    /// Set custom labels
    pub fn with_labels(mut self, before: impl Into<String>, after: impl Into<String>) -> Self {
        self.before_label = before.into();
        self.after_label = after.into();
        self
    }

    /// Generate colorized diff output
    pub fn render(&self) -> String {
        let differ = ColorizedDiff::with_options(DiffOptions {
            context_lines: 3,
            use_color: true,
            ..Default::default()
        });
        differ.diff(
            &self.before,
            &self.after,
            &self.before_label,
            &self.after_label,
        )
    }
}

/// Summary of planned changes for a host
#[derive(Debug, Clone, Default)]
pub struct HostSummary {
    /// Number of resources to create
    pub creates: usize,
    /// Number of resources to modify
    pub modifies: usize,
    /// Number of resources to delete
    pub deletes: usize,
    /// Number of no-change tasks
    pub no_changes: usize,
    /// Number of unknown/conditional tasks
    pub unknowns: usize,
    /// List of changes
    pub changes: Vec<PlannedChange>,
}

impl HostSummary {
    /// Add a change to this summary
    pub fn add_change(&mut self, change: PlannedChange) {
        match change.action_type {
            ActionType::Create => self.creates += 1,
            ActionType::Modify => self.modifies += 1,
            ActionType::Delete => self.deletes += 1,
            ActionType::NoChange => self.no_changes += 1,
            ActionType::Unknown => self.unknowns += 1,
        }
        self.changes.push(change);
    }

    /// Get total number of changes (excluding no-change)
    pub fn total_changes(&self) -> usize {
        self.creates + self.modifies + self.deletes
    }

    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        self.total_changes() > 0
    }
}

/// Plan output formatter
pub struct PlanFormatter {
    /// Per-host summaries
    host_summaries: HashMap<String, HostSummary>,
    /// Whether to show diffs
    show_diffs: bool,
    /// Whether to use colors
    use_color: bool,
}

impl Default for PlanFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl PlanFormatter {
    /// Create a new plan formatter
    pub fn new() -> Self {
        Self {
            host_summaries: HashMap::new(),
            show_diffs: true,
            use_color: true,
        }
    }

    /// Set whether to show diffs
    pub fn with_diffs(mut self, show: bool) -> Self {
        self.show_diffs = show;
        self
    }

    /// Set whether to use colors
    pub fn with_color(mut self, use_color: bool) -> Self {
        self.use_color = use_color;
        self
    }

    /// Add a planned change
    pub fn add_change(&mut self, change: PlannedChange) {
        let host = change.host.clone();
        self.host_summaries
            .entry(host)
            .or_default()
            .add_change(change);
    }

    /// Get summary for a specific host
    pub fn host_summary(&self, host: &str) -> Option<&HostSummary> {
        self.host_summaries.get(host)
    }

    /// Get all host summaries
    pub fn all_summaries(&self) -> &HashMap<String, HostSummary> {
        &self.host_summaries
    }

    /// Get total statistics across all hosts
    pub fn total_stats(&self) -> (usize, usize, usize, usize) {
        let mut creates = 0;
        let mut modifies = 0;
        let mut deletes = 0;
        let mut unknowns = 0;

        for summary in self.host_summaries.values() {
            creates += summary.creates;
            modifies += summary.modifies;
            deletes += summary.deletes;
            unknowns += summary.unknowns;
        }

        (creates, modifies, deletes, unknowns)
    }

    /// Format the plan summary header
    pub fn format_header(&self) -> String {
        let (creates, modifies, deletes, _) = self.total_stats();
        let total_hosts = self.host_summaries.len();

        let mut output = String::new();
        output.push_str(&"─".repeat(60));
        output.push('\n');
        output.push_str(&format!(
            "Plan: {} to add, {} to change, {} to destroy\n",
            creates.to_string().green(),
            modifies.to_string().yellow(),
            deletes.to_string().red()
        ));
        output.push_str(&format!("      across {} host(s)\n", total_hosts));
        output.push_str(&"─".repeat(60));
        output.push('\n');

        output
    }

    /// Format per-host summary
    pub fn format_host_summary(&self, host: &str) -> Option<String> {
        let summary = self.host_summaries.get(host)?;

        let mut output = String::new();
        output.push_str(&format!("\n{} {}:\n", "Host:".bold(), host.cyan()));

        if !summary.has_changes() && summary.unknowns == 0 {
            output.push_str("  No changes planned\n");
            return Some(output);
        }

        // Action counts
        let counts = format!(
            "  {} to add, {} to change, {} to destroy",
            summary.creates.to_string().green(),
            summary.modifies.to_string().yellow(),
            summary.deletes.to_string().red()
        );
        if summary.unknowns > 0 {
            output.push_str(&format!(
                "{}, {} conditional\n",
                counts,
                summary.unknowns.to_string().dimmed()
            ));
        } else {
            output.push_str(&format!("{}\n", counts));
        }

        // List each change
        for change in &summary.changes {
            let symbol = change.action_type.symbol();
            output.push_str(&format!(
                "\n  {} {} ({})\n",
                symbol,
                change.task_name.bold(),
                change.module.dimmed()
            ));

            if !change.resource.is_empty() {
                output.push_str(&format!("    Resource: {}\n", change.resource));
            }

            if !change.description.is_empty() {
                output.push_str(&format!("    {}\n", change.description.dimmed()));
            }

            // Show diff if available
            if self.show_diffs {
                if let Some(ref diff) = change.diff {
                    output.push_str("\n");
                    for line in diff.render().lines() {
                        output.push_str(&format!("      {}\n", line));
                    }
                }
            }
        }

        Some(output)
    }

    /// Format the complete plan output
    pub fn format(&self) -> String {
        let mut output = String::new();

        // Header
        output.push_str(&self.format_header());

        // Per-host summaries
        let mut hosts: Vec<_> = self.host_summaries.keys().collect();
        hosts.sort();

        for host in hosts {
            if let Some(host_output) = self.format_host_summary(host) {
                output.push_str(&host_output);
            }
        }

        // Footer
        output.push('\n');
        output.push_str(&"─".repeat(60));
        output.push('\n');

        let (creates, modifies, deletes, unknowns) = self.total_stats();
        if creates + modifies + deletes > 0 {
            output.push_str("Plan includes changes. Run without --plan to apply.\n");
        } else if unknowns > 0 {
            output.push_str(
                "Plan has conditional tasks. Actual changes depend on runtime conditions.\n",
            );
        } else {
            output.push_str("No changes planned.\n");
        }

        output
    }
}

/// Classify the action type based on module and arguments
pub fn classify_action(module: &str, args: Option<&serde_yaml::Value>) -> ActionType {
    match module {
        // File operations
        "file" | "ansible.builtin.file" => {
            if let Some(args) = args {
                match args.get("state").and_then(|s| s.as_str()) {
                    Some("absent") => ActionType::Delete,
                    Some("directory") | Some("touch") | Some("link") | Some("hard") => {
                        ActionType::Create
                    }
                    _ => ActionType::Modify,
                }
            } else {
                ActionType::Unknown
            }
        }

        // Copy/template - usually create or modify
        "copy" | "ansible.builtin.copy" | "template" | "ansible.builtin.template" => {
            ActionType::Create // Will be modified if file exists
        }

        // Package management
        "apt"
        | "ansible.builtin.apt"
        | "yum"
        | "ansible.builtin.yum"
        | "dnf"
        | "ansible.builtin.dnf"
        | "package"
        | "ansible.builtin.package" => {
            if let Some(args) = args {
                match args.get("state").and_then(|s| s.as_str()) {
                    Some("absent") | Some("removed") => ActionType::Delete,
                    Some("present") | Some("installed") | Some("latest") => ActionType::Create,
                    _ => ActionType::Modify,
                }
            } else {
                ActionType::Unknown
            }
        }

        // Service management
        "service" | "ansible.builtin.service" | "systemd" | "ansible.builtin.systemd" => {
            ActionType::Modify
        }

        // User/Group management
        "user" | "ansible.builtin.user" | "group" | "ansible.builtin.group" => {
            if let Some(args) = args {
                match args.get("state").and_then(|s| s.as_str()) {
                    Some("absent") => ActionType::Delete,
                    Some("present") => ActionType::Create,
                    _ => ActionType::Modify,
                }
            } else {
                ActionType::Create
            }
        }

        // Command/shell - unknown effect
        "command"
        | "ansible.builtin.command"
        | "shell"
        | "ansible.builtin.shell"
        | "raw"
        | "ansible.builtin.raw"
        | "script"
        | "ansible.builtin.script" => ActionType::Unknown,

        // Debug - no change
        "debug"
        | "ansible.builtin.debug"
        | "set_fact"
        | "ansible.builtin.set_fact"
        | "assert"
        | "ansible.builtin.assert" => ActionType::NoChange,

        // Include/import - no direct change
        "include_tasks"
        | "ansible.builtin.include_tasks"
        | "import_tasks"
        | "ansible.builtin.import_tasks"
        | "include_role"
        | "ansible.builtin.include_role"
        | "import_role"
        | "ansible.builtin.import_role" => ActionType::NoChange,

        // Default - unknown
        _ => ActionType::Unknown,
    }
}

/// Get a description of what the action will do
pub fn describe_action(
    module: &str,
    args: Option<&serde_yaml::Value>,
    action_type: ActionType,
) -> String {
    let action_verb = match action_type {
        ActionType::Create => "create",
        ActionType::Modify => "modify",
        ActionType::Delete => "delete",
        ActionType::NoChange => "no change to",
        ActionType::Unknown => "may affect",
    };

    match module {
        "file" | "ansible.builtin.file" => {
            let path = args
                .and_then(|a| a.get("path").or_else(|| a.get("dest")))
                .and_then(|p| p.as_str())
                .unwrap_or("<path>");
            format!("will {} file/directory: {}", action_verb, path)
        }

        "copy" | "ansible.builtin.copy" => {
            let dest = args
                .and_then(|a| a.get("dest"))
                .and_then(|d| d.as_str())
                .unwrap_or("<dest>");
            format!("will {} file: {}", action_verb, dest)
        }

        "template" | "ansible.builtin.template" => {
            let dest = args
                .and_then(|a| a.get("dest"))
                .and_then(|d| d.as_str())
                .unwrap_or("<dest>");
            format!("will {} from template: {}", action_verb, dest)
        }

        "apt"
        | "ansible.builtin.apt"
        | "yum"
        | "ansible.builtin.yum"
        | "dnf"
        | "ansible.builtin.dnf"
        | "package"
        | "ansible.builtin.package" => {
            let name = args
                .and_then(|a| a.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("<package>");
            format!("will {} package: {}", action_verb, name)
        }

        "service" | "ansible.builtin.service" | "systemd" | "ansible.builtin.systemd" => {
            let name = args
                .and_then(|a| a.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("<service>");
            let state = args
                .and_then(|a| a.get("state"))
                .and_then(|s| s.as_str())
                .map(|s| format!(" ({})", s))
                .unwrap_or_default();
            format!("will configure service: {}{}", name, state)
        }

        "user" | "ansible.builtin.user" => {
            let name = args
                .and_then(|a| a.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("<user>");
            format!("will {} user: {}", action_verb, name)
        }

        "group" | "ansible.builtin.group" => {
            let name = args
                .and_then(|a| a.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("<group>");
            format!("will {} group: {}", action_verb, name)
        }

        "command" | "ansible.builtin.command" | "shell" | "ansible.builtin.shell" => {
            let cmd = args
                .and_then(|a| a.get("cmd").or_else(|| a.get("_raw_params")))
                .and_then(|c| c.as_str())
                .map(|c| {
                    if c.len() > 40 {
                        format!("{}...", &c[..40])
                    } else {
                        c.to_string()
                    }
                })
                .unwrap_or_else(|| "<command>".to_string());
            format!("will execute: {}", cmd)
        }

        "debug" | "ansible.builtin.debug" => "will display debug info".to_string(),

        "set_fact" | "ansible.builtin.set_fact" => "will set fact variable".to_string(),

        _ => format!("will execute {} module", module),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_type_symbols() {
        assert!(ActionType::Create.symbol().contains('+'));
        assert!(ActionType::Modify.symbol().contains('~'));
        assert!(ActionType::Delete.symbol().contains('-'));
    }

    #[test]
    fn test_classify_action_file() {
        let args = serde_yaml::from_str::<serde_yaml::Value>("state: absent").unwrap();
        assert_eq!(classify_action("file", Some(&args)), ActionType::Delete);

        let args = serde_yaml::from_str::<serde_yaml::Value>("state: directory").unwrap();
        assert_eq!(classify_action("file", Some(&args)), ActionType::Create);
    }

    #[test]
    fn test_classify_action_package() {
        let args = serde_yaml::from_str::<serde_yaml::Value>("state: absent").unwrap();
        assert_eq!(classify_action("apt", Some(&args)), ActionType::Delete);

        let args = serde_yaml::from_str::<serde_yaml::Value>("state: present").unwrap();
        assert_eq!(classify_action("apt", Some(&args)), ActionType::Create);
    }

    #[test]
    fn test_classify_action_debug() {
        assert_eq!(classify_action("debug", None), ActionType::NoChange);
    }

    #[test]
    fn test_host_summary() {
        let mut summary = HostSummary::default();

        summary.add_change(PlannedChange {
            host: "host1".to_string(),
            task_name: "Create file".to_string(),
            module: "file".to_string(),
            action_type: ActionType::Create,
            resource: "/tmp/test".to_string(),
            diff: None,
            description: "will create file".to_string(),
        });

        summary.add_change(PlannedChange {
            host: "host1".to_string(),
            task_name: "Delete file".to_string(),
            module: "file".to_string(),
            action_type: ActionType::Delete,
            resource: "/tmp/old".to_string(),
            diff: None,
            description: "will delete file".to_string(),
        });

        assert_eq!(summary.creates, 1);
        assert_eq!(summary.deletes, 1);
        assert_eq!(summary.total_changes(), 2);
        assert!(summary.has_changes());
    }

    #[test]
    fn test_plan_formatter() {
        let mut formatter = PlanFormatter::new();

        formatter.add_change(PlannedChange {
            host: "host1".to_string(),
            task_name: "Install package".to_string(),
            module: "apt".to_string(),
            action_type: ActionType::Create,
            resource: "nginx".to_string(),
            diff: None,
            description: "will install package: nginx".to_string(),
        });

        let output = formatter.format();
        assert!(output.contains("host1"));
        assert!(output.contains("Install package"));
        assert!(output.contains("1 to add"));
    }

    #[test]
    fn test_plan_diff() {
        let diff =
            PlanDiff::new("old content\n", "new content\n").with_labels("original", "modified");

        let rendered = diff.render();
        assert!(rendered.contains("original") || rendered.contains("modified"));
    }

    #[test]
    fn test_describe_action_file() {
        let args = serde_yaml::from_str::<serde_yaml::Value>("path: /tmp/test").unwrap();
        let desc = describe_action("file", Some(&args), ActionType::Create);
        assert!(desc.contains("/tmp/test"));
        assert!(desc.contains("create"));
    }

    #[test]
    fn test_describe_action_package() {
        let args = serde_yaml::from_str::<serde_yaml::Value>("name: nginx").unwrap();
        let desc = describe_action("apt", Some(&args), ActionType::Create);
        assert!(desc.contains("nginx"));
        assert!(desc.contains("package"));
    }
}
