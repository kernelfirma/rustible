//! Tag filter implementation for task selection.

use super::expression::TagExpression;
use super::special;

/// A filter for selecting tasks based on tags.
///
/// This provides a unified interface for tag-based task filtering,
/// handling both include and exclude tags, as well as special tags.
#[derive(Debug, Clone, Default)]
pub struct TagFilter {
    /// Tags to include (tasks must match at least one)
    include_tags: Option<TagExpression>,
    /// Tags to skip (tasks matching any are excluded)
    skip_tags: Option<TagExpression>,
}

impl TagFilter {
    /// Create a new empty tag filter (matches all tasks)
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a filter with include tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        if !tags.is_empty() {
            // Parse each tag and combine with OR
            let exprs: Vec<_> = tags
                .iter()
                .filter_map(|t| TagExpression::parse(t).ok())
                .collect();

            if exprs.len() == 1 {
                self.include_tags = exprs.into_iter().next();
            } else if !exprs.is_empty() {
                self.include_tags = Some(TagExpression::Or(exprs));
            }
        }
        self
    }

    /// Create a filter with skip tags
    pub fn with_skip_tags(mut self, tags: Vec<String>) -> Self {
        if !tags.is_empty() {
            let exprs: Vec<_> = tags
                .iter()
                .filter_map(|t| TagExpression::parse(t).ok())
                .collect();

            if exprs.len() == 1 {
                self.skip_tags = exprs.into_iter().next();
            } else if !exprs.is_empty() {
                self.skip_tags = Some(TagExpression::Or(exprs));
            }
        }
        self
    }

    /// Set the include tag expression directly
    pub fn with_include_expression(mut self, expr: Option<TagExpression>) -> Self {
        self.include_tags = expr;
        self
    }

    /// Set the skip tag expression directly
    pub fn with_skip_expression(mut self, expr: Option<TagExpression>) -> Self {
        self.skip_tags = expr;
        self
    }

    /// Check if any filters are active
    pub fn is_active(&self) -> bool {
        self.include_tags.is_some() || self.skip_tags.is_some()
    }

    /// Check if a task with the given tags should run
    ///
    /// # Logic
    ///
    /// 1. If task has `always` tag and `always` is not in skip_tags, run it
    /// 2. If task has `never` tag and `never` is not in include_tags, skip it
    /// 3. If skip_tags matches, skip the task
    /// 4. If include_tags is specified, task must match to run
    /// 5. Otherwise, run the task
    pub fn should_run(&self, task_tags: &[String]) -> bool {
        let task_tag_strs: Vec<&str> = task_tags.iter().map(|s| s.as_str()).collect();

        // Handle 'always' tag - runs unless explicitly skipped
        if task_tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case(special::ALWAYS))
        {
            // Check if 'always' is explicitly in skip_tags
            if let Some(ref skip_expr) = self.skip_tags {
                let always_explicitly_skipped = matches!(
                    skip_expr,
                    TagExpression::Tag(t) if t.eq_ignore_ascii_case(special::ALWAYS)
                ) || matches!(
                    skip_expr,
                    TagExpression::Or(exprs) if exprs.iter().any(|e| matches!(e, TagExpression::Tag(t) if t.eq_ignore_ascii_case(special::ALWAYS)))
                );

                if !always_explicitly_skipped {
                    return true;
                }
            } else {
                return true;
            }
        }

        // Handle 'never' tag - never runs unless explicitly included
        if task_tags
            .iter()
            .any(|t| t.eq_ignore_ascii_case(special::NEVER))
        {
            // Check if 'never' is explicitly in include_tags
            if let Some(ref include_expr) = self.include_tags {
                let never_explicitly_included =
                    self.expression_contains_tag(include_expr, special::NEVER);
                if !never_explicitly_included {
                    return false;
                }
            } else {
                return false;
            }
        }

        // Check skip_tags first - if any match, skip the task
        if let Some(ref skip_expr) = self.skip_tags {
            if skip_expr.matches(&task_tag_strs) {
                return false;
            }
        }

        // If no include_tags specified, run the task
        if self.include_tags.is_none() {
            return true;
        }

        // Check if include_tags match
        if let Some(ref include_expr) = self.include_tags {
            // Handle 'untagged' special case
            if self.expression_contains_tag(include_expr, special::UNTAGGED) && task_tags.is_empty()
            {
                return true;
            }

            // Handle 'tagged' special case
            if self.expression_contains_tag(include_expr, special::TAGGED) && !task_tags.is_empty()
            {
                return true;
            }

            // Handle 'all' special case
            if self.expression_contains_tag(include_expr, special::ALL) {
                return true;
            }

            return include_expr.matches(&task_tag_strs);
        }

        true
    }

    /// Check if an expression contains a specific tag (at any level)
    fn expression_contains_tag(&self, expr: &TagExpression, tag: &str) -> bool {
        match expr {
            TagExpression::Tag(t) => t.eq_ignore_ascii_case(tag),
            TagExpression::Not(inner) => self.expression_contains_tag(inner, tag),
            TagExpression::And(exprs) | TagExpression::Or(exprs) => {
                exprs.iter().any(|e| self.expression_contains_tag(e, tag))
            }
        }
    }

    /// Get all referenced tags from the filter
    pub fn referenced_tags(&self) -> Vec<&str> {
        let mut tags = Vec::new();

        if let Some(ref expr) = self.include_tags {
            tags.extend(expr.referenced_tags());
        }

        if let Some(ref expr) = self.skip_tags {
            tags.extend(expr.referenced_tags());
        }

        tags
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_filter_matches_all() {
        let filter = TagFilter::new();

        assert!(filter.should_run(&[]));
        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(filter.should_run(&["any".to_string(), "tags".to_string()]));
    }

    #[test]
    fn test_include_tags() {
        let filter = TagFilter::new().with_tags(vec!["deploy".to_string()]);

        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(filter.should_run(&["deploy".to_string(), "web".to_string()]));
        assert!(!filter.should_run(&["web".to_string()]));
        assert!(!filter.should_run(&[]));
    }

    #[test]
    fn test_skip_tags() {
        let filter = TagFilter::new().with_skip_tags(vec!["debug".to_string()]);

        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(filter.should_run(&[]));
        assert!(!filter.should_run(&["debug".to_string()]));
        assert!(!filter.should_run(&["deploy".to_string(), "debug".to_string()]));
    }

    #[test]
    fn test_include_and_skip_tags() {
        let filter = TagFilter::new()
            .with_tags(vec!["deploy".to_string()])
            .with_skip_tags(vec!["debug".to_string()]);

        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(!filter.should_run(&["deploy".to_string(), "debug".to_string()]));
        assert!(!filter.should_run(&["web".to_string()]));
        assert!(!filter.should_run(&["debug".to_string()]));
    }

    #[test]
    fn test_always_tag() {
        let filter = TagFilter::new().with_tags(vec!["deploy".to_string()]);

        // Task with 'always' tag should run even without matching tags
        assert!(filter.should_run(&["always".to_string()]));
        assert!(filter.should_run(&["always".to_string(), "something".to_string()]));
    }

    #[test]
    fn test_always_tag_with_skip() {
        let filter = TagFilter::new()
            .with_tags(vec!["deploy".to_string()])
            .with_skip_tags(vec!["always".to_string()]);

        // When 'always' is explicitly in skip_tags, it should be skipped
        assert!(!filter.should_run(&["always".to_string()]));
    }

    #[test]
    fn test_never_tag() {
        let filter = TagFilter::new();

        // Task with 'never' tag should not run by default
        assert!(!filter.should_run(&["never".to_string()]));
        assert!(!filter.should_run(&["never".to_string(), "deploy".to_string()]));
    }

    #[test]
    fn test_never_tag_explicitly_included() {
        let filter = TagFilter::new().with_tags(vec!["never".to_string()]);

        // When 'never' is explicitly included, the task should run
        assert!(filter.should_run(&["never".to_string()]));
    }

    #[test]
    fn test_untagged_special() {
        let filter = TagFilter::new().with_tags(vec!["untagged".to_string()]);

        assert!(filter.should_run(&[]));
        assert!(!filter.should_run(&["deploy".to_string()]));
    }

    #[test]
    fn test_tagged_special() {
        let filter = TagFilter::new().with_tags(vec!["tagged".to_string()]);

        assert!(!filter.should_run(&[]));
        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(filter.should_run(&["any".to_string(), "tags".to_string()]));
    }

    #[test]
    fn test_all_special() {
        let filter = TagFilter::new().with_tags(vec!["all".to_string()]);

        assert!(filter.should_run(&[]));
        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(filter.should_run(&["any".to_string(), "tags".to_string()]));
    }

    #[test]
    fn test_is_active() {
        let filter = TagFilter::new();
        assert!(!filter.is_active());

        let filter = TagFilter::new().with_tags(vec!["deploy".to_string()]);
        assert!(filter.is_active());

        let filter = TagFilter::new().with_skip_tags(vec!["debug".to_string()]);
        assert!(filter.is_active());
    }

    #[test]
    fn test_multiple_include_tags() {
        let filter = TagFilter::new().with_tags(vec!["deploy".to_string(), "web".to_string()]);

        assert!(filter.should_run(&["deploy".to_string()]));
        assert!(filter.should_run(&["web".to_string()]));
        assert!(filter.should_run(&["deploy".to_string(), "web".to_string()]));
        assert!(!filter.should_run(&["database".to_string()]));
    }

    #[test]
    fn test_complex_expression() {
        let filter = TagFilter::new().with_tags(vec!["deploy&web".to_string()]);

        assert!(filter.should_run(&["deploy".to_string(), "web".to_string()]));
        assert!(!filter.should_run(&["deploy".to_string()]));
        assert!(!filter.should_run(&["web".to_string()]));
    }

    #[test]
    fn test_referenced_tags() {
        let filter = TagFilter::new()
            .with_tags(vec!["deploy".to_string(), "web".to_string()])
            .with_skip_tags(vec!["debug".to_string()]);

        let tags = filter.referenced_tags();
        assert!(tags.contains(&"deploy"));
        assert!(tags.contains(&"web"));
        assert!(tags.contains(&"debug"));
    }
}
