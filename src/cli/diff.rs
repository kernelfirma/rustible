//! Colorized diff output module for Rustible
//!
//! Provides colorized diff display for file changes using the similar crate.

use colored::Colorize;
use similar::{ChangeTag, DiffOp, TextDiff};
use std::fmt::Write;

/// Extract hunk range information from diff operations
/// Returns (old_start, old_len, new_start, new_len) in 1-based line numbers for display
fn hunk_ranges(ops: &[DiffOp]) -> (usize, usize, usize, usize) {
    if ops.is_empty() {
        return (1, 0, 1, 0);
    }
    let first = &ops[0];
    let last = &ops[ops.len() - 1];
    let old_start = first.old_range().start;
    let new_start = first.new_range().start;
    let old_end = last.old_range().end;
    let new_end = last.new_range().end;
    // Convert to 1-based line numbers for display
    let old_display_start = old_start + 1;
    let new_display_start = new_start + 1;
    let old_len = old_end.saturating_sub(old_start);
    let new_len = new_end.saturating_sub(new_start);
    (old_display_start, old_len, new_display_start, new_len)
}

/// Diff output format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffFormat {
    /// Unified diff format (default)
    Unified,
    /// Side-by-side format
    SideBySide,
    /// Context diff format
    Context,
    /// Inline format with markers
    Inline,
}

impl Default for DiffFormat {
    fn default() -> Self {
        Self::Unified
    }
}

/// Diff display options
#[derive(Debug, Clone)]
pub struct DiffOptions {
    /// Number of context lines to show
    pub context_lines: usize,
    /// Use colors
    pub use_color: bool,
    /// Diff format
    pub format: DiffFormat,
    /// Show line numbers
    pub show_line_numbers: bool,
    /// Tab width for display
    pub tab_width: usize,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            context_lines: 3,
            use_color: true,
            format: DiffFormat::Unified,
            show_line_numbers: true,
            tab_width: 4,
        }
    }
}

/// Colorized diff generator
pub struct ColorizedDiff {
    options: DiffOptions,
}

impl ColorizedDiff {
    /// Create a new colorized diff with default options
    pub fn new() -> Self {
        Self {
            options: DiffOptions::default(),
        }
    }

    /// Create a new colorized diff with custom options
    pub fn with_options(options: DiffOptions) -> Self {
        Self { options }
    }

    /// Generate a colorized diff between two strings
    pub fn diff(&self, old: &str, new: &str, old_name: &str, new_name: &str) -> String {
        let diff = TextDiff::from_lines(old, new);

        match self.options.format {
            DiffFormat::Unified => self.format_unified(&diff, old_name, new_name),
            DiffFormat::SideBySide => self.format_side_by_side(&diff),
            DiffFormat::Context => self.format_context(&diff, old_name, new_name),
            DiffFormat::Inline => self.format_inline(&diff),
        }
    }

    /// Format as unified diff
    fn format_unified<'a>(
        &self,
        diff: &TextDiff<'a, 'a, 'a, str>,
        old_name: &str,
        new_name: &str,
    ) -> String {
        let mut output = String::new();

        // Header
        if self.options.use_color {
            writeln!(output, "{}", format!("--- {}", old_name).red()).unwrap();
            writeln!(output, "{}", format!("+++ {}", new_name).green()).unwrap();
        } else {
            writeln!(output, "--- {}", old_name).unwrap();
            writeln!(output, "+++ {}", new_name).unwrap();
        }

        // Get unified diff with context
        for hunk in diff
            .unified_diff()
            .context_radius(self.options.context_lines)
            .iter_hunks()
        {
            // Hunk header - extract ranges from operations
            let (old_start, old_len, new_start, new_len) = hunk_ranges(hunk.ops());
            let header = format!(
                "@@ -{},{} +{},{} @@",
                old_start, old_len, new_start, new_len
            );

            if self.options.use_color {
                writeln!(output, "{}", header.cyan()).unwrap();
            } else {
                writeln!(output, "{}", header).unwrap();
            }

            // Changes
            for change in hunk.iter_changes() {
                let line = change.value();
                let line_display = if line.ends_with('\n') {
                    line.to_string()
                } else {
                    format!("{}\n\\ No newline at end of file\n", line)
                };

                match change.tag() {
                    ChangeTag::Delete => {
                        if self.options.use_color {
                            write!(output, "{}", format!("-{}", line_display).red()).unwrap();
                        } else {
                            write!(output, "-{}", line_display).unwrap();
                        }
                    }
                    ChangeTag::Insert => {
                        if self.options.use_color {
                            write!(output, "{}", format!("+{}", line_display).green()).unwrap();
                        } else {
                            write!(output, "+{}", line_display).unwrap();
                        }
                    }
                    ChangeTag::Equal => {
                        if self.options.use_color {
                            write!(output, "{}", format!(" {}", line_display).dimmed()).unwrap();
                        } else {
                            write!(output, " {}", line_display).unwrap();
                        }
                    }
                }
            }
        }

        output
    }

    /// Format as side-by-side diff
    fn format_side_by_side<'a>(&self, diff: &TextDiff<'a, 'a, 'a, str>) -> String {
        let mut output = String::new();
        let width = 40; // Width of each side

        for change in diff.iter_all_changes() {
            let line = change.value().trim_end();
            let truncated = truncate(line, width);

            match change.tag() {
                ChangeTag::Delete => {
                    let left = format!("{:<width$}", truncated, width = width);
                    let right = format!("{:<width$}", "", width = width);
                    if self.options.use_color {
                        writeln!(output, "{} | {}", left.red(), right).unwrap();
                    } else {
                        writeln!(output, "{} | {}", left, right).unwrap();
                    }
                }
                ChangeTag::Insert => {
                    let left = format!("{:<width$}", "", width = width);
                    let right = format!("{:<width$}", truncated, width = width);
                    if self.options.use_color {
                        writeln!(output, "{} | {}", left, right.green()).unwrap();
                    } else {
                        writeln!(output, "{} | {}", left, right).unwrap();
                    }
                }
                ChangeTag::Equal => {
                    let formatted = format!("{:<width$}", truncated, width = width);
                    if self.options.use_color {
                        writeln!(output, "{} | {}", formatted.dimmed(), formatted.dimmed())
                            .unwrap();
                    } else {
                        writeln!(output, "{} | {}", formatted, formatted).unwrap();
                    }
                }
            }
        }

        output
    }

    /// Format as context diff
    fn format_context<'a>(
        &self,
        diff: &TextDiff<'a, 'a, 'a, str>,
        old_name: &str,
        new_name: &str,
    ) -> String {
        let mut output = String::new();

        // Header
        if self.options.use_color {
            writeln!(output, "{}", format!("*** {}", old_name).red()).unwrap();
            writeln!(output, "{}", format!("--- {}", new_name).green()).unwrap();
        } else {
            writeln!(output, "*** {}", old_name).unwrap();
            writeln!(output, "--- {}", new_name).unwrap();
        }

        for hunk in diff
            .unified_diff()
            .context_radius(self.options.context_lines)
            .iter_hunks()
        {
            // Old section - extract ranges from operations
            let (old_start, old_len, new_start, new_len) = hunk_ranges(hunk.ops());
            let old_header = format!("*** {},{} ****", old_start, old_start + old_len);
            if self.options.use_color {
                writeln!(output, "{}", old_header.yellow()).unwrap();
            } else {
                writeln!(output, "{}", old_header).unwrap();
            }

            for change in hunk.iter_changes() {
                if change.tag() != ChangeTag::Insert {
                    let marker = match change.tag() {
                        ChangeTag::Delete => "- ",
                        _ => "  ",
                    };
                    let line = change.value();
                    if self.options.use_color && change.tag() == ChangeTag::Delete {
                        write!(output, "{}", format!("{}{}", marker, line).red()).unwrap();
                    } else {
                        write!(output, "{}{}", marker, line).unwrap();
                    }
                }
            }

            // New section
            let new_header = format!("--- {},{} ----", new_start, new_start + new_len);
            if self.options.use_color {
                writeln!(output, "{}", new_header.yellow()).unwrap();
            } else {
                writeln!(output, "{}", new_header).unwrap();
            }

            for change in hunk.iter_changes() {
                if change.tag() != ChangeTag::Delete {
                    let marker = match change.tag() {
                        ChangeTag::Insert => "+ ",
                        _ => "  ",
                    };
                    let line = change.value();
                    if self.options.use_color && change.tag() == ChangeTag::Insert {
                        write!(output, "{}", format!("{}{}", marker, line).green()).unwrap();
                    } else {
                        write!(output, "{}{}", marker, line).unwrap();
                    }
                }
            }
        }

        output
    }

    /// Format as inline diff with markers
    fn format_inline<'a>(&self, diff: &TextDiff<'a, 'a, 'a, str>) -> String {
        let mut output = String::new();

        for change in diff.iter_all_changes() {
            let line = change.value();

            match change.tag() {
                ChangeTag::Delete => {
                    if self.options.use_color {
                        write!(
                            output,
                            "{}",
                            format!("[-{}]", line.trim_end()).red().strikethrough()
                        )
                        .unwrap();
                    } else {
                        write!(output, "[-{}]", line.trim_end()).unwrap();
                    }
                }
                ChangeTag::Insert => {
                    if self.options.use_color {
                        write!(output, "{}", format!("[+{}]", line.trim_end()).green()).unwrap();
                    } else {
                        write!(output, "[+{}]", line.trim_end()).unwrap();
                    }
                    writeln!(output).unwrap();
                }
                ChangeTag::Equal => {
                    write!(output, "{}", line).unwrap();
                }
            }
        }

        output
    }

    /// Generate a simple diff summary
    pub fn summary(&self, old: &str, new: &str) -> DiffSummary {
        let diff = TextDiff::from_lines(old, new);
        let mut additions = 0;
        let mut deletions = 0;

        for change in diff.iter_all_changes() {
            match change.tag() {
                ChangeTag::Insert => additions += 1,
                ChangeTag::Delete => deletions += 1,
                _ => {}
            }
        }

        // Count changes as min of additions and deletions (rough estimate)
        let changes = additions.min(deletions);
        additions -= changes;
        deletions -= changes;

        DiffSummary {
            additions,
            deletions,
            changes,
        }
    }

    /// Check if there are any differences
    pub fn has_changes(&self, old: &str, new: &str) -> bool {
        let diff = TextDiff::from_lines(old, new);
        diff.iter_all_changes().any(|c| c.tag() != ChangeTag::Equal)
    }
}

impl Default for ColorizedDiff {
    fn default() -> Self {
        Self::new()
    }
}

/// Diff summary statistics
#[derive(Debug, Clone, Default)]
pub struct DiffSummary {
    /// Number of lines added
    pub additions: usize,
    /// Number of lines deleted
    pub deletions: usize,
    /// Number of lines changed
    pub changes: usize,
}

impl DiffSummary {
    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        self.additions > 0 || self.deletions > 0 || self.changes > 0
    }

    /// Get total number of modifications
    pub fn total(&self) -> usize {
        self.additions + self.deletions + self.changes
    }

    /// Format as a colored string
    pub fn format(&self, use_color: bool) -> String {
        let mut parts = Vec::new();

        if self.additions > 0 {
            let s = format!("+{}", self.additions);
            parts.push(if use_color { s.green().to_string() } else { s });
        }

        if self.deletions > 0 {
            let s = format!("-{}", self.deletions);
            parts.push(if use_color { s.red().to_string() } else { s });
        }

        if self.changes > 0 {
            let s = format!("~{}", self.changes);
            parts.push(if use_color { s.yellow().to_string() } else { s });
        }

        if parts.is_empty() {
            "no changes".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// Word-level diff for more granular changes
pub fn word_diff(old: &str, new: &str, use_color: bool) -> String {
    let diff = TextDiff::from_words(old, new);
    let mut output = String::new();

    for change in diff.iter_all_changes() {
        let word = change.value();

        match change.tag() {
            ChangeTag::Delete => {
                if use_color {
                    write!(output, "{}", word.red().strikethrough()).unwrap();
                } else {
                    write!(output, "[-{}]", word).unwrap();
                }
            }
            ChangeTag::Insert => {
                if use_color {
                    write!(output, "{}", word.green().bold()).unwrap();
                } else {
                    write!(output, "[+{}]", word).unwrap();
                }
            }
            ChangeTag::Equal => {
                write!(output, "{}", word).unwrap();
            }
        }
    }

    output
}

/// Character-level diff for small changes
pub fn char_diff(old: &str, new: &str, use_color: bool) -> String {
    let diff = TextDiff::from_chars(old, new);
    let mut output = String::new();

    for change in diff.iter_all_changes() {
        let ch = change.value();

        match change.tag() {
            ChangeTag::Delete => {
                if use_color {
                    write!(output, "{}", ch.red().strikethrough()).unwrap();
                } else {
                    write!(output, "[-{}]", ch).unwrap();
                }
            }
            ChangeTag::Insert => {
                if use_color {
                    write!(output, "{}", ch.green().bold()).unwrap();
                } else {
                    write!(output, "[+{}]", ch).unwrap();
                }
            }
            ChangeTag::Equal => {
                write!(output, "{}", ch).unwrap();
            }
        }
    }

    output
}

/// Truncate a string to a maximum width
fn truncate(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else {
        format!("{}...", &s[..max_width.saturating_sub(3)])
    }
}

/// Format diff output for JSON mode
#[derive(Debug, Clone, serde::Serialize)]
pub struct JsonDiff {
    pub has_changes: bool,
    pub summary: JsonDiffSummary,
    pub hunks: Vec<JsonHunk>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JsonDiffSummary {
    pub additions: usize,
    pub deletions: usize,
    pub changes: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JsonHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<JsonDiffLine>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JsonDiffLine {
    pub tag: String,
    pub content: String,
}

/// Generate JSON-friendly diff output
pub fn json_diff(old: &str, new: &str) -> JsonDiff {
    let differ = ColorizedDiff::new();
    let summary = differ.summary(old, new);
    let text_diff = TextDiff::from_lines(old, new);

    let mut hunks = Vec::new();

    for hunk in text_diff.unified_diff().context_radius(3).iter_hunks() {
        let mut lines = Vec::new();
        let (old_start, old_count, new_start, new_count) = hunk_ranges(hunk.ops());

        for change in hunk.iter_changes() {
            let tag = match change.tag() {
                ChangeTag::Delete => "delete",
                ChangeTag::Insert => "insert",
                ChangeTag::Equal => "equal",
            };

            lines.push(JsonDiffLine {
                tag: tag.to_string(),
                content: change.value().to_string(),
            });
        }

        hunks.push(JsonHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines,
        });
    }

    JsonDiff {
        has_changes: summary.has_changes(),
        summary: JsonDiffSummary {
            additions: summary.additions,
            deletions: summary.deletions,
            changes: summary.changes,
        },
        hunks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_diff() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nmodified line 2\nline 3\n";

        let differ = ColorizedDiff::with_options(DiffOptions {
            use_color: false,
            ..Default::default()
        });

        let diff = differ.diff(old, new, "old.txt", "new.txt");
        assert!(diff.contains("--- old.txt"));
        assert!(diff.contains("+++ new.txt"));
        assert!(diff.contains("-line 2"));
        assert!(diff.contains("+modified line 2"));
    }

    #[test]
    fn test_diff_summary() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nmodified\nline 3\nnew line\n";

        let differ = ColorizedDiff::new();
        let summary = differ.summary(old, new);

        assert!(summary.has_changes());
    }

    #[test]
    fn test_no_changes() {
        let content = "same content\n";
        let differ = ColorizedDiff::new();

        assert!(!differ.has_changes(content, content));
    }

    #[test]
    fn test_word_diff() {
        let old = "hello world";
        let new = "hello rust";

        let diff = word_diff(old, new, false);
        assert!(diff.contains("[-world]") || diff.contains("world"));
        assert!(diff.contains("[+rust]") || diff.contains("rust"));
    }

    #[test]
    fn test_json_diff() {
        let old = "line 1\nline 2\n";
        let new = "line 1\nmodified\n";

        let diff = json_diff(old, new);
        assert!(diff.has_changes);
        assert!(!diff.hunks.is_empty());
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("very long string here", 10), "very lo...");
    }
}
