//! Enhanced diff module for Rustible
//!
//! This module provides advanced diff functionality including:
//! - Unified and side-by-side diff formats
//! - Colored terminal output
//! - Context-aware diffs
//! - Word-level diff highlighting
//! - Diff statistics
//!
//! # Example
//!
//! ```rust,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::diff::{DiffFormatter, DiffFormat, DiffOptions};
//!
//! let formatter = DiffFormatter::new(DiffOptions::default());
//! let diff = formatter.format("old content", "new content");
//! println!("{}", diff);
//! # Ok(())
//! # }
//! ```

mod formatter;
mod stats;
mod word_diff;

pub use formatter::{DiffFormat, DiffFormatter, DiffOptions};
pub use stats::{DiffStats, DiffStatsAccumulator};
pub use word_diff::{format_inline_diff, lines_are_similar, pair_similar_lines, WordDiff};

use similar::{ChangeTag, TextDiff};

/// Type of change in a diff line
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    /// Line was inserted
    Insert,
    /// Line was deleted
    Delete,
    /// Line is unchanged (context)
    Equal,
}

/// A single line in a diff
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// The content of the line
    pub content: String,
    /// The type of change
    pub change_type: ChangeType,
    /// Old line number (if applicable)
    pub old_line_num: Option<usize>,
    /// New line number (if applicable)
    pub new_line_num: Option<usize>,
}

/// A hunk (group of changes) in a diff
#[derive(Debug, Clone)]
pub struct DiffHunk {
    /// Starting line number in old file
    pub old_start: usize,
    /// Number of lines from old file
    pub old_count: usize,
    /// Starting line number in new file
    pub new_start: usize,
    /// Number of lines from new file
    pub new_count: usize,
    /// Lines in this hunk
    pub lines: Vec<DiffLine>,
}

/// Result of a diff operation
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Header for old file
    pub old_header: String,
    /// Header for new file
    pub new_header: String,
    /// All hunks in the diff
    pub hunks: Vec<DiffHunk>,
    /// Statistics about the diff
    pub stats: DiffStats,
}

impl DiffResult {
    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        self.stats.has_changes()
    }
}

/// Generate a diff between two strings
pub fn generate_diff(
    old: &str,
    new: &str,
    old_label: Option<&str>,
    new_label: Option<&str>,
    context_lines: usize,
) -> DiffResult {
    let text_diff = TextDiff::from_lines(old, new);
    let unified = text_diff.unified_diff();

    let old_header = format!("--- {}", old_label.unwrap_or("a"));
    let new_header = format!("+++ {}", new_label.unwrap_or("b"));

    let mut hunks = Vec::new();
    let mut stats = DiffStats::new();
    stats.files_changed = 1;

    for hunk in unified.iter_hunks() {
        let ops = hunk.ops();
        let (old_start, old_end, new_start, new_end) =
            if let (Some(first), Some(last)) = (ops.first(), ops.last()) {
                (
                    first.old_range().start,
                    last.old_range().end,
                    first.new_range().start,
                    last.new_range().end,
                )
            } else {
                (0, 0, 0, 0)
            };

        let mut diff_hunk = DiffHunk {
            old_start: old_start + 1,
            old_count: old_end.saturating_sub(old_start),
            new_start: new_start + 1,
            new_count: new_end.saturating_sub(new_start),
            lines: Vec::new(),
        };

        let mut old_line = diff_hunk.old_start;
        let mut new_line = diff_hunk.new_start;

        for change in hunk.iter_changes() {
            let (change_type, old_num, new_num) = match change.tag() {
                ChangeTag::Delete => {
                    stats.deletions += 1;
                    let num = old_line;
                    old_line += 1;
                    (ChangeType::Delete, Some(num), None)
                }
                ChangeTag::Insert => {
                    stats.insertions += 1;
                    let num = new_line;
                    new_line += 1;
                    (ChangeType::Insert, None, Some(num))
                }
                ChangeTag::Equal => {
                    stats.context_lines += 1;
                    let old_num = old_line;
                    let new_num = new_line;
                    old_line += 1;
                    new_line += 1;
                    (ChangeType::Equal, Some(old_num), Some(new_num))
                }
            };

            diff_hunk.lines.push(DiffLine {
                content: change.value().to_string(),
                change_type,
                old_line_num: old_num,
                new_line_num: new_num,
            });
        }

        stats.hunks += 1;
        hunks.push(diff_hunk);
    }

    DiffResult {
        old_header,
        new_header,
        hunks,
        stats,
    }
}

/// Quick helper to generate a unified diff string
pub fn unified_diff(before: &str, after: &str, context_lines: usize) -> String {
    let formatter = DiffFormatter::new(
        DiffOptions::default()
            .with_context_lines(context_lines)
            .with_format(DiffFormat::Unified),
    );
    formatter.format(before, after)
}

/// Quick helper to generate a side-by-side diff string
pub fn side_by_side_diff(before: &str, after: &str, width: usize) -> String {
    let formatter = DiffFormatter::new(
        DiffOptions::default()
            .with_format(DiffFormat::SideBySide)
            .with_max_width(width),
    );
    formatter.format(before, after)
}

/// Quick helper to compute diff statistics
pub fn compute_stats(before: &str, after: &str) -> DiffStats {
    let diff = TextDiff::from_lines(before, after);
    let mut stats = DiffStats::new();
    stats.files_changed = 1;

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Insert => stats.insertions += 1,
            ChangeTag::Delete => stats.deletions += 1,
            ChangeTag::Equal => {}
        }
    }

    // Count hunks
    let unified = diff.unified_diff();
    stats.hunks = unified.iter_hunks().count();

    stats
}

/// Check if two strings have any differences
pub fn has_changes(before: &str, after: &str) -> bool {
    before != after
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_stats() {
        let before = "line1\nline2\nline3\n";
        let after = "line1\nmodified\nline3\n";

        let stats = compute_stats(before, after);
        assert!(stats.has_changes());
        assert_eq!(stats.insertions, 1);
        assert_eq!(stats.deletions, 1);
    }

    #[test]
    fn test_no_changes() {
        let content = "same content\n";
        let stats = compute_stats(content, content);
        assert!(!stats.has_changes());
    }

    #[test]
    fn test_has_changes() {
        assert!(has_changes("old", "new"));
        assert!(!has_changes("same", "same"));
    }

    #[test]
    fn test_unified_diff() {
        let before = "line1\nline2\n";
        let after = "line1\nmodified\n";
        let diff = unified_diff(before, after, 3);
        assert!(diff.contains("-line2"));
        assert!(diff.contains("+modified"));
    }

    #[test]
    fn test_generate_diff() {
        let result = generate_diff("old\n", "new\n", Some("old.txt"), Some("new.txt"), 3);
        assert!(result.has_changes());
        assert_eq!(result.stats.insertions, 1);
        assert_eq!(result.stats.deletions, 1);
        assert!(!result.hunks.is_empty());
    }
}
