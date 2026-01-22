//! Tag system for task filtering and selection.
//!
//! This module provides comprehensive tag handling including:
//! - Tag expressions with AND, OR, NOT operators
//! - Tag inheritance from plays, roles, and blocks to tasks
//! - Special tags: `always`, `never`, `tagged`, `untagged`
//! - Tag collection and listing
//!
//! # Tag Expression Syntax
//!
//! Tags can be combined using logical operators:
//! - `tag1,tag2` - OR: matches if any tag matches
//! - `tag1&tag2` or `tag1+tag2` - AND: matches only if all tags match
//! - `!tag1` or `not tag1` - NOT: excludes tasks with this tag
//!
//! # Special Tags
//!
//! - `always`: Task runs regardless of tag selection (unless explicitly skipped)
//! - `never`: Task never runs unless explicitly included with `--tags never`
//! - `tagged`: Matches any task that has at least one tag
//! - `untagged`: Matches any task with no tags
//! - `all`: Matches all tasks (equivalent to no tag filter)
//!
//! # Tag Inheritance
//!
//! Tags are inherited from parent constructs:
//! - Play tags apply to all tasks in the play
//! - Role tags apply to all tasks in the role
//! - Block tags apply to all tasks in the block
//!
//! # Example
//!
//! ```rust
//! use rustible::tags::{TagFilter, TagExpression};
//!
//! // Simple tag matching
//! let filter = TagFilter::new()
//!     .with_tags(vec!["deploy".to_string()])
//!     .with_skip_tags(vec!["debug".to_string()]);
//!
//! let task_tags = vec!["deploy".to_string(), "web".to_string()];
//! assert!(filter.should_run(&task_tags));
//!
//! // Tag expression parsing
//! let expr = TagExpression::parse("deploy,web&!debug").unwrap();
//! assert!(expr.matches(&["deploy", "web"]));
//! ```

mod expression;
mod filter;
mod inheritance;

pub use expression::{TagExpression, TagExpressionError};
pub use filter::TagFilter;
pub use inheritance::TagInheritance;

/// Special tag constants
pub mod special {
    /// Tag that causes a task to always run regardless of tag selection
    pub const ALWAYS: &str = "always";

    /// Tag that causes a task to never run unless explicitly selected
    pub const NEVER: &str = "never";

    /// Matches any task that has at least one tag
    pub const TAGGED: &str = "tagged";

    /// Matches any task with no tags
    pub const UNTAGGED: &str = "untagged";

    /// Matches all tasks
    pub const ALL: &str = "all";
}

/// Check if a tag is a special tag
pub fn is_special_tag(tag: &str) -> bool {
    matches!(
        tag.to_lowercase().as_str(),
        special::ALWAYS | special::NEVER | special::TAGGED | special::UNTAGGED | special::ALL
    )
}

/// Collect all tags from a playbook structure
#[derive(Debug, Clone, Default)]
pub struct TagCollector {
    /// All unique tags found
    pub tags: std::collections::BTreeSet<String>,
    /// Tag to task name mapping
    pub tag_tasks: std::collections::HashMap<String, Vec<String>>,
}

impl TagCollector {
    /// Create a new empty tag collector
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a tag with an optional task name
    pub fn add_tag(&mut self, tag: impl Into<String>, task_name: Option<&str>) {
        let tag = tag.into();
        self.tags.insert(tag.clone());

        if let Some(name) = task_name {
            self.tag_tasks
                .entry(tag)
                .or_default()
                .push(name.to_string());
        }
    }

    /// Add multiple tags
    pub fn add_tags(&mut self, tags: &[String], task_name: Option<&str>) {
        for tag in tags {
            self.add_tag(tag.clone(), task_name);
        }
    }

    /// Merge another collector into this one
    pub fn merge(&mut self, other: TagCollector) {
        self.tags.extend(other.tags);
        for (tag, tasks) in other.tag_tasks {
            self.tag_tasks.entry(tag).or_default().extend(tasks);
        }
    }

    /// Get all tags as a sorted vector
    pub fn all_tags(&self) -> Vec<&String> {
        self.tags.iter().collect()
    }

    /// Get task names for a specific tag
    pub fn tasks_for_tag(&self, tag: &str) -> Option<&Vec<String>> {
        self.tag_tasks.get(tag)
    }

    /// Format tags for display
    pub fn format_display(&self) -> String {
        let mut output = String::new();

        if self.tags.is_empty() {
            output.push_str("No tags found in playbook.\n");
            return output;
        }

        output.push_str(&format!("Found {} unique tags:\n\n", self.tags.len()));

        for tag in &self.tags {
            output.push_str(&format!("  {}", tag));

            if let Some(tasks) = self.tag_tasks.get(tag) {
                output.push_str(&format!(
                    " ({} task{})",
                    tasks.len(),
                    if tasks.len() == 1 { "" } else { "s" }
                ));
            }

            output.push('\n');
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_special_tag() {
        assert!(is_special_tag("always"));
        assert!(is_special_tag("ALWAYS"));
        assert!(is_special_tag("never"));
        assert!(is_special_tag("tagged"));
        assert!(is_special_tag("untagged"));
        assert!(is_special_tag("all"));

        assert!(!is_special_tag("deploy"));
        assert!(!is_special_tag("web"));
    }

    #[test]
    fn test_tag_collector() {
        let mut collector = TagCollector::new();

        collector.add_tag("deploy", Some("Deploy application"));
        collector.add_tag("deploy", Some("Deploy config"));
        collector.add_tag("web", Some("Configure nginx"));

        assert_eq!(collector.tags.len(), 2);
        assert!(collector.tags.contains("deploy"));
        assert!(collector.tags.contains("web"));

        let deploy_tasks = collector.tasks_for_tag("deploy").unwrap();
        assert_eq!(deploy_tasks.len(), 2);
    }

    #[test]
    fn test_tag_collector_merge() {
        let mut collector1 = TagCollector::new();
        collector1.add_tag("deploy", Some("Task 1"));

        let mut collector2 = TagCollector::new();
        collector2.add_tag("web", Some("Task 2"));
        collector2.add_tag("deploy", Some("Task 3"));

        collector1.merge(collector2);

        assert_eq!(collector1.tags.len(), 2);
        assert_eq!(collector1.tasks_for_tag("deploy").unwrap().len(), 2);
    }
}
