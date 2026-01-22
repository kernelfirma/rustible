//! YAML syntax validation for playbooks.
//!
//! This module provides YAML-level validation including:
//! - Syntax error detection with line numbers
//! - Structure validation (playbooks must be lists of plays)
//! - Key validation (detecting unknown or misspelled keys)
//! - Indentation consistency checking

use super::types::{
    LintConfig, LintIssue, LintOpResult, LintResult, Location, RuleCategory, Severity,
};
use std::collections::HashSet;
use std::path::Path;

/// Known valid keys at the play level.
const PLAY_KEYS: &[&str] = &[
    "name",
    "hosts",
    "gather_facts",
    "gather_subset",
    "gather_timeout",
    "remote_user",
    "become",
    "become_method",
    "become_user",
    "become_password",
    "connection",
    "environment",
    "vars",
    "vars_files",
    "vars_prompt",
    "pre_tasks",
    "roles",
    "tasks",
    "post_tasks",
    "handlers",
    "serial",
    "max_fail_percentage",
    "ignore_errors",
    "ignore_unreachable",
    "module_defaults",
    "tags",
    "strategy",
    "throttle",
    "order",
    "force_handlers",
    "run_once",
    "when",
    "any_errors_fatal",
    "port",
    "timeout",
    "collections",
    "fact_path",
];

/// Known valid keys at the task level.
const TASK_KEYS: &[&str] = &[
    "name",
    "action",
    "when",
    "loop",
    "with_items",
    "with_dict",
    "with_file",
    "with_fileglob",
    "with_first_found",
    "with_together",
    "with_nested",
    "with_random_choice",
    "with_sequence",
    "with_subelements",
    "with_template",
    "with_inventory_hostnames",
    "with_indexed_items",
    "loop_control",
    "register",
    "notify",
    "listen",
    "ignore_errors",
    "ignore_unreachable",
    "changed_when",
    "failed_when",
    "tags",
    "become",
    "become_method",
    "become_user",
    "delegate_to",
    "delegate_facts",
    "local_action",
    "run_once",
    "retries",
    "delay",
    "until",
    "async",
    "poll",
    "environment",
    "vars",
    "args",
    "block",
    "rescue",
    "always",
    "connection",
    "throttle",
    "timeout",
    "no_log",
    "diff",
    "check_mode",
    "module_defaults",
    "any_errors_fatal",
    "debugger",
];

/// YAML syntax checker.
pub struct YamlChecker {
    /// Known module names for validation.
    known_modules: HashSet<String>,
}

impl Default for YamlChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl YamlChecker {
    /// Create a new YAML checker.
    pub fn new() -> Self {
        Self {
            known_modules: crate::modules::ModuleRegistry::with_builtins()
                .names()
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }

    /// Add additional known modules.
    pub fn with_modules(mut self, modules: impl IntoIterator<Item = String>) -> Self {
        self.known_modules.extend(modules);
        self
    }

    /// Check YAML syntax in a file.
    pub fn check_file(&self, path: &Path, config: &LintConfig) -> LintOpResult<LintResult> {
        let content =
            std::fs::read_to_string(path).map_err(|e| super::types::LintError::FileRead {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;

        self.check_content(&content, path, config)
    }

    /// Check YAML syntax in content.
    pub fn check_content(
        &self,
        content: &str,
        path: &Path,
        config: &LintConfig,
    ) -> LintOpResult<LintResult> {
        let mut result = LintResult::new();
        result.files_analyzed.push(path.to_path_buf());

        // First, try to parse as YAML to catch syntax errors
        let yaml_result: Result<serde_yaml::Value, _> = serde_yaml::from_str(content);

        match yaml_result {
            Err(e) => {
                // Extract line number from YAML error if available
                let line = e.location().map(|l| l.line());

                if config.should_run_rule("E001", RuleCategory::Syntax, Severity::Error) {
                    result.add_issue(LintIssue::new(
                        "E001",
                        "yaml-syntax-error",
                        Severity::Error,
                        RuleCategory::Syntax,
                        format!("YAML syntax error: {}", e),
                        Location::file(path).with_line(line.unwrap_or(1)),
                    ).with_suggestion("Check YAML syntax - ensure proper indentation (2 spaces) and correct structure"));
                }
            }
            Ok(value) => {
                // Check structure
                self.check_structure(&value, path, config, &mut result)?;
            }
        }

        // Check for common YAML issues in the raw content
        self.check_common_issues(content, path, config, &mut result);

        Ok(result)
    }

    /// Check the structure of parsed YAML.
    fn check_structure(
        &self,
        value: &serde_yaml::Value,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) -> LintOpResult<()> {
        // Playbooks should be a sequence at the top level
        match value {
            serde_yaml::Value::Sequence(plays) => {
                result.plays_analyzed = plays.len();

                for (play_idx, play) in plays.iter().enumerate() {
                    self.check_play(play, play_idx, path, config, result)?;
                }
            }
            serde_yaml::Value::Mapping(_) => {
                // Could be a single play or a task file
                if config.should_run_rule("E002", RuleCategory::Syntax, Severity::Warning) {
                    result.add_issue(
                        LintIssue::new(
                            "E002",
                            "playbook-not-list",
                            Severity::Warning,
                            RuleCategory::Syntax,
                            "Playbook should be a list of plays, not a single mapping",
                            Location::file(path),
                        )
                        .with_suggestion("Wrap the play in a YAML list using '- ' prefix"),
                    );
                }
                result.plays_analyzed = 1;
            }
            _ => {
                if config.should_run_rule("E003", RuleCategory::Syntax, Severity::Error) {
                    result.add_issue(LintIssue::new(
                        "E003",
                        "invalid-playbook-type",
                        Severity::Error,
                        RuleCategory::Syntax,
                        "Playbook must be a YAML list of plays",
                        Location::file(path),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Check a single play.
    fn check_play(
        &self,
        play: &serde_yaml::Value,
        play_idx: usize,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) -> LintOpResult<()> {
        let play_map = match play.as_mapping() {
            Some(m) => m,
            None => {
                if config.should_run_rule("E004", RuleCategory::Syntax, Severity::Error) {
                    result.add_issue(LintIssue::new(
                        "E004",
                        "play-not-mapping",
                        Severity::Error,
                        RuleCategory::Syntax,
                        "Play must be a YAML mapping",
                        Location::file(path).with_play(play_idx, None),
                    ));
                }
                return Ok(());
            }
        };

        let play_name = play_map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str())
            .map(String::from);

        // Check for required 'hosts' key
        if !play_map.contains_key(serde_yaml::Value::String("hosts".to_string()))
            && config.should_run_rule("E005", RuleCategory::Syntax, Severity::Error)
        {
            result.add_issue(
                LintIssue::new(
                    "E005",
                    "missing-hosts",
                    Severity::Error,
                    RuleCategory::Syntax,
                    "Play is missing required 'hosts' key",
                    Location::file(path).with_play(play_idx, play_name.clone()),
                )
                .with_suggestion("Add 'hosts: all' or specify target hosts"),
            );
        }

        // Check for unknown keys
        let play_keys_set: HashSet<&str> = PLAY_KEYS.iter().copied().collect();
        for key in play_map.keys() {
            if let Some(key_str) = key.as_str() {
                if !play_keys_set.contains(key_str)
                    && config.should_run_rule("W001", RuleCategory::Syntax, Severity::Warning)
                {
                    let similar = find_similar_key(key_str, &play_keys_set);
                    let mut issue = LintIssue::new(
                        "W001",
                        "unknown-play-key",
                        Severity::Warning,
                        RuleCategory::Syntax,
                        format!("Unknown key '{}' in play", key_str),
                        Location::file(path).with_play(play_idx, play_name.clone()),
                    );
                    if let Some(suggestion) = similar {
                        issue = issue.with_suggestion(format!("Did you mean '{}'?", suggestion));
                    }
                    result.add_issue(issue);
                }
            }
        }

        // Check tasks
        for task_key in &["tasks", "pre_tasks", "post_tasks", "handlers"] {
            if let Some(tasks) = play_map.get(serde_yaml::Value::String(task_key.to_string())) {
                if let Some(task_list) = tasks.as_sequence() {
                    for (task_idx, task) in task_list.iter().enumerate() {
                        self.check_task(
                            task, task_idx, play_idx, &play_name, path, config, result,
                        )?;
                        result.tasks_analyzed += 1;
                    }
                }
            }
        }

        Ok(())
    }

    /// Check a single task.
    #[allow(clippy::too_many_arguments)]
    fn check_task(
        &self,
        task: &serde_yaml::Value,
        task_idx: usize,
        play_idx: usize,
        play_name: &Option<String>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) -> LintOpResult<()> {
        let task_map = match task.as_mapping() {
            Some(m) => m,
            None => {
                if config.should_run_rule("E006", RuleCategory::Syntax, Severity::Error) {
                    result.add_issue(LintIssue::new(
                        "E006",
                        "task-not-mapping",
                        Severity::Error,
                        RuleCategory::Syntax,
                        "Task must be a YAML mapping",
                        Location::file(path)
                            .with_play(play_idx, play_name.clone())
                            .with_task(task_idx, None),
                    ));
                }
                return Ok(());
            }
        };

        let task_name = task_map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str())
            .map(String::from);

        // Check for unknown keys
        let task_keys_set: HashSet<&str> = TASK_KEYS.iter().copied().collect();
        let mut found_module = false;

        for key in task_map.keys() {
            if let Some(key_str) = key.as_str() {
                // Check if it's a known task attribute
                if task_keys_set.contains(key_str) {
                    continue;
                }

                // Check if it's a known module
                if self.known_modules.contains(key_str) {
                    found_module = true;
                    continue;
                }

                // Check if it starts with 'with_' (loop construct)
                if key_str.starts_with("with_") {
                    continue;
                }

                // Check for include/import variants
                if key_str.starts_with("include_") || key_str.starts_with("import_") {
                    found_module = true;
                    continue;
                }

                // Unknown key - could be a misspelled module or task attribute
                if config.should_run_rule("W002", RuleCategory::Syntax, Severity::Warning) {
                    let mut issue = LintIssue::new(
                        "W002",
                        "unknown-task-key",
                        Severity::Warning,
                        RuleCategory::Syntax,
                        format!(
                            "Unknown key '{}' in task - may be misspelled module or attribute",
                            key_str
                        ),
                        Location::file(path)
                            .with_play(play_idx, play_name.clone())
                            .with_task(task_idx, task_name.clone()),
                    );

                    // Check for similar task keys
                    if let Some(similar) = find_similar_key(key_str, &task_keys_set) {
                        issue = issue.with_suggestion(format!("Did you mean '{}'?", similar));
                    }
                    result.add_issue(issue);
                }
            }
        }

        // Check if task has a module (unless it's a block task)
        let is_block = task_map.contains_key(serde_yaml::Value::String("block".to_string()));
        if !found_module && !is_block {
            // Task might be using 'action' key
            let has_action = task_map.contains_key(serde_yaml::Value::String("action".to_string()));
            let has_local_action =
                task_map.contains_key(serde_yaml::Value::String("local_action".to_string()));

            if !has_action
                && !has_local_action
                && config.should_run_rule("E007", RuleCategory::Syntax, Severity::Error)
            {
                result.add_issue(
                    LintIssue::new(
                        "E007",
                        "task-missing-module",
                        Severity::Error,
                        RuleCategory::Syntax,
                        "Task does not specify a module to execute",
                        Location::file(path)
                            .with_play(play_idx, play_name.clone())
                            .with_task(task_idx, task_name.clone()),
                    )
                    .with_suggestion("Add a module name like 'debug:', 'command:', or 'shell:'"),
                );
            }
        }

        // Recursively check block/rescue/always
        for block_key in &["block", "rescue", "always"] {
            if let Some(block_tasks) =
                task_map.get(serde_yaml::Value::String(block_key.to_string()))
            {
                if let Some(block_list) = block_tasks.as_sequence() {
                    for (block_task_idx, block_task) in block_list.iter().enumerate() {
                        self.check_task(
                            block_task,
                            block_task_idx,
                            play_idx,
                            play_name,
                            path,
                            config,
                            result,
                        )?;
                        result.tasks_analyzed += 1;
                    }
                }
            }
        }

        Ok(())
    }

    /// Check for common YAML issues in raw content.
    fn check_common_issues(
        &self,
        content: &str,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        for (line_num, line) in content.lines().enumerate() {
            let line_num = line_num + 1; // 1-indexed

            // Check for tabs (YAML should use spaces)
            if line.contains('\t')
                && config.should_run_rule("E008", RuleCategory::Syntax, Severity::Error)
            {
                result.add_issue(
                    LintIssue::new(
                        "E008",
                        "yaml-tab-character",
                        Severity::Error,
                        RuleCategory::Syntax,
                        "YAML files should use spaces for indentation, not tabs",
                        Location::file(path).with_line(line_num),
                    )
                    .with_suggestion("Replace tabs with spaces (2 spaces per level is standard)"),
                );
            }

            // Check for trailing whitespace
            if (line.ends_with(' ') || line.ends_with('\t'))
                && config.should_run_rule("W003", RuleCategory::Syntax, Severity::Hint)
            {
                result.add_issue(
                    LintIssue::new(
                        "W003",
                        "trailing-whitespace",
                        Severity::Hint,
                        RuleCategory::Syntax,
                        "Line has trailing whitespace",
                        Location::file(path).with_line(line_num),
                    )
                    .with_suggestion("Remove trailing whitespace"),
                );
            }

            // Check for very long lines
            if line.len() > 160
                && config.should_run_rule("W004", RuleCategory::Syntax, Severity::Hint)
            {
                result.add_issue(
                    LintIssue::new(
                        "W004",
                        "line-too-long",
                        Severity::Hint,
                        RuleCategory::Syntax,
                        format!(
                            "Line is {} characters long (max recommended: 160)",
                            line.len()
                        ),
                        Location::file(path).with_line(line_num),
                    )
                    .with_suggestion("Consider breaking long lines for readability"),
                );
            }

            // Check for odd indentation (not multiple of 2)
            let indent = line.len() - line.trim_start().len();
            if indent > 0
                && indent % 2 != 0
                && !line.trim().is_empty()
                && config.should_run_rule("W005", RuleCategory::Syntax, Severity::Warning)
            {
                result.add_issue(
                    LintIssue::new(
                        "W005",
                        "odd-indentation",
                        Severity::Warning,
                        RuleCategory::Syntax,
                        format!("Indentation is {} spaces (should be multiple of 2)", indent),
                        Location::file(path).with_line(line_num),
                    )
                    .with_suggestion("Use consistent 2-space indentation"),
                );
            }
        }

        // Check for CRLF line endings
        if content.contains("\r\n")
            && config.should_run_rule("W006", RuleCategory::Syntax, Severity::Hint)
        {
            result.add_issue(
                LintIssue::new(
                    "W006",
                    "crlf-line-endings",
                    Severity::Hint,
                    RuleCategory::Syntax,
                    "File uses Windows-style (CRLF) line endings",
                    Location::file(path),
                )
                .with_suggestion("Convert to Unix-style (LF) line endings"),
            );
        }
    }
}

/// Find a similar key using Levenshtein distance.
fn find_similar_key<'a>(key: &str, valid_keys: &HashSet<&'a str>) -> Option<&'a str> {
    let key_lower = key.to_lowercase();
    let mut best_match: Option<(&str, usize)> = None;

    for &valid in valid_keys {
        let distance = levenshtein_distance(&key_lower, &valid.to_lowercase());
        // Only suggest if distance is small relative to key length
        if distance <= 2 && distance < key.len() / 2 + 1 {
            match best_match {
                None => best_match = Some((valid, distance)),
                Some((_, best_dist)) if distance < best_dist => {
                    best_match = Some((valid, distance));
                }
                _ => {}
            }
        }
    }

    best_match.map(|(s, _)| s)
}

/// Calculate Levenshtein distance between two strings.
#[allow(clippy::needless_range_loop)]
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = (matrix[i - 1][j] + 1)
                .min(matrix[i][j - 1] + 1)
                .min(matrix[i - 1][j - 1] + cost);
        }
    }

    matrix[a_len][b_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", "abc"), 0);
    }

    #[test]
    fn test_find_similar_key() {
        let keys: HashSet<&str> = ["hosts", "tasks", "name"].iter().copied().collect();
        assert_eq!(find_similar_key("hots", &keys), Some("hosts"));
        assert_eq!(find_similar_key("taks", &keys), Some("tasks"));
        assert_eq!(find_similar_key("xyz", &keys), None);
    }

    #[test]
    fn test_yaml_checker_valid() {
        let checker = YamlChecker::new();
        let content = r#"
- name: Test playbook
  hosts: all
  tasks:
    - name: Test task
      debug:
        msg: "Hello"
"#;
        let result = checker
            .check_content(content, Path::new("test.yml"), &LintConfig::new())
            .unwrap();

        // Should not have critical errors
        assert!(!result.issues.iter().any(|i| i.severity == Severity::Error));
    }

    #[test]
    fn test_yaml_checker_syntax_error() {
        let checker = YamlChecker::new();
        let content = r#"
- name: Bad playbook
  hosts: all
  tasks:
    - name: Bad indentation
    debug:  # Wrong indentation
      msg: "Hello"
"#;
        let result = checker
            .check_content(content, Path::new("test.yml"), &LintConfig::new())
            .unwrap();

        // Should have syntax error
        assert!(result.issues.iter().any(|i| i.rule_id == "E001"));
    }

    #[test]
    fn test_yaml_checker_missing_hosts() {
        let checker = YamlChecker::new();
        let content = r#"
- name: Play without hosts
  tasks:
    - name: Test task
      debug:
        msg: "Hello"
"#;
        let result = checker
            .check_content(content, Path::new("test.yml"), &LintConfig::new())
            .unwrap();

        // Should have missing hosts error
        assert!(result.issues.iter().any(|i| i.rule_id == "E005"));
    }
}
