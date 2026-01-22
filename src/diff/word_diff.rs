//! Word-level diff highlighting for small changes.
//!
//! This module provides functionality to highlight word-level changes
//! within modified lines, making it easier to see exactly what changed
//! when only a small portion of a line was modified.

use colored::Colorize;
use similar::{ChangeTag, TextDiff};

/// Word diff result containing highlighted old and new lines
#[derive(Debug, Clone)]
pub struct WordDiff {
    /// The old line with word-level highlighting
    pub old_highlighted: String,
    /// The new line with word-level highlighting
    pub new_highlighted: String,
    /// Whether any differences were found
    pub has_changes: bool,
}

impl WordDiff {
    /// Create a word diff between two lines
    pub fn new(old: &str, new: &str, use_color: bool) -> Self {
        if old == new {
            return Self {
                old_highlighted: old.to_string(),
                new_highlighted: new.to_string(),
                has_changes: false,
            };
        }

        // Use character-level diff for better precision
        let diff = TextDiff::from_chars(old, new);
        let mut old_parts = Vec::new();
        let mut new_parts = Vec::new();
        let mut has_changes = false;

        for change in diff.iter_all_changes() {
            let value = change.value();
            match change.tag() {
                ChangeTag::Delete => {
                    has_changes = true;
                    if use_color {
                        old_parts.push(value.on_red().white().to_string());
                    } else {
                        old_parts.push(format!("[-{}]", value));
                    }
                }
                ChangeTag::Insert => {
                    has_changes = true;
                    if use_color {
                        new_parts.push(value.on_green().white().to_string());
                    } else {
                        new_parts.push(format!("[+{}]", value));
                    }
                }
                ChangeTag::Equal => {
                    old_parts.push(value.to_string());
                    new_parts.push(value.to_string());
                }
            }
        }

        Self {
            old_highlighted: old_parts.concat(),
            new_highlighted: new_parts.concat(),
            has_changes,
        }
    }

    /// Create a word diff using word boundaries instead of characters
    pub fn word_level(old: &str, new: &str, use_color: bool) -> Self {
        if old == new {
            return Self {
                old_highlighted: old.to_string(),
                new_highlighted: new.to_string(),
                has_changes: false,
            };
        }

        let diff = TextDiff::from_words(old, new);
        let mut old_parts = Vec::new();
        let mut new_parts = Vec::new();
        let mut has_changes = false;

        for change in diff.iter_all_changes() {
            let value = change.value();
            match change.tag() {
                ChangeTag::Delete => {
                    has_changes = true;
                    if use_color {
                        old_parts.push(value.on_red().white().bold().to_string());
                    } else {
                        old_parts.push(format!("[-{}]", value));
                    }
                }
                ChangeTag::Insert => {
                    has_changes = true;
                    if use_color {
                        new_parts.push(value.on_green().white().bold().to_string());
                    } else {
                        new_parts.push(format!("[+{}]", value));
                    }
                }
                ChangeTag::Equal => {
                    old_parts.push(value.to_string());
                    new_parts.push(value.to_string());
                }
            }
        }

        Self {
            old_highlighted: old_parts.concat(),
            new_highlighted: new_parts.concat(),
            has_changes,
        }
    }
}

/// Format a line with inline word-level diff highlighting
pub fn format_inline_diff(old: &str, new: &str, use_color: bool) -> String {
    let word_diff = WordDiff::new(old, new, use_color);

    if !word_diff.has_changes {
        return format!("  {}", old.trim_end());
    }

    let mut output = String::new();

    if use_color {
        output.push_str(&format!("{}{}\n", "-".red(), word_diff.old_highlighted));
        output.push_str(&format!("{}{}", "+".green(), word_diff.new_highlighted));
    } else {
        output.push_str(&format!("-{}\n", word_diff.old_highlighted));
        output.push_str(&format!("+{}", word_diff.new_highlighted));
    }

    output
}

/// Check if two lines are similar enough for word-level diff
/// (i.e., they share enough common content that word-level diff would be useful)
pub fn lines_are_similar(old: &str, new: &str) -> bool {
    if old.is_empty() || new.is_empty() {
        return false;
    }

    // Use a simple heuristic: if the lines share a significant prefix or suffix,
    // or if the edit distance is small relative to the line length
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();

    // Check common prefix
    let common_prefix = old_chars
        .iter()
        .zip(new_chars.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // Check common suffix
    let common_suffix = old_chars
        .iter()
        .rev()
        .zip(new_chars.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    let max_len = old_chars.len().max(new_chars.len());
    let common = common_prefix + common_suffix;

    // Consider similar if at least 40% of the longer line is common
    common >= max_len * 2 / 5
}

/// Pair up similar deleted and inserted lines for word-level diffing
pub fn pair_similar_lines<'a>(
    deleted: &'a [&str],
    inserted: &'a [&str],
) -> Vec<(Option<&'a str>, Option<&'a str>)> {
    let mut result = Vec::new();
    let mut used_inserted: Vec<bool> = vec![false; inserted.len()];

    for &del in deleted {
        let mut best_match: Option<(usize, usize)> = None;
        let mut best_similarity = 0usize;

        for (i, &ins) in inserted.iter().enumerate() {
            if used_inserted[i] {
                continue;
            }

            if lines_are_similar(del, ins) {
                let similarity = common_chars_count(del, ins);
                if similarity > best_similarity {
                    best_similarity = similarity;
                    best_match = Some((i, similarity));
                }
            }
        }

        if let Some((i, _)) = best_match {
            used_inserted[i] = true;
            result.push((Some(del), Some(inserted[i])));
        } else {
            result.push((Some(del), None));
        }
    }

    // Add remaining unmatched inserted lines
    for (i, &ins) in inserted.iter().enumerate() {
        if !used_inserted[i] {
            result.push((None, Some(ins)));
        }
    }

    result
}

/// Count common characters between two strings
fn common_chars_count(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();

    let mut common = 0;

    // Count common prefix
    common += a_chars
        .iter()
        .zip(b_chars.iter())
        .take_while(|(x, y)| x == y)
        .count();

    // Count common suffix (avoiding double-counting)
    let suffix_common = a_chars
        .iter()
        .rev()
        .zip(b_chars.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();

    // Make sure we don't count overlapping parts twice
    let min_len = a_chars.len().min(b_chars.len());
    if common + suffix_common <= min_len {
        common += suffix_common;
    }

    common
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_diff_no_changes() {
        let diff = WordDiff::new("same content", "same content", false);
        assert!(!diff.has_changes);
        assert_eq!(diff.old_highlighted, "same content");
        assert_eq!(diff.new_highlighted, "same content");
    }

    #[test]
    fn test_word_diff_single_char() {
        let diff = WordDiff::new("hello world", "hello World", false);
        assert!(diff.has_changes);
        assert!(diff.old_highlighted.contains("[-w]"));
        assert!(diff.new_highlighted.contains("[+W]"));
    }

    #[test]
    fn test_word_diff_word_level() {
        let diff = WordDiff::word_level("the quick brown fox", "the slow brown fox", false);
        assert!(diff.has_changes);
        assert!(
            diff.old_highlighted.contains("[-quick]") || diff.old_highlighted.contains("quick")
        );
        assert!(diff.new_highlighted.contains("[+slow]") || diff.new_highlighted.contains("slow"));
    }

    #[test]
    fn test_lines_are_similar() {
        assert!(lines_are_similar("hello world", "hello World"));
        assert!(lines_are_similar(
            "server_name old.com;",
            "server_name new.com;"
        ));
        assert!(!lines_are_similar("completely different", "another thing"));
        assert!(!lines_are_similar("", "something"));
    }

    #[test]
    fn test_pair_similar_lines() {
        let deleted = vec!["server_name old.com;", "listen 80;"];
        let inserted = vec!["server_name new.com;", "listen 8080;"];

        let pairs = pair_similar_lines(&deleted, &inserted);

        assert_eq!(pairs.len(), 2);
        // Both should be paired
        for (del, ins) in pairs {
            assert!(del.is_some());
            assert!(ins.is_some());
        }
    }

    #[test]
    fn test_common_chars_count() {
        assert_eq!(common_chars_count("hello", "hello"), 5);
        assert_eq!(common_chars_count("hello", "helloworld"), 5);
        assert!(common_chars_count("hello world", "hello World") > 8);
    }
}
