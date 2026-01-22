//! Diff statistics module.
//!
//! Provides summary statistics for diff operations including line counts,
//! file counts, and various metrics.

use colored::Colorize;
use std::fmt;

/// Statistics about a diff operation
#[derive(Debug, Clone, Default)]
pub struct DiffStats {
    /// Number of files that were changed
    pub files_changed: usize,
    /// Number of lines inserted
    pub insertions: usize,
    /// Number of lines deleted
    pub deletions: usize,
    /// Number of context lines shown
    pub context_lines: usize,
    /// Number of hunks in the diff
    pub hunks: usize,
}

impl DiffStats {
    /// Create a new empty stats instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        self.insertions > 0 || self.deletions > 0
    }

    /// Get total number of changed lines
    pub fn total_changes(&self) -> usize {
        self.insertions + self.deletions
    }

    /// Get the net change (insertions - deletions)
    pub fn net_change(&self) -> isize {
        self.insertions as isize - self.deletions as isize
    }

    /// Merge statistics from another DiffStats instance
    pub fn merge(&mut self, other: &DiffStats) {
        self.files_changed += other.files_changed;
        self.insertions += other.insertions;
        self.deletions += other.deletions;
        self.context_lines += other.context_lines;
        self.hunks += other.hunks;
    }

    /// Format as a short summary string
    pub fn short_summary(&self) -> String {
        format!(
            "{} file(s), +{} -{}",
            self.files_changed, self.insertions, self.deletions
        )
    }

    /// Format as a colored short summary
    pub fn short_summary_colored(&self) -> String {
        format!(
            "{} file(s), {} {}",
            self.files_changed.to_string().bright_white(),
            format!("+{}", self.insertions).green(),
            format!("-{}", self.deletions).red()
        )
    }

    /// Format as a detailed summary with visual bar
    pub fn detailed_summary(&self, use_color: bool) -> String {
        let total = self.insertions + self.deletions;
        if total == 0 {
            return if use_color {
                "No changes".bright_black().to_string()
            } else {
                "No changes".to_string()
            };
        }

        // Create a visual bar (max 50 chars wide)
        let bar_width = 50.min(total);
        let insert_width = if total > 0 {
            (self.insertions * bar_width) / total
        } else {
            0
        };
        let delete_width = bar_width - insert_width;

        let insert_bar = "+".repeat(insert_width);
        let delete_bar = "-".repeat(delete_width);

        let bar = if use_color {
            format!("{}{}", insert_bar.green(), delete_bar.red())
        } else {
            format!("{}{}", insert_bar, delete_bar)
        };

        let stats_line = if use_color {
            format!(
                "{} file{} changed, {} insertion{}{}, {} deletion{}{}",
                self.files_changed.to_string().bright_white().bold(),
                if self.files_changed == 1 { "" } else { "s" },
                self.insertions.to_string().green().bold(),
                if self.insertions == 1 { "" } else { "s" },
                "(+)".green(),
                self.deletions.to_string().red().bold(),
                if self.deletions == 1 { "" } else { "s" },
                "(-)".red()
            )
        } else {
            format!(
                "{} file{} changed, {} insertion{}(+), {} deletion{}(-)",
                self.files_changed,
                if self.files_changed == 1 { "" } else { "s" },
                self.insertions,
                if self.insertions == 1 { "" } else { "s" },
                self.deletions,
                if self.deletions == 1 { "" } else { "s" }
            )
        };

        format!("{}\n{}", stats_line, bar)
    }

    /// Format as git-style stat summary
    pub fn git_style_summary(&self, filename: &str, use_color: bool, max_width: usize) -> String {
        let total = self.insertions + self.deletions;
        let bar_width = max_width.min(50).min(total);

        let insert_width = if total > 0 && bar_width > 0 {
            ((self.insertions * bar_width) / total).max(if self.insertions > 0 { 1 } else { 0 })
        } else {
            0
        };
        let delete_width = if total > 0 && bar_width > 0 {
            ((self.deletions * bar_width) / total).max(if self.deletions > 0 { 1 } else { 0 })
        } else {
            0
        };

        let insert_bar = "+".repeat(insert_width);
        let delete_bar = "-".repeat(delete_width);

        let change_str = format!(
            "{:>4}",
            if total > 999 {
                "+999".to_string()
            } else {
                format!("{}", total)
            }
        );

        if use_color {
            format!(
                " {} | {} {}{}",
                filename.bright_white(),
                change_str.bright_white(),
                insert_bar.green(),
                delete_bar.red()
            )
        } else {
            format!(
                " {} | {} {}{}",
                filename, change_str, insert_bar, delete_bar
            )
        }
    }
}

impl fmt::Display for DiffStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} file(s) changed, {} insertion(s)(+), {} deletion(s)(-)",
            self.files_changed, self.insertions, self.deletions
        )
    }
}

/// Accumulator for collecting stats across multiple diffs
#[derive(Debug, Clone, Default)]
pub struct DiffStatsAccumulator {
    /// All individual file stats
    file_stats: Vec<(String, DiffStats)>,
    /// Combined totals
    totals: DiffStats,
}

impl DiffStatsAccumulator {
    /// Create a new accumulator
    pub fn new() -> Self {
        Self::default()
    }

    /// Add stats for a file
    pub fn add(&mut self, filename: String, stats: DiffStats) {
        self.totals.merge(&stats);
        self.file_stats.push((filename, stats));
    }

    /// Get the total stats
    pub fn totals(&self) -> &DiffStats {
        &self.totals
    }

    /// Get all file stats
    pub fn file_stats(&self) -> &[(String, DiffStats)] {
        &self.file_stats
    }

    /// Check if any changes were recorded
    pub fn has_changes(&self) -> bool {
        self.totals.has_changes()
    }

    /// Format a git-style summary for all files
    pub fn full_summary(&self, use_color: bool) -> String {
        if self.file_stats.is_empty() {
            return if use_color {
                "No changes".bright_black().to_string()
            } else {
                "No changes".to_string()
            };
        }

        // Find the longest filename for alignment
        let max_filename_len = self
            .file_stats
            .iter()
            .map(|(name, _)| name.len())
            .max()
            .unwrap_or(10);

        let mut output = Vec::new();

        // Per-file stats
        for (filename, stats) in &self.file_stats {
            let padded_name = format!("{:width$}", filename, width = max_filename_len);
            output.push(stats.git_style_summary(&padded_name, use_color, 50));
        }

        // Totals
        output.push(String::new());
        output.push(self.totals.detailed_summary(use_color));

        output.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_default() {
        let stats = DiffStats::default();
        assert_eq!(stats.files_changed, 0);
        assert_eq!(stats.insertions, 0);
        assert_eq!(stats.deletions, 0);
        assert!(!stats.has_changes());
    }

    #[test]
    fn test_stats_has_changes() {
        let mut stats = DiffStats::default();
        assert!(!stats.has_changes());

        stats.insertions = 1;
        assert!(stats.has_changes());

        stats.insertions = 0;
        stats.deletions = 1;
        assert!(stats.has_changes());
    }

    #[test]
    fn test_stats_total_changes() {
        let mut stats = DiffStats::default();
        stats.insertions = 5;
        stats.deletions = 3;
        assert_eq!(stats.total_changes(), 8);
    }

    #[test]
    fn test_stats_net_change() {
        let mut stats = DiffStats::default();
        stats.insertions = 10;
        stats.deletions = 3;
        assert_eq!(stats.net_change(), 7);

        stats.insertions = 3;
        stats.deletions = 10;
        assert_eq!(stats.net_change(), -7);
    }

    #[test]
    fn test_stats_merge() {
        let mut stats1 = DiffStats {
            files_changed: 1,
            insertions: 5,
            deletions: 2,
            context_lines: 10,
            hunks: 2,
        };

        let stats2 = DiffStats {
            files_changed: 2,
            insertions: 3,
            deletions: 4,
            context_lines: 8,
            hunks: 1,
        };

        stats1.merge(&stats2);

        assert_eq!(stats1.files_changed, 3);
        assert_eq!(stats1.insertions, 8);
        assert_eq!(stats1.deletions, 6);
        assert_eq!(stats1.context_lines, 18);
        assert_eq!(stats1.hunks, 3);
    }

    #[test]
    fn test_stats_short_summary() {
        let stats = DiffStats {
            files_changed: 2,
            insertions: 10,
            deletions: 5,
            ..Default::default()
        };

        let summary = stats.short_summary();
        assert!(summary.contains("2 file(s)"));
        assert!(summary.contains("+10"));
        assert!(summary.contains("-5"));
    }

    #[test]
    fn test_accumulator() {
        let mut acc = DiffStatsAccumulator::new();

        acc.add(
            "file1.txt".to_string(),
            DiffStats {
                files_changed: 1,
                insertions: 5,
                deletions: 2,
                ..Default::default()
            },
        );

        acc.add(
            "file2.txt".to_string(),
            DiffStats {
                files_changed: 1,
                insertions: 3,
                deletions: 1,
                ..Default::default()
            },
        );

        assert!(acc.has_changes());
        assert_eq!(acc.totals().files_changed, 2);
        assert_eq!(acc.totals().insertions, 8);
        assert_eq!(acc.totals().deletions, 3);
        assert_eq!(acc.file_stats().len(), 2);
    }
}
