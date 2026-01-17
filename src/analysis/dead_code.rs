//! Dead Code Detection
//!
//! This module provides analysis to detect unused or unreachable code in playbooks,
//! including unused tasks, handlers, variables, and unreachable code paths.

use super::{
    helpers, AnalysisCategory, AnalysisFinding, AnalysisResult, Severity, SourceLocation,
};
use crate::playbook::{Playbook, Task, When};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Type of dead code detected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeadCodeType {
    /// Handler that is never notified
    UnusedHandler,
    /// Task that is always skipped (when: false)
    AlwaysSkippedTask,
    /// Task after unconditional fail/meta end_play
    UnreachableTask,
    /// Play with no tasks
    EmptyPlay,
    /// Duplicate task (same module, same args)
    DuplicateTask,
    /// Unused variable (set but never read)
    UnusedVariable,
    /// Dead conditional branch
    DeadBranch,
    /// Role that is never applied
    UnusedRole,
}

impl std::fmt::Display for DeadCodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeadCodeType::UnusedHandler => write!(f, "unused handler"),
            DeadCodeType::AlwaysSkippedTask => write!(f, "always skipped task"),
            DeadCodeType::UnreachableTask => write!(f, "unreachable task"),
            DeadCodeType::EmptyPlay => write!(f, "empty play"),
            DeadCodeType::DuplicateTask => write!(f, "duplicate task"),
            DeadCodeType::UnusedVariable => write!(f, "unused variable"),
            DeadCodeType::DeadBranch => write!(f, "dead branch"),
            DeadCodeType::UnusedRole => write!(f, "unused role"),
        }
    }
}

/// A dead code finding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadCodeFinding {
    /// Type of dead code
    pub dead_code_type: DeadCodeType,
    /// Name of the dead code element
    pub name: String,
    /// Location in source
    pub location: SourceLocation,
    /// Additional context
    pub context: Option<String>,
}

impl DeadCodeFinding {
    pub fn new(
        dead_code_type: DeadCodeType,
        name: impl Into<String>,
        location: SourceLocation,
    ) -> Self {
        Self {
            dead_code_type,
            name: name.into(),
            location,
            context: None,
        }
    }

    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Convert to a standard AnalysisFinding
    pub fn to_finding(&self) -> AnalysisFinding {
        let (rule_id, severity, message) = match self.dead_code_type {
            DeadCodeType::UnusedHandler => (
                "DEAD001",
                Severity::Warning,
                format!("Handler '{}' is never notified", self.name),
            ),
            DeadCodeType::AlwaysSkippedTask => (
                "DEAD002",
                Severity::Warning,
                format!("Task '{}' will always be skipped", self.name),
            ),
            DeadCodeType::UnreachableTask => (
                "DEAD003",
                Severity::Error,
                format!("Task '{}' is unreachable", self.name),
            ),
            DeadCodeType::EmptyPlay => (
                "DEAD004",
                Severity::Warning,
                format!("Play '{}' has no tasks", self.name),
            ),
            DeadCodeType::DuplicateTask => (
                "DEAD005",
                Severity::Info,
                format!("Task '{}' is a duplicate", self.name),
            ),
            DeadCodeType::UnusedVariable => (
                "DEAD006",
                Severity::Hint,
                format!("Variable '{}' is set but never used", self.name),
            ),
            DeadCodeType::DeadBranch => (
                "DEAD007",
                Severity::Warning,
                format!("Conditional branch '{}' is never reachable", self.name),
            ),
            DeadCodeType::UnusedRole => (
                "DEAD008",
                Severity::Info,
                format!("Role '{}' is defined but never applied", self.name),
            ),
        };

        let mut finding = AnalysisFinding::new(rule_id, AnalysisCategory::DeadCode, severity, message)
            .with_location(self.location.clone());

        if let Some(context) = &self.context {
            finding = finding.with_description(context.clone());
        }

        finding
    }
}

/// Dead code analyzer
pub struct DeadCodeAnalyzer {
    /// Whether to check for duplicate tasks
    check_duplicates: bool,
    /// Whether to check for always-false conditions
    check_conditions: bool,
}

impl DeadCodeAnalyzer {
    pub fn new() -> Self {
        Self {
            check_duplicates: true,
            check_conditions: true,
        }
    }

    /// Enable or disable duplicate task detection
    pub fn with_duplicate_detection(mut self, enabled: bool) -> Self {
        self.check_duplicates = enabled;
        self
    }

    /// Analyze a playbook for dead code
    pub fn analyze(&self, playbook: &Playbook) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();
        let source_file = playbook
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        // Check for unused handlers
        findings.extend(self.find_unused_handlers(playbook, &source_file)?);

        // Check for empty plays
        findings.extend(self.find_empty_plays(playbook, &source_file)?);

        // Check for always-skipped tasks
        if self.check_conditions {
            findings.extend(self.find_always_skipped_tasks(playbook, &source_file)?);
        }

        // Check for unreachable tasks
        findings.extend(self.find_unreachable_tasks(playbook, &source_file)?);

        // Check for duplicate tasks
        if self.check_duplicates {
            findings.extend(self.find_duplicate_tasks(playbook, &source_file)?);
        }

        Ok(findings)
    }

    /// Find handlers that are never notified
    fn find_unused_handlers(
        &self,
        playbook: &Playbook,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            // Collect all handler names and listen names
            let mut handler_names: HashMap<String, SourceLocation> = HashMap::new();
            for (handler_idx, handler) in play.handlers.iter().enumerate() {
                let location = SourceLocation::new()
                    .with_play(play_idx, &play.name)
                    .with_task(handler_idx, &handler.name);
                let location = if let Some(f) = source_file {
                    location.with_file(f.clone())
                } else {
                    location
                };

                handler_names.insert(handler.name.clone(), location.clone());
                for listen_name in &handler.listen {
                    handler_names.insert(listen_name.clone(), location.clone());
                }
            }

            // Collect all notified handlers
            let mut notified: HashSet<String> = HashSet::new();
            let all_tasks = helpers::get_all_tasks(play);
            for task in all_tasks {
                for notify in &task.notify {
                    notified.insert(notify.clone());
                }
            }

            // Find handlers that are never notified
            for (handler_name, location) in &handler_names {
                if !notified.contains(handler_name) {
                    findings.push(
                        DeadCodeFinding::new(
                            DeadCodeType::UnusedHandler,
                            handler_name,
                            location.clone(),
                        )
                        .with_context(
                            "This handler is defined but never notified by any task. \
                             It will never be executed."
                        )
                        .to_finding()
                        .with_suggestion(
                            "Either notify this handler from a task, or remove it."
                        ),
                    );
                }
            }
        }

        Ok(findings)
    }

    /// Find plays with no tasks
    fn find_empty_plays(
        &self,
        playbook: &Playbook,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            let has_tasks = !play.pre_tasks.is_empty()
                || !play.tasks.is_empty()
                || !play.post_tasks.is_empty()
                || !play.roles.is_empty();

            if !has_tasks {
                let location = SourceLocation::new().with_play(play_idx, &play.name);
                let location = if let Some(f) = source_file {
                    location.with_file(f.clone())
                } else {
                    location
                };

                findings.push(
                    DeadCodeFinding::new(DeadCodeType::EmptyPlay, &play.name, location)
                        .with_context(
                            "This play has no tasks, roles, pre_tasks, or post_tasks. \
                             It will only gather facts (if enabled) and do nothing else."
                        )
                        .to_finding()
                        .with_suggestion(
                            "Add tasks to this play or remove it if not needed."
                        ),
                );
            }
        }

        Ok(findings)
    }

    /// Find tasks that will always be skipped (when: false, when: False, etc.)
    fn find_always_skipped_tasks(
        &self,
        playbook: &Playbook,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            let all_tasks = helpers::get_all_tasks(play);
            for (task_idx, task) in all_tasks.iter().enumerate() {
                if let Some(when) = &task.when {
                    if self.is_always_false(when) {
                        let location = SourceLocation::new()
                            .with_play(play_idx, &play.name)
                            .with_task(task_idx, &task.name);
                        let location = if let Some(f) = source_file {
                            location.with_file(f.clone())
                        } else {
                            location
                        };

                        findings.push(
                            DeadCodeFinding::new(
                                DeadCodeType::AlwaysSkippedTask,
                                &task.name,
                                location,
                            )
                            .with_context(
                                "This task has a 'when' condition that is always false."
                            )
                            .to_finding()
                            .with_suggestion(
                                "Remove the task or fix the condition."
                            ),
                        );
                    }
                }
            }
        }

        Ok(findings)
    }

    /// Check if a when condition is always false
    fn is_always_false(&self, when: &When) -> bool {
        let conditions = when.conditions();
        for condition in conditions {
            let trimmed = condition.trim().to_lowercase();
            if trimmed == "false" || trimmed == "no" || trimmed == "0" {
                return true;
            }
        }
        false
    }

    /// Check if a when condition is always true
    fn is_always_true(&self, when: &When) -> bool {
        let conditions = when.conditions();
        conditions.iter().all(|c| {
            let trimmed = c.trim().to_lowercase();
            trimmed == "true" || trimmed == "yes" || trimmed == "1"
        })
    }

    /// Find tasks that are unreachable (after fail/end_play)
    fn find_unreachable_tasks(
        &self,
        playbook: &Playbook,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            // Check each task section separately
            findings.extend(self.check_task_reachability(
                &play.pre_tasks,
                play_idx,
                &play.name,
                "pre_tasks",
                source_file,
            )?);
            findings.extend(self.check_task_reachability(
                &play.tasks,
                play_idx,
                &play.name,
                "tasks",
                source_file,
            )?);
            findings.extend(self.check_task_reachability(
                &play.post_tasks,
                play_idx,
                &play.name,
                "post_tasks",
                source_file,
            )?);
        }

        Ok(findings)
    }

    /// Check task reachability within a task list
    fn check_task_reachability(
        &self,
        tasks: &[Task],
        play_idx: usize,
        play_name: &str,
        section: &str,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();
        let mut unreachable_after: Option<usize> = None;

        for (task_idx, task) in tasks.iter().enumerate() {
            // If we're after an unconditional terminator, all subsequent tasks are unreachable
            if let Some(term_idx) = unreachable_after {
                let location = SourceLocation::new()
                    .with_play(play_idx, play_name)
                    .with_task(task_idx, &task.name);
                let location = if let Some(f) = source_file {
                    location.with_file(f.clone())
                } else {
                    location
                };

                findings.push(
                    DeadCodeFinding::new(DeadCodeType::UnreachableTask, &task.name, location)
                        .with_context(format!(
                            "This task in {} is unreachable because task #{} unconditionally \
                             terminates the play.",
                            section,
                            term_idx + 1
                        ))
                        .to_finding()
                        .with_suggestion(
                            "Remove this task or move it before the terminating task."
                        ),
                );
                continue;
            }

            // Check if this task unconditionally terminates the play
            if self.is_unconditional_terminator(task) {
                unreachable_after = Some(task_idx);
            }
        }

        Ok(findings)
    }

    /// Check if a task unconditionally terminates the play
    fn is_unconditional_terminator(&self, task: &Task) -> bool {
        // Must not have a when condition (or have an always-true condition)
        let has_condition = task.when.as_ref().map(|w| !self.is_always_true(w)).unwrap_or(false);
        if has_condition {
            return false;
        }

        // Check for fail module
        if task.module.name == "fail" || task.module.name == "ansible.builtin.fail" {
            return true;
        }

        // Check for meta: end_play
        if task.module.name == "meta" || task.module.name == "ansible.builtin.meta" {
            if let Some(args) = task.module.args.as_str() {
                if args == "end_play" || args == "end_host" {
                    return true;
                }
            }
            if let Some(obj) = task.module.args.as_object() {
                if obj.get("msg").and_then(|v| v.as_str()) == Some("end_play")
                    || obj.get("msg").and_then(|v| v.as_str()) == Some("end_host")
                {
                    return true;
                }
            }
        }

        false
    }

    /// Find duplicate tasks (same module, same args, no conditions)
    fn find_duplicate_tasks(
        &self,
        playbook: &Playbook,
        source_file: &Option<String>,
    ) -> AnalysisResult<Vec<AnalysisFinding>> {
        let mut findings = Vec::new();

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            let all_tasks: Vec<_> = helpers::get_all_tasks(play);
            let mut seen: HashMap<String, (usize, String)> = HashMap::new();

            for (task_idx, task) in all_tasks.iter().enumerate() {
                // Skip tasks with conditions, loops, or delegates
                if task.when.is_some()
                    || task.loop_.is_some()
                    || task.with_items.is_some()
                    || task.delegate_to.is_some()
                    || task.run_once
                {
                    continue;
                }

                // Create a signature for the task
                let signature = format!(
                    "{}:{}",
                    task.module.name,
                    serde_json::to_string(&task.module.args).unwrap_or_default()
                );

                if let Some((prev_idx, prev_name)) = seen.get(&signature) {
                    let location = SourceLocation::new()
                        .with_play(play_idx, &play.name)
                        .with_task(task_idx, &task.name);
                    let location = if let Some(f) = source_file {
                        location.with_file(f.clone())
                    } else {
                        location
                    };

                    findings.push(
                        DeadCodeFinding::new(DeadCodeType::DuplicateTask, &task.name, location)
                            .with_context(format!(
                                "This task is identical to task #{} '{}'. \
                                 Consider removing the duplicate or adding conditions.",
                                prev_idx + 1,
                                prev_name
                            ))
                            .to_finding()
                            .with_suggestion(
                                "Remove the duplicate task or add distinguishing conditions."
                            ),
                    );
                } else {
                    seen.insert(signature, (task_idx, task.name.clone()));
                }
            }
        }

        Ok(findings)
    }
}

impl Default for DeadCodeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbook::When;

    #[test]
    fn test_is_always_false() {
        let analyzer = DeadCodeAnalyzer::new();

        assert!(analyzer.is_always_false(&When::Single("false".to_string())));
        assert!(analyzer.is_always_false(&When::Single("False".to_string())));
        assert!(analyzer.is_always_false(&When::Single("no".to_string())));
        assert!(!analyzer.is_always_false(&When::Single("true".to_string())));
        assert!(!analyzer.is_always_false(&When::Single("my_var".to_string())));
    }

    #[test]
    fn test_is_always_true() {
        let analyzer = DeadCodeAnalyzer::new();

        assert!(analyzer.is_always_true(&When::Single("true".to_string())));
        assert!(analyzer.is_always_true(&When::Single("True".to_string())));
        assert!(analyzer.is_always_true(&When::Single("yes".to_string())));
        assert!(!analyzer.is_always_true(&When::Single("false".to_string())));
        assert!(!analyzer.is_always_true(&When::Single("my_var".to_string())));
    }

    #[test]
    fn test_dead_code_type_display() {
        assert_eq!(format!("{}", DeadCodeType::UnusedHandler), "unused handler");
        assert_eq!(format!("{}", DeadCodeType::EmptyPlay), "empty play");
    }
}
