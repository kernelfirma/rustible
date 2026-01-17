//! Best practices checks for playbooks.
//!
//! This module implements checks for Ansible/Rustible best practices including:
//! - Naming conventions
//! - Task organization
//! - Variable usage
//! - Idempotency concerns

use super::types::{LintConfig, LintIssue, LintResult, Location, RuleCategory, Severity};
use regex::Regex;
use std::path::Path;

/// Best practices checker.
pub struct BestPracticesChecker {
    /// Regex for valid task names.
    valid_name_pattern: Regex,
    /// Regex for detecting Jinja2 templates.
    jinja_pattern: Regex,
}

impl Default for BestPracticesChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl BestPracticesChecker {
    /// Create a new best practices checker.
    pub fn new() -> Self {
        Self {
            valid_name_pattern: Regex::new(r"^[A-Z][a-zA-Z0-9\s\-_:]+$").unwrap(),
            jinja_pattern: Regex::new(r"\{\{.*?\}\}|\{%.*?%\}").unwrap(),
        }
    }

    /// Check best practices in a parsed playbook.
    pub fn check_playbook(
        &self,
        value: &serde_yaml::Value,
        path: &Path,
        config: &LintConfig,
    ) -> LintResult {
        let mut result = LintResult::new();
        result.files_analyzed.push(path.to_path_buf());

        if let Some(plays) = value.as_sequence() {
            for (play_idx, play) in plays.iter().enumerate() {
                self.check_play(play, play_idx, path, config, &mut result);
            }
        }

        result
    }

    /// Check a single play.
    fn check_play(
        &self,
        play: &serde_yaml::Value,
        play_idx: usize,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        let play_map = match play.as_mapping() {
            Some(m) => m,
            None => return,
        };

        let play_name = play_map
            .get(&serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str());

        result.plays_analyzed += 1;

        // Check play name
        self.check_play_name(play_name, play_idx, path, config, result);

        // Check for gather_facts with no tasks
        self.check_gather_facts_usage(play_map, play_idx, play_name, path, config, result);

        // Check tasks
        for task_key in &["tasks", "pre_tasks", "post_tasks", "handlers"] {
            if let Some(tasks) = play_map.get(&serde_yaml::Value::String(task_key.to_string())) {
                if let Some(task_list) = tasks.as_sequence() {
                    for (task_idx, task) in task_list.iter().enumerate() {
                        self.check_task(task, task_idx, play_idx, play_name, path, config, result);
                    }
                }
            }
        }
    }

    /// Check play name.
    fn check_play_name(
        &self,
        name: Option<&str>,
        play_idx: usize,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        match name {
            None => {
                if config.should_run_rule("B001", RuleCategory::BestPractices, Severity::Warning) {
                    result.add_issue(LintIssue::new(
                        "B001",
                        "unnamed-play",
                        Severity::Warning,
                        RuleCategory::BestPractices,
                        "Play does not have a name",
                        Location::file(path).with_play(play_idx, None),
                    ).with_suggestion("Add a descriptive 'name' to the play for better readability"));
                }
            }
            Some(name) if name.trim().is_empty() => {
                if config.should_run_rule("B002", RuleCategory::BestPractices, Severity::Warning) {
                    result.add_issue(LintIssue::new(
                        "B002",
                        "empty-play-name",
                        Severity::Warning,
                        RuleCategory::BestPractices,
                        "Play has an empty name",
                        Location::file(path).with_play(play_idx, None),
                    ).with_suggestion("Provide a meaningful name for the play"));
                }
            }
            _ => {}
        }
    }

    /// Check gather_facts usage.
    fn check_gather_facts_usage(
        &self,
        play: &serde_yaml::Mapping,
        play_idx: usize,
        play_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        let gather_facts = play
            .get(&serde_yaml::Value::String("gather_facts".to_string()))
            .and_then(|v| {
                if v.is_bool() {
                    v.as_bool()
                } else if let Some(s) = v.as_str() {
                    Some(matches!(s.to_lowercase().as_str(), "true" | "yes"))
                } else {
                    None
                }
            })
            .unwrap_or(true); // default is true

        // Count total tasks
        let task_count: usize = ["tasks", "pre_tasks", "post_tasks"]
            .iter()
            .filter_map(|k| play.get(&serde_yaml::Value::String(k.to_string())))
            .filter_map(|v| v.as_sequence())
            .map(|s| s.len())
            .sum();

        if gather_facts && task_count == 0 {
            if config.should_run_rule("B003", RuleCategory::BestPractices, Severity::Hint) {
                result.add_issue(LintIssue::new(
                    "B003",
                    "gather-facts-no-tasks",
                    Severity::Hint,
                    RuleCategory::BestPractices,
                    "Play gathers facts but has no tasks",
                    Location::file(path).with_play(play_idx, play_name.map(String::from)),
                ).with_suggestion("Consider setting 'gather_facts: false' or adding tasks"));
            }
        }
    }

    /// Check a single task.
    fn check_task(
        &self,
        task: &serde_yaml::Value,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        let task_map = match task.as_mapping() {
            Some(m) => m,
            None => return,
        };

        let task_name = task_map
            .get(&serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str());

        result.tasks_analyzed += 1;

        // Check task name
        self.check_task_name(task_name, task_idx, play_idx, play_name, path, config, result);

        // Check for command/shell usage
        self.check_command_usage(task_map, task_idx, play_idx, play_name, task_name, path, config, result);

        // Check for deprecated features
        self.check_deprecated_features(task_map, task_idx, play_idx, play_name, task_name, path, config, result);

        // Check for git with version
        self.check_git_pinning(task_map, task_idx, play_idx, play_name, task_name, path, config, result);

        // Check for proper use of become
        self.check_become_usage(task_map, task_idx, play_idx, play_name, task_name, path, config, result);

        // Check for relative paths in certain modules
        self.check_path_usage(task_map, task_idx, play_idx, play_name, task_name, path, config, result);

        // Check for handlers without notify
        self.check_handler_names(task_map, task_idx, play_idx, play_name, task_name, path, config, result);

        // Check for retries without until
        self.check_retry_usage(task_map, task_idx, play_idx, play_name, task_name, path, config, result);

        // Recursively check block tasks
        for block_key in &["block", "rescue", "always"] {
            if let Some(block_tasks) = task_map.get(&serde_yaml::Value::String(block_key.to_string())) {
                if let Some(block_list) = block_tasks.as_sequence() {
                    for (block_idx, block_task) in block_list.iter().enumerate() {
                        self.check_task(block_task, block_idx, play_idx, play_name, path, config, result);
                    }
                }
            }
        }
    }

    /// Check task name.
    fn check_task_name(
        &self,
        name: Option<&str>,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        match name {
            None => {
                if config.should_run_rule("B004", RuleCategory::BestPractices, Severity::Warning) {
                    result.add_issue(LintIssue::new(
                        "B004",
                        "unnamed-task",
                        Severity::Warning,
                        RuleCategory::BestPractices,
                        "Task does not have a name",
                        Location::file(path)
                            .with_play(play_idx, play_name.map(String::from))
                            .with_task(task_idx, None),
                    ).with_suggestion("Add a descriptive 'name' to the task"));
                }
            }
            Some(name) if name.trim().is_empty() => {
                if config.should_run_rule("B005", RuleCategory::BestPractices, Severity::Warning) {
                    result.add_issue(LintIssue::new(
                        "B005",
                        "empty-task-name",
                        Severity::Warning,
                        RuleCategory::BestPractices,
                        "Task has an empty name",
                        Location::file(path)
                            .with_play(play_idx, play_name.map(String::from))
                            .with_task(task_idx, None),
                    ).with_suggestion("Provide a meaningful name for the task"));
                }
            }
            Some(name) if !name.starts_with(|c: char| c.is_uppercase()) => {
                if config.should_run_rule("B006", RuleCategory::BestPractices, Severity::Hint) {
                    result.add_issue(LintIssue::new(
                        "B006",
                        "lowercase-task-name",
                        Severity::Hint,
                        RuleCategory::BestPractices,
                        "Task name should start with an uppercase letter",
                        Location::file(path)
                            .with_play(play_idx, play_name.map(String::from))
                            .with_task(task_idx, Some(name.to_string())),
                    ).with_suggestion("Capitalize the first letter of the task name"));
                }
            }
            _ => {}
        }
    }

    /// Check command/shell usage.
    fn check_command_usage(
        &self,
        task: &serde_yaml::Mapping,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        task_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        // Check for shell when command would suffice
        if let Some(shell_args) = task.get(&serde_yaml::Value::String("shell".to_string())) {
            let cmd = match shell_args {
                serde_yaml::Value::String(s) => Some(s.as_str()),
                serde_yaml::Value::Mapping(m) => {
                    m.get(&serde_yaml::Value::String("cmd".to_string()))
                        .and_then(|v| v.as_str())
                }
                _ => None,
            };

            if let Some(cmd) = cmd {
                // Check for shell features
                let shell_features = ['|', '>', '<', '&', ';', '$', '`', '(', ')', '{', '}', '*', '?', '[', ']'];
                let uses_shell_features = cmd.chars().any(|c| shell_features.contains(&c));

                if !uses_shell_features {
                    if config.should_run_rule("B007", RuleCategory::BestPractices, Severity::Warning) {
                        result.add_issue(LintIssue::new(
                            "B007",
                            "use-command-instead",
                            Severity::Warning,
                            RuleCategory::BestPractices,
                            "Use 'command' module instead of 'shell' when shell features are not needed",
                            Location::file(path)
                                .with_play(play_idx, play_name.map(String::from))
                                .with_task(task_idx, task_name.map(String::from)),
                        ).with_suggestion("Replace 'shell' with 'command' for better security and performance"));
                    }
                }
            }
        }

        // Check for command/shell without creates/removes for idempotency
        for cmd_module in &["command", "shell"] {
            if let Some(args) = task.get(&serde_yaml::Value::String(cmd_module.to_string())) {
                let has_creates = if let Some(m) = args.as_mapping() {
                    m.contains_key(&serde_yaml::Value::String("creates".to_string()))
                        || m.contains_key(&serde_yaml::Value::String("removes".to_string()))
                } else {
                    false
                };

                let has_changed_when = task.contains_key(&serde_yaml::Value::String("changed_when".to_string()));

                if !has_creates && !has_changed_when {
                    if config.should_run_rule("B008", RuleCategory::BestPractices, Severity::Hint) {
                        result.add_issue(LintIssue::new(
                            "B008",
                            "command-not-idempotent",
                            Severity::Hint,
                            RuleCategory::BestPractices,
                            format!("'{}' module used without idempotency guard", cmd_module),
                            Location::file(path)
                                .with_play(play_idx, play_name.map(String::from))
                                .with_task(task_idx, task_name.map(String::from)),
                        ).with_suggestion("Add 'creates', 'removes', or 'changed_when' for idempotency"));
                    }
                }
            }
        }
    }

    /// Check for deprecated features.
    fn check_deprecated_features(
        &self,
        task: &serde_yaml::Mapping,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        task_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        // Check for deprecated with_* loops
        let deprecated_loops = [
            ("with_items", "Use 'loop' instead of 'with_items'"),
            ("with_nested", "Use 'loop' with 'product' filter instead of 'with_nested'"),
        ];

        for (loop_key, suggestion) in deprecated_loops {
            if task.contains_key(&serde_yaml::Value::String(loop_key.to_string())) {
                if config.should_run_rule("B009", RuleCategory::Deprecation, Severity::Hint) {
                    result.add_issue(LintIssue::new(
                        "B009",
                        "deprecated-loop",
                        Severity::Hint,
                        RuleCategory::Deprecation,
                        format!("'{}' is deprecated", loop_key),
                        Location::file(path)
                            .with_play(play_idx, play_name.map(String::from))
                            .with_task(task_idx, task_name.map(String::from)),
                    ).with_suggestion(suggestion));
                }
            }
        }

        // Check for sudo instead of become
        if task.contains_key(&serde_yaml::Value::String("sudo".to_string())) {
            if config.should_run_rule("B010", RuleCategory::Deprecation, Severity::Warning) {
                result.add_issue(LintIssue::new(
                    "B010",
                    "deprecated-sudo",
                    Severity::Warning,
                    RuleCategory::Deprecation,
                    "'sudo' is deprecated, use 'become' instead",
                    Location::file(path)
                        .with_play(play_idx, play_name.map(String::from))
                        .with_task(task_idx, task_name.map(String::from)),
                ).with_suggestion("Replace 'sudo: yes' with 'become: yes'"));
            }
        }
    }

    /// Check git module for version pinning.
    fn check_git_pinning(
        &self,
        task: &serde_yaml::Mapping,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        task_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        if let Some(git_args) = task.get(&serde_yaml::Value::String("git".to_string())) {
            if let Some(args_map) = git_args.as_mapping() {
                let has_version = args_map.contains_key(&serde_yaml::Value::String("version".to_string()));

                if !has_version {
                    if config.should_run_rule("B011", RuleCategory::BestPractices, Severity::Warning) {
                        result.add_issue(LintIssue::new(
                            "B011",
                            "git-no-version",
                            Severity::Warning,
                            RuleCategory::BestPractices,
                            "git module used without specifying a version/tag/branch",
                            Location::file(path)
                                .with_play(play_idx, play_name.map(String::from))
                                .with_task(task_idx, task_name.map(String::from)),
                        ).with_suggestion("Specify 'version' to ensure reproducible builds"));
                    }
                }
            }
        }
    }

    /// Check become usage.
    fn check_become_usage(
        &self,
        task: &serde_yaml::Mapping,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        task_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        // Check for become_user without become
        let has_become = task.contains_key(&serde_yaml::Value::String("become".to_string()));
        let has_become_user = task.contains_key(&serde_yaml::Value::String("become_user".to_string()));

        if has_become_user && !has_become {
            if config.should_run_rule("B012", RuleCategory::BestPractices, Severity::Warning) {
                result.add_issue(LintIssue::new(
                    "B012",
                    "become-user-without-become",
                    Severity::Warning,
                    RuleCategory::BestPractices,
                    "'become_user' specified without 'become: yes'",
                    Location::file(path)
                        .with_play(play_idx, play_name.map(String::from))
                        .with_task(task_idx, task_name.map(String::from)),
                ).with_suggestion("Add 'become: yes' or remove 'become_user'"));
            }
        }
    }

    /// Check path usage in certain modules.
    fn check_path_usage(
        &self,
        task: &serde_yaml::Mapping,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        task_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        // Modules that should use absolute paths for dest
        let path_modules = ["copy", "template", "file"];

        for module_name in path_modules {
            if let Some(args) = task.get(&serde_yaml::Value::String(module_name.to_string())) {
                if let Some(args_map) = args.as_mapping() {
                    // Check dest parameter
                    for dest_key in &["dest", "path"] {
                        if let Some(dest) = args_map.get(&serde_yaml::Value::String(dest_key.to_string())) {
                            if let Some(dest_str) = dest.as_str() {
                                // Skip if it's a template variable
                                if !self.jinja_pattern.is_match(dest_str) && !dest_str.starts_with('/') && !dest_str.starts_with('~') {
                                    if config.should_run_rule("B013", RuleCategory::BestPractices, Severity::Hint) {
                                        result.add_issue(LintIssue::new(
                                            "B013",
                                            "relative-path",
                                            Severity::Hint,
                                            RuleCategory::BestPractices,
                                            format!("Relative path '{}' used for '{}'", dest_str, dest_key),
                                            Location::file(path)
                                                .with_play(play_idx, play_name.map(String::from))
                                                .with_task(task_idx, task_name.map(String::from)),
                                        ).with_suggestion("Consider using an absolute path for clarity"));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Check handler naming.
    fn check_handler_names(
        &self,
        task: &serde_yaml::Mapping,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        task_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        if let Some(notify) = task.get(&serde_yaml::Value::String("notify".to_string())) {
            let handlers: Vec<&str> = match notify {
                serde_yaml::Value::String(s) => vec![s.as_str()],
                serde_yaml::Value::Sequence(seq) => {
                    seq.iter().filter_map(|v| v.as_str()).collect()
                }
                _ => vec![],
            };

            for handler in handlers {
                // Check for handler names with spaces in wrong format
                if handler.contains("  ") {
                    if config.should_run_rule("B014", RuleCategory::BestPractices, Severity::Hint) {
                        result.add_issue(LintIssue::new(
                            "B014",
                            "handler-multiple-spaces",
                            Severity::Hint,
                            RuleCategory::BestPractices,
                            format!("Handler name '{}' contains multiple consecutive spaces", handler),
                            Location::file(path)
                                .with_play(play_idx, play_name.map(String::from))
                                .with_task(task_idx, task_name.map(String::from)),
                        ).with_suggestion("Use single spaces in handler names"));
                    }
                }
            }
        }
    }

    /// Check retry usage.
    fn check_retry_usage(
        &self,
        task: &serde_yaml::Mapping,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        task_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        let has_retries = task.contains_key(&serde_yaml::Value::String("retries".to_string()));
        let has_until = task.contains_key(&serde_yaml::Value::String("until".to_string()));

        if has_retries && !has_until {
            if config.should_run_rule("B015", RuleCategory::BestPractices, Severity::Warning) {
                result.add_issue(LintIssue::new(
                    "B015",
                    "retries-without-until",
                    Severity::Warning,
                    RuleCategory::BestPractices,
                    "'retries' specified without 'until' condition",
                    Location::file(path)
                        .with_play(play_idx, play_name.map(String::from))
                        .with_task(task_idx, task_name.map(String::from)),
                ).with_suggestion("Add 'until' condition to define when retries should stop"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unnamed_task() {
        let checker = BestPracticesChecker::new();
        let content = r#"
- name: Test play
  hosts: all
  tasks:
    - command: echo hello
"#;
        let value: serde_yaml::Value = serde_yaml::from_str(content).unwrap();
        let result = checker.check_playbook(&value, Path::new("test.yml"), &LintConfig::new());

        assert!(result.issues.iter().any(|i| i.rule_id == "B004"));
    }

    #[test]
    fn test_shell_without_features() {
        let checker = BestPracticesChecker::new();
        let content = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Simple command
      shell: echo hello
"#;
        let value: serde_yaml::Value = serde_yaml::from_str(content).unwrap();
        let result = checker.check_playbook(&value, Path::new("test.yml"), &LintConfig::new());

        assert!(result.issues.iter().any(|i| i.rule_id == "B007"));
    }
}
