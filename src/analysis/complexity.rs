//! Complexity Analysis
//!
//! This module provides analysis of playbook complexity including cyclomatic complexity,
//! nesting depth, and maintainability metrics.

use super::{
    helpers, AnalysisCategory, AnalysisFinding, AnalysisResult, Severity, SourceLocation,
};
use crate::playbook::{Play, Playbook, Task};
use serde::{Deserialize, Serialize};

/// Complexity metrics for a playbook or component
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComplexityMetrics {
    /// Total number of plays
    pub play_count: usize,
    /// Total number of tasks
    pub task_count: usize,
    /// Total number of handlers
    pub handler_count: usize,
    /// Maximum nesting depth
    pub max_nesting_depth: usize,
    /// Average tasks per play
    pub avg_tasks_per_play: f64,
    /// Cyclomatic complexity estimate
    pub cyclomatic_complexity: usize,
    /// Number of conditionals
    pub conditional_count: usize,
    /// Number of loops
    pub loop_count: usize,
    /// Number of blocks
    pub block_count: usize,
    /// Number of variables defined
    pub variable_count: usize,
    /// Number of roles used
    pub role_count: usize,
    /// Maintainability index (0-100, higher is better)
    pub maintainability_index: f64,
    /// Per-play metrics
    pub play_metrics: Vec<PlayMetrics>,
}

/// Metrics for a single play
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayMetrics {
    /// Play name
    pub name: String,
    /// Number of tasks (including pre/post tasks)
    pub task_count: usize,
    /// Number of handlers
    pub handler_count: usize,
    /// Maximum nesting depth
    pub max_depth: usize,
    /// Cyclomatic complexity
    pub complexity: usize,
}

/// Complexity report with findings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityReport {
    /// Overall metrics
    pub metrics: ComplexityMetrics,
    /// Complexity-related findings
    pub findings: Vec<AnalysisFinding>,
}

/// Complexity analyzer
pub struct ComplexityAnalyzer {
    /// Maximum complexity threshold
    max_complexity: u32,
    /// Maximum nesting depth threshold
    max_nesting_depth: u32,
}

impl ComplexityAnalyzer {
    /// Create a new complexity analyzer with thresholds
    pub fn new(max_complexity: u32, max_nesting_depth: u32) -> Self {
        Self {
            max_complexity,
            max_nesting_depth,
        }
    }

    /// Analyze a playbook for complexity
    pub fn analyze(
        &self,
        playbook: &Playbook,
    ) -> AnalysisResult<(ComplexityMetrics, Vec<AnalysisFinding>)> {
        let mut metrics = ComplexityMetrics::default();
        let mut findings = Vec::new();
        let source_file = playbook
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());

        metrics.play_count = playbook.plays.len();

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            let play_metrics = self.analyze_play(play, play_idx, &source_file, &mut findings);

            metrics.task_count += play_metrics.task_count;
            metrics.handler_count += play_metrics.handler_count;
            metrics.max_nesting_depth = metrics.max_nesting_depth.max(play_metrics.max_depth);
            metrics.cyclomatic_complexity += play_metrics.complexity;
            metrics.role_count += play.roles.len();

            // Count conditionals and loops
            let all_tasks = helpers::get_all_tasks(play);
            for task in &all_tasks {
                if task.when.is_some() {
                    metrics.conditional_count += 1;
                }
                if task.loop_.is_some() || task.with_items.is_some() {
                    metrics.loop_count += 1;
                }
            }

            // Count blocks recursively
            metrics.block_count += self.count_blocks(&all_tasks);

            // Count variables
            metrics.variable_count += play.vars.len();
            for task in &all_tasks {
                metrics.variable_count += task.vars.len();
                if task.register.is_some() {
                    metrics.variable_count += 1;
                }
            }

            metrics.play_metrics.push(play_metrics);
        }

        // Calculate averages
        if metrics.play_count > 0 {
            metrics.avg_tasks_per_play = metrics.task_count as f64 / metrics.play_count as f64;
        }

        // Calculate maintainability index
        metrics.maintainability_index = self.calculate_maintainability_index(&metrics);

        // Check for overall complexity issues
        if metrics.cyclomatic_complexity > self.max_complexity as usize {
            findings.push(
                AnalysisFinding::new(
                    "CMPLX001",
                    AnalysisCategory::Complexity,
                    Severity::Warning,
                    format!(
                        "Playbook has high cyclomatic complexity ({} > threshold {})",
                        metrics.cyclomatic_complexity, self.max_complexity
                    ),
                )
                .with_description(
                    "High complexity makes the playbook harder to understand and maintain.",
                )
                .with_suggestion(
                    "Consider breaking the playbook into smaller roles or separate playbooks.",
                ),
            );
        }

        Ok((metrics, findings))
    }

    /// Analyze a single play
    fn analyze_play(
        &self,
        play: &Play,
        play_idx: usize,
        source_file: &Option<String>,
        findings: &mut Vec<AnalysisFinding>,
    ) -> PlayMetrics {
        let all_tasks = helpers::get_all_tasks(play);
        let task_count = all_tasks.len();
        let handler_count = play.handlers.len();

        // Calculate max depth
        let max_depth = self.calculate_max_depth(&all_tasks);

        // Calculate complexity
        let mut complexity = 1; // Base complexity
        for task in &all_tasks {
            if task.when.is_some() {
                complexity += 1;
            }
            if task.loop_.is_some() || task.with_items.is_some() {
                complexity += 1;
            }
            // Block adds complexity
            if task.block.as_deref().is_some_and(|block| !block.is_empty()) {
                complexity += 1;
                if task
                    .rescue
                    .as_deref()
                    .is_some_and(|rescue| !rescue.is_empty())
                {
                    complexity += 1;
                }
            }
        }

        // Check for issues
        if max_depth > self.max_nesting_depth as usize {
            let location = SourceLocation::new().with_play(play_idx, &play.name);
            let location = if let Some(f) = source_file {
                location.with_file(f.clone())
            } else {
                location
            };

            findings.push(
                AnalysisFinding::new(
                    "CMPLX002",
                    AnalysisCategory::Complexity,
                    Severity::Warning,
                    format!(
                        "Play '{}' has high nesting depth ({} > threshold {})",
                        play.name, max_depth, self.max_nesting_depth
                    ),
                )
                .with_location(location)
                .with_description("Deeply nested blocks make code harder to read and maintain.")
                .with_suggestion("Consider flattening the structure or extracting blocks to separate tasks."),
            );
        }

        if task_count > 50 {
            let location = SourceLocation::new().with_play(play_idx, &play.name);
            let location = if let Some(f) = source_file {
                location.with_file(f.clone())
            } else {
                location
            };

            findings.push(
                AnalysisFinding::new(
                    "CMPLX003",
                    AnalysisCategory::Complexity,
                    Severity::Info,
                    format!("Play '{}' has many tasks ({})", play.name, task_count),
                )
                .with_location(location)
                .with_description("Large plays can be difficult to maintain and debug.")
                .with_suggestion("Consider splitting into multiple plays or using roles."),
            );
        }

        PlayMetrics {
            name: play.name.clone(),
            task_count,
            handler_count,
            max_depth,
            complexity,
        }
    }

    /// Calculate maximum nesting depth
    fn calculate_max_depth(&self, tasks: &[&Task]) -> usize {
        let mut max_depth = 0;

        for task in tasks {
            let depth = self.task_depth(task, 0);
            max_depth = max_depth.max(depth);
        }

        max_depth
    }

    /// Calculate depth of a task (considering blocks)
    fn task_depth(&self, task: &Task, current_depth: usize) -> usize {
        let mut max = current_depth;

        if let Some(block) = task.block.as_deref() {
            for block_task in block {
                max = max.max(self.task_depth(block_task, current_depth + 1));
            }
        }

        if let Some(rescue) = task.rescue.as_deref() {
            for rescue_task in rescue {
                max = max.max(self.task_depth(rescue_task, current_depth + 1));
            }
        }

        if let Some(always) = task.always.as_deref() {
            for always_task in always {
                max = max.max(self.task_depth(always_task, current_depth + 1));
            }
        }

        max
    }

    /// Count blocks in tasks
    fn count_blocks(&self, tasks: &[&Task]) -> usize {
        let mut count = 0;

        for task in tasks {
            if let Some(block) = task.block.as_deref() {
                if !block.is_empty() {
                    count += 1;
                    count += self.count_blocks_recursive(block);
                }
            }
        }

        count
    }

    fn count_blocks_recursive(&self, tasks: &[Task]) -> usize {
        let mut count = 0;

        for task in tasks {
            if let Some(block) = task.block.as_deref() {
                if !block.is_empty() {
                    count += 1;
                    count += self.count_blocks_recursive(block);
                }
            }
            if let Some(rescue) = task.rescue.as_deref() {
                count += self.count_blocks_recursive(rescue);
            }
            if let Some(always) = task.always.as_deref() {
                count += self.count_blocks_recursive(always);
            }
        }

        count
    }

    /// Calculate maintainability index
    fn calculate_maintainability_index(&self, metrics: &ComplexityMetrics) -> f64 {
        // Simplified maintainability index based on:
        // - Cyclomatic complexity
        // - Nesting depth
        // - Number of tasks
        // Formula: 100 - (complexity_penalty + depth_penalty + size_penalty)

        let complexity_penalty = (metrics.cyclomatic_complexity as f64 * 2.0).min(40.0);
        let depth_penalty = (metrics.max_nesting_depth as f64 * 5.0).min(25.0);
        let size_penalty = (metrics.task_count as f64 * 0.2).min(25.0);

        let index = 100.0 - complexity_penalty - depth_penalty - size_penalty;
        index.clamp(0.0, 100.0)
    }
}

impl Default for ComplexityAnalyzer {
    fn default() -> Self {
        Self::new(10, 4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_maintainability_index_simple() {
        let analyzer = ComplexityAnalyzer::default();

        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 5,
            max_nesting_depth: 2,
            task_count: 10,
            ..Default::default()
        };

        let mi = analyzer.calculate_maintainability_index(&metrics);
        assert!(mi > 70.0); // Simple playbook should have high maintainability
    }

    #[test]
    fn test_maintainability_index_complex() {
        let analyzer = ComplexityAnalyzer::default();

        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 25,
            max_nesting_depth: 6,
            task_count: 100,
            ..Default::default()
        };

        let mi = analyzer.calculate_maintainability_index(&metrics);
        assert!(mi < 50.0); // Complex playbook should have lower maintainability
    }
}
