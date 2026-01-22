//! Diff formatter for various output formats.
//!
//! This module provides formatting functionality for diffs including:
//! - Unified diff format (default)
//! - Side-by-side diff format
//! - Inline word-level diff highlighting

use colored::Colorize;
use similar::TextDiff;

use super::stats::DiffStats;
use super::word_diff::{lines_are_similar, pair_similar_lines, WordDiff};
use super::{generate_diff, ChangeType, DiffHunk, DiffLine};

/// Diff output format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiffFormat {
    /// Unified diff format (default)
    #[default]
    Unified,
    /// Side-by-side diff format
    SideBySide,
    /// Context diff format
    Context,
    /// Minimal output (just stats)
    Minimal,
}

impl std::str::FromStr for DiffFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "unified" | "u" => Ok(DiffFormat::Unified),
            "side-by-side" | "sidebyside" | "side" | "s" => Ok(DiffFormat::SideBySide),
            "context" | "c" => Ok(DiffFormat::Context),
            "minimal" | "m" => Ok(DiffFormat::Minimal),
            _ => Err(format!(
                "Unknown diff format: {}. Valid options: unified, side-by-side, context, minimal",
                s
            )),
        }
    }
}

/// Options for diff formatting
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Number of context lines to show around changes
    pub context_lines: usize,
    /// Whether to use color output
    pub use_color: bool,
    /// Whether to show line numbers
    pub show_line_numbers: bool,
    /// Maximum width for side-by-side display (0 = auto)
    pub max_width: usize,
    /// Output format
    pub format: DiffFormat,
    /// Whether to use word-level diff for similar lines
    pub word_diff: bool,
    /// Tab width for alignment
    pub tab_width: usize,
    /// Whether to show file headers
    pub show_headers: bool,
    /// Whether to show statistics summary
    pub show_stats: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            context_lines: 3,
            use_color: true,
            show_line_numbers: true,
            max_width: 0,
            format: DiffFormat::Unified,
            word_diff: true,
            tab_width: 4,
            show_headers: true,
            show_stats: true,
        }
    }
}

impl DiffOptions {
    /// Create new options with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of context lines
    pub fn with_context_lines(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }

    /// Enable or disable color
    pub fn with_color(mut self, use_color: bool) -> Self {
        self.use_color = use_color;
        self
    }

    /// Enable or disable line numbers
    pub fn with_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    /// Set the output format
    pub fn with_format(mut self, format: DiffFormat) -> Self {
        self.format = format;
        self
    }

    /// Enable or disable word-level diff
    pub fn with_word_diff(mut self, enabled: bool) -> Self {
        self.word_diff = enabled;
        self
    }

    /// Set maximum width for side-by-side display
    pub fn with_max_width(mut self, width: usize) -> Self {
        self.max_width = width;
        self
    }

    /// Enable or disable headers
    pub fn with_headers(mut self, show: bool) -> Self {
        self.show_headers = show;
        self
    }

    /// Enable or disable statistics
    pub fn with_stats(mut self, show: bool) -> Self {
        self.show_stats = show;
        self
    }
}

/// Diff formatter for generating formatted diff output
#[derive(Debug, Clone)]
pub struct DiffFormatter {
    options: DiffOptions,
}

impl DiffFormatter {
    /// Create a new formatter with the given options
    pub fn new(options: DiffOptions) -> Self {
        Self { options }
    }

    /// Create a formatter with default options
    pub fn default_formatter() -> Self {
        Self::new(DiffOptions::default())
    }

    /// Format a diff between two strings
    pub fn format(&self, old: &str, new: &str) -> String {
        self.format_with_labels(old, new, None, None)
    }

    /// Format a diff with custom labels
    pub fn format_with_labels(
        &self,
        old: &str,
        new: &str,
        old_label: Option<&str>,
        new_label: Option<&str>,
    ) -> String {
        match self.options.format {
            DiffFormat::Unified => self.format_unified(old, new, old_label, new_label),
            DiffFormat::SideBySide => self.format_side_by_side(old, new, old_label, new_label),
            DiffFormat::Context => self.format_context(old, new, old_label, new_label),
            DiffFormat::Minimal => self.format_minimal(old, new),
        }
    }

    /// Format as unified diff
    fn format_unified(
        &self,
        old: &str,
        new: &str,
        old_label: Option<&str>,
        new_label: Option<&str>,
    ) -> String {
        let diff_result = generate_diff(old, new, old_label, new_label, self.options.context_lines);

        if !diff_result.has_changes() {
            return String::new();
        }

        let mut output = Vec::new();

        // Headers
        if self.options.show_headers {
            if self.options.use_color {
                output.push(diff_result.old_header.red().bold().to_string());
                output.push(diff_result.new_header.green().bold().to_string());
            } else {
                output.push(diff_result.old_header.clone());
                output.push(diff_result.new_header.clone());
            }
        }

        // Hunks
        for hunk in &diff_result.hunks {
            output.push(self.format_hunk_header(hunk));
            output.extend(self.format_hunk_lines(hunk));
        }

        // Statistics
        if self.options.show_stats {
            output.push(String::new());
            output.push(diff_result.stats.detailed_summary(self.options.use_color));
        }

        output.join("\n")
    }

    /// Format hunk header
    fn format_hunk_header(&self, hunk: &DiffHunk) -> String {
        let header = format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        );

        if self.options.use_color {
            header.cyan().to_string()
        } else {
            header
        }
    }

    /// Format hunk lines with optional word-level diff
    fn format_hunk_lines(&self, hunk: &DiffHunk) -> Vec<String> {
        let mut output = Vec::new();

        // Collect consecutive changes for word-level diffing
        let mut pending_deletes: Vec<(usize, &DiffLine)> = Vec::new();
        let mut pending_inserts: Vec<(usize, &DiffLine)> = Vec::new();

        for (i, line) in hunk.lines.iter().enumerate() {
            match line.change_type {
                ChangeType::Delete => {
                    pending_deletes.push((i, line));
                }
                ChangeType::Insert => {
                    pending_inserts.push((i, line));
                }
                ChangeType::Equal => {
                    // Flush pending changes with word-level diff
                    if !pending_deletes.is_empty() || !pending_inserts.is_empty() {
                        output.extend(self.format_change_group(&pending_deletes, &pending_inserts));
                        pending_deletes.clear();
                        pending_inserts.clear();
                    }

                    // Format context line
                    output.push(self.format_line(line));
                }
            }
        }

        // Flush remaining changes
        if !pending_deletes.is_empty() || !pending_inserts.is_empty() {
            output.extend(self.format_change_group(&pending_deletes, &pending_inserts));
        }

        output
    }

    /// Format a group of changes, optionally with word-level diff
    fn format_change_group(
        &self,
        deletes: &[(usize, &DiffLine)],
        inserts: &[(usize, &DiffLine)],
    ) -> Vec<String> {
        let mut output = Vec::new();

        if !self.options.word_diff || deletes.is_empty() || inserts.is_empty() {
            // Simple output without word-level diff
            for (_, line) in deletes {
                output.push(self.format_line(line));
            }
            for (_, line) in inserts {
                output.push(self.format_line(line));
            }
            return output;
        }

        // Try to pair similar lines for word-level diff
        let del_strs: Vec<&str> = deletes.iter().map(|(_, l)| l.content.as_str()).collect();
        let ins_strs: Vec<&str> = inserts.iter().map(|(_, l)| l.content.as_str()).collect();

        let pairs = pair_similar_lines(&del_strs, &ins_strs);

        for (del_opt, ins_opt) in pairs {
            match (del_opt, ins_opt) {
                (Some(del), Some(ins)) if lines_are_similar(del, ins) => {
                    // Word-level diff for similar lines
                    let word_diff =
                        WordDiff::new(del.trim_end(), ins.trim_end(), self.options.use_color);

                    if self.options.use_color {
                        output.push(format!("{}{}", "-".red(), word_diff.old_highlighted));
                        output.push(format!("{}{}", "+".green(), word_diff.new_highlighted));
                    } else {
                        output.push(format!("-{}", word_diff.old_highlighted));
                        output.push(format!("+{}", word_diff.new_highlighted));
                    }
                }
                (Some(del), Some(ins)) => {
                    // Not similar enough, just show normal diff
                    output.push(self.format_delete_line(del));
                    output.push(self.format_insert_line(ins));
                }
                (Some(del), None) => {
                    output.push(self.format_delete_line(del));
                }
                (None, Some(ins)) => {
                    output.push(self.format_insert_line(ins));
                }
                (None, None) => {}
            }
        }

        output
    }

    /// Format a single diff line
    fn format_line(&self, line: &DiffLine) -> String {
        let content = line.content.trim_end();

        match line.change_type {
            ChangeType::Delete => self.format_delete_line(content),
            ChangeType::Insert => self.format_insert_line(content),
            ChangeType::Equal => format!(" {}", content),
        }
    }

    /// Format a delete line
    fn format_delete_line(&self, content: &str) -> String {
        if self.options.use_color {
            format!("{}{}", "-".red(), content.red())
        } else {
            format!("-{}", content)
        }
    }

    /// Format an insert line
    fn format_insert_line(&self, content: &str) -> String {
        if self.options.use_color {
            format!("{}{}", "+".green(), content.green())
        } else {
            format!("+{}", content)
        }
    }

    /// Format as side-by-side diff
    fn format_side_by_side(
        &self,
        old: &str,
        new: &str,
        old_label: Option<&str>,
        new_label: Option<&str>,
    ) -> String {
        let terminal_width = self.options.max_width.max(80);
        let half_width = (terminal_width - 3) / 2; // -3 for separator " | "

        let old_lines: Vec<&str> = old.lines().collect();
        let new_lines: Vec<&str> = new.lines().collect();

        let text_diff = TextDiff::from_lines(old, new);
        let mut output = Vec::new();

        // Headers
        if self.options.show_headers {
            let old_header = old_label.unwrap_or("old");
            let new_header = new_label.unwrap_or("new");

            let header = format!(
                "{:^width$} | {:^width$}",
                old_header,
                new_header,
                width = half_width
            );

            if self.options.use_color {
                output.push(header.bright_white().bold().to_string());
            } else {
                output.push(header);
            }

            output.push("-".repeat(terminal_width));
        }

        // Process changes
        let mut new_idx = 0;
        let mut stats = DiffStats::default();

        for group in text_diff.grouped_ops(self.options.context_lines) {
            for op in group {
                match op.tag() {
                    similar::DiffTag::Equal => {
                        for i in op.old_range() {
                            let left =
                                self.truncate_line(old_lines.get(i).unwrap_or(&""), half_width);
                            let right = self.truncate_line(
                                new_lines
                                    .get(new_idx + (i - op.old_range().start))
                                    .unwrap_or(&""),
                                half_width,
                            );

                            let line =
                                format!("{:width$} | {:width$}", left, right, width = half_width);
                            output.push(line);
                        }
                        new_idx += op.old_range().len();
                    }
                    similar::DiffTag::Delete => {
                        for i in op.old_range() {
                            let left =
                                self.truncate_line(old_lines.get(i).unwrap_or(&""), half_width);
                            stats.deletions += 1;

                            let line = if self.options.use_color {
                                format!(
                                    "{:width$} | {:width$}",
                                    left.to_string().red(),
                                    "",
                                    width = half_width
                                )
                            } else {
                                format!("{:width$} < {:width$}", left, "", width = half_width)
                            };
                            output.push(line);
                        }
                    }
                    similar::DiffTag::Insert => {
                        for i in op.new_range() {
                            let right =
                                self.truncate_line(new_lines.get(i).unwrap_or(&""), half_width);
                            stats.insertions += 1;

                            let line = if self.options.use_color {
                                format!(
                                    "{:width$} | {:width$}",
                                    "",
                                    right.to_string().green(),
                                    width = half_width
                                )
                            } else {
                                format!("{:width$} > {:width$}", "", right, width = half_width)
                            };
                            output.push(line);
                        }
                    }
                    similar::DiffTag::Replace => {
                        let old_range = op.old_range();
                        let new_range = op.new_range();
                        let max_len = old_range.len().max(new_range.len());

                        for i in 0..max_len {
                            let left = if i < old_range.len() {
                                stats.deletions += 1;
                                self.truncate_line(
                                    old_lines.get(old_range.start + i).unwrap_or(&""),
                                    half_width,
                                )
                            } else {
                                String::new()
                            };

                            let right = if i < new_range.len() {
                                stats.insertions += 1;
                                self.truncate_line(
                                    new_lines.get(new_range.start + i).unwrap_or(&""),
                                    half_width,
                                )
                            } else {
                                String::new()
                            };

                            let line = if self.options.use_color {
                                format!(
                                    "{:width$} | {:width$}",
                                    if !left.is_empty() {
                                        left.red().to_string()
                                    } else {
                                        left
                                    },
                                    if !right.is_empty() {
                                        right.green().to_string()
                                    } else {
                                        right
                                    },
                                    width = half_width
                                )
                            } else {
                                format!("{:width$} | {:width$}", left, right, width = half_width)
                            };
                            output.push(line);
                        }
                    }
                }
            }
        }

        // Statistics
        if self.options.show_stats && stats.has_changes() {
            output.push(String::new());
            stats.files_changed = 1;
            output.push(stats.detailed_summary(self.options.use_color));
        }

        output.join("\n")
    }

    /// Truncate a line to fit within width
    fn truncate_line(&self, line: &str, max_width: usize) -> String {
        let line = line.trim_end();
        if line.len() <= max_width {
            line.to_string()
        } else if max_width > 3 {
            format!("{}...", &line[..max_width - 3])
        } else {
            line[..max_width].to_string()
        }
    }

    /// Format as context diff
    fn format_context(
        &self,
        old: &str,
        new: &str,
        old_label: Option<&str>,
        new_label: Option<&str>,
    ) -> String {
        // Context diff format (similar to diff -c)
        let diff_result = generate_diff(old, new, old_label, new_label, self.options.context_lines);

        if !diff_result.has_changes() {
            return String::new();
        }

        let mut output = Vec::new();

        // Headers
        if self.options.show_headers {
            let old_header = format!("*** {}", old_label.unwrap_or("old"));
            let new_header = format!("--- {}", new_label.unwrap_or("new"));

            if self.options.use_color {
                output.push(old_header.red().bold().to_string());
                output.push(new_header.green().bold().to_string());
            } else {
                output.push(old_header);
                output.push(new_header);
            }
        }

        output.push("***************".to_string());

        for hunk in &diff_result.hunks {
            // Old section
            let old_range = format!(
                "*** {},{} ****",
                hunk.old_start,
                hunk.old_start + hunk.old_count - 1
            );
            if self.options.use_color {
                output.push(old_range.red().to_string());
            } else {
                output.push(old_range);
            }

            for line in &hunk.lines {
                match line.change_type {
                    ChangeType::Delete => {
                        let prefix = if self.options.use_color {
                            "- ".red().to_string()
                        } else {
                            "- ".to_string()
                        };
                        output.push(format!("{}{}", prefix, line.content.trim_end()));
                    }
                    ChangeType::Equal => {
                        output.push(format!("  {}", line.content.trim_end()));
                    }
                    _ => {}
                }
            }

            // New section
            let new_range = format!(
                "--- {},{} ----",
                hunk.new_start,
                hunk.new_start + hunk.new_count - 1
            );
            if self.options.use_color {
                output.push(new_range.green().to_string());
            } else {
                output.push(new_range);
            }

            for line in &hunk.lines {
                match line.change_type {
                    ChangeType::Insert => {
                        let prefix = if self.options.use_color {
                            "+ ".green().to_string()
                        } else {
                            "+ ".to_string()
                        };
                        output.push(format!("{}{}", prefix, line.content.trim_end()));
                    }
                    ChangeType::Equal => {
                        output.push(format!("  {}", line.content.trim_end()));
                    }
                    _ => {}
                }
            }
        }

        // Statistics
        if self.options.show_stats {
            output.push(String::new());
            output.push(diff_result.stats.detailed_summary(self.options.use_color));
        }

        output.join("\n")
    }

    /// Format as minimal output (stats only)
    fn format_minimal(&self, old: &str, new: &str) -> String {
        let diff_result = generate_diff(old, new, None, None, 0);

        if !diff_result.has_changes() {
            return if self.options.use_color {
                "No changes".bright_black().to_string()
            } else {
                "No changes".to_string()
            };
        }

        diff_result.stats.short_summary_colored()
    }
}

impl Default for DiffFormatter {
    fn default() -> Self {
        Self::new(DiffOptions::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_unified() {
        let formatter = DiffFormatter::new(DiffOptions::default().with_color(false));
        let output = formatter.format("line1\nline2\n", "line1\nmodified\n");

        assert!(output.contains("--- a"));
        assert!(output.contains("+++ b"));
        assert!(output.contains("-line2"));
        assert!(output.contains("+modified"));
    }

    #[test]
    fn test_format_no_changes() {
        let formatter = DiffFormatter::default_formatter();
        let output = formatter.format("same\n", "same\n");
        assert!(output.is_empty());
    }

    #[test]
    fn test_format_side_by_side() {
        let options = DiffOptions::default()
            .with_format(DiffFormat::SideBySide)
            .with_color(false)
            .with_max_width(80);

        let formatter = DiffFormatter::new(options);
        let output = formatter.format("old line\n", "new line\n");

        assert!(output.contains("|"));
    }

    #[test]
    fn test_format_minimal() {
        let options = DiffOptions::default()
            .with_format(DiffFormat::Minimal)
            .with_color(false);

        let formatter = DiffFormatter::new(options);
        let output = formatter.format("line1\nline2\n", "line1\nmodified\nline3\n");

        assert!(output.contains("file"));
        assert!(output.contains("+"));
        assert!(output.contains("-"));
    }

    #[test]
    fn test_format_with_labels() {
        let formatter = DiffFormatter::new(DiffOptions::default().with_color(false));
        let output =
            formatter.format_with_labels("old\n", "new\n", Some("old.txt"), Some("new.txt"));

        assert!(output.contains("--- old.txt"));
        assert!(output.contains("+++ new.txt"));
    }

    #[test]
    fn test_diff_format_from_str() {
        assert_eq!(
            "unified".parse::<DiffFormat>().unwrap(),
            DiffFormat::Unified
        );
        assert_eq!(
            "side-by-side".parse::<DiffFormat>().unwrap(),
            DiffFormat::SideBySide
        );
        assert_eq!(
            "context".parse::<DiffFormat>().unwrap(),
            DiffFormat::Context
        );
        assert_eq!(
            "minimal".parse::<DiffFormat>().unwrap(),
            DiffFormat::Minimal
        );
        assert!("invalid".parse::<DiffFormat>().is_err());
    }
}
