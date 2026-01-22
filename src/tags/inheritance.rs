//! Tag inheritance system for propagating tags from parent constructs.
//!
//! Tags can be inherited from:
//! - Plays to all tasks in the play
//! - Roles to all tasks in the role
//! - Blocks to all tasks in the block
//! - include_tasks/import_tasks to included tasks

use std::collections::HashSet;

/// Manages tag inheritance from parent constructs to tasks.
#[derive(Debug, Clone, Default)]
pub struct TagInheritance {
    /// Tags inherited from the play
    play_tags: Vec<String>,
    /// Tags inherited from roles (in order of nesting)
    role_tags: Vec<String>,
    /// Tags inherited from blocks (in order of nesting)
    block_tags: Vec<String>,
    /// Tags from include_tasks/import_tasks
    include_tags: Vec<String>,
}

impl TagInheritance {
    /// Create a new empty inheritance context
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with play-level tags
    pub fn with_play_tags(mut self, tags: Vec<String>) -> Self {
        self.play_tags = tags;
        self
    }

    /// Set play-level tags
    pub fn set_play_tags(&mut self, tags: Vec<String>) {
        self.play_tags = tags;
    }

    /// Push role tags (entering a role)
    pub fn push_role_tags(&mut self, tags: &[String]) {
        self.role_tags.extend(tags.iter().cloned());
    }

    /// Pop role tags (exiting a role)
    pub fn pop_role_tags(&mut self, count: usize) {
        let new_len = self.role_tags.len().saturating_sub(count);
        self.role_tags.truncate(new_len);
    }

    /// Push block tags (entering a block)
    pub fn push_block_tags(&mut self, tags: &[String]) {
        self.block_tags.extend(tags.iter().cloned());
    }

    /// Pop block tags (exiting a block)
    pub fn pop_block_tags(&mut self, count: usize) {
        let new_len = self.block_tags.len().saturating_sub(count);
        self.block_tags.truncate(new_len);
    }

    /// Set include tags (for include_tasks/import_tasks)
    pub fn set_include_tags(&mut self, tags: Vec<String>) {
        self.include_tags = tags;
    }

    /// Clear include tags
    pub fn clear_include_tags(&mut self) {
        self.include_tags.clear();
    }

    /// Get all inherited tags combined with task's own tags.
    ///
    /// The inheritance order (lowest to highest precedence):
    /// 1. Play tags
    /// 2. Role tags
    /// 3. Block tags
    /// 4. Include tags
    /// 5. Task's own tags
    pub fn resolve_tags(&self, task_tags: &[String]) -> Vec<String> {
        let mut all_tags: HashSet<String> = HashSet::new();

        // Add inherited tags in order
        all_tags.extend(self.play_tags.iter().cloned());
        all_tags.extend(self.role_tags.iter().cloned());
        all_tags.extend(self.block_tags.iter().cloned());
        all_tags.extend(self.include_tags.iter().cloned());

        // Add task's own tags (highest precedence)
        all_tags.extend(task_tags.iter().cloned());

        // Convert to sorted vector for consistent ordering
        let mut result: Vec<String> = all_tags.into_iter().collect();
        result.sort();
        result
    }

    /// Check if there are any inherited tags
    pub fn has_inherited_tags(&self) -> bool {
        !self.play_tags.is_empty()
            || !self.role_tags.is_empty()
            || !self.block_tags.is_empty()
            || !self.include_tags.is_empty()
    }

    /// Get play tags
    pub fn play_tags(&self) -> &[String] {
        &self.play_tags
    }

    /// Get role tags
    pub fn role_tags(&self) -> &[String] {
        &self.role_tags
    }

    /// Get block tags
    pub fn block_tags(&self) -> &[String] {
        &self.block_tags
    }

    /// Get include tags
    pub fn include_tags(&self) -> &[String] {
        &self.include_tags
    }

    /// Create a child context for entering a nested scope
    pub fn child(&self) -> Self {
        Self {
            play_tags: self.play_tags.clone(),
            role_tags: self.role_tags.clone(),
            block_tags: self.block_tags.clone(),
            include_tags: Vec::new(), // Include tags don't propagate to nested includes
        }
    }
}

/// Builder for constructing tag inheritance contexts
#[derive(Debug, Default)]
pub struct TagInheritanceBuilder {
    inner: TagInheritance,
}

impl TagInheritanceBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set play tags
    pub fn play_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inner.play_tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// Add role tags
    pub fn role_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inner
            .role_tags
            .extend(tags.into_iter().map(Into::into));
        self
    }

    /// Add block tags
    pub fn block_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inner
            .block_tags
            .extend(tags.into_iter().map(Into::into));
        self
    }

    /// Set include tags
    pub fn include_tags(mut self, tags: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inner.include_tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// Build the TagInheritance
    pub fn build(self) -> TagInheritance {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_inheritance() {
        let inheritance = TagInheritance::new();
        let task_tags = vec!["deploy".to_string()];

        let resolved = inheritance.resolve_tags(&task_tags);
        assert_eq!(resolved, vec!["deploy".to_string()]);
    }

    #[test]
    fn test_play_tag_inheritance() {
        let inheritance = TagInheritance::new().with_play_tags(vec!["production".to_string()]);
        let task_tags = vec!["deploy".to_string()];

        let resolved = inheritance.resolve_tags(&task_tags);
        assert!(resolved.contains(&"production".to_string()));
        assert!(resolved.contains(&"deploy".to_string()));
    }

    #[test]
    fn test_role_tag_inheritance() {
        let mut inheritance = TagInheritance::new();
        inheritance.push_role_tags(&["webserver".to_string()]);

        let task_tags = vec!["install".to_string()];
        let resolved = inheritance.resolve_tags(&task_tags);

        assert!(resolved.contains(&"webserver".to_string()));
        assert!(resolved.contains(&"install".to_string()));
    }

    #[test]
    fn test_nested_role_tags() {
        let mut inheritance = TagInheritance::new();
        inheritance.push_role_tags(&["outer_role".to_string()]);
        inheritance.push_role_tags(&["inner_role".to_string()]);

        let task_tags = vec!["task".to_string()];
        let resolved = inheritance.resolve_tags(&task_tags);

        assert!(resolved.contains(&"outer_role".to_string()));
        assert!(resolved.contains(&"inner_role".to_string()));
        assert!(resolved.contains(&"task".to_string()));
    }

    #[test]
    fn test_block_tag_inheritance() {
        let mut inheritance = TagInheritance::new();
        inheritance.push_block_tags(&["block_tag".to_string()]);

        let task_tags = vec!["task_tag".to_string()];
        let resolved = inheritance.resolve_tags(&task_tags);

        assert!(resolved.contains(&"block_tag".to_string()));
        assert!(resolved.contains(&"task_tag".to_string()));
    }

    #[test]
    fn test_include_tag_inheritance() {
        let mut inheritance = TagInheritance::new();
        inheritance.set_include_tags(vec!["included".to_string()]);

        let task_tags = vec!["task".to_string()];
        let resolved = inheritance.resolve_tags(&task_tags);

        assert!(resolved.contains(&"included".to_string()));
        assert!(resolved.contains(&"task".to_string()));
    }

    #[test]
    fn test_full_inheritance_chain() {
        let mut inheritance = TagInheritance::new().with_play_tags(vec!["play".to_string()]);
        inheritance.push_role_tags(&["role".to_string()]);
        inheritance.push_block_tags(&["block".to_string()]);
        inheritance.set_include_tags(vec!["include".to_string()]);

        let task_tags = vec!["task".to_string()];
        let resolved = inheritance.resolve_tags(&task_tags);

        assert!(resolved.contains(&"play".to_string()));
        assert!(resolved.contains(&"role".to_string()));
        assert!(resolved.contains(&"block".to_string()));
        assert!(resolved.contains(&"include".to_string()));
        assert!(resolved.contains(&"task".to_string()));
    }

    #[test]
    fn test_duplicate_tags_deduplicated() {
        let inheritance = TagInheritance::new().with_play_tags(vec!["common".to_string()]);
        let task_tags = vec!["common".to_string(), "unique".to_string()];

        let resolved = inheritance.resolve_tags(&task_tags);

        // Count occurrences of "common"
        let common_count = resolved.iter().filter(|t| *t == "common").count();
        assert_eq!(common_count, 1);
    }

    #[test]
    fn test_pop_role_tags() {
        let mut inheritance = TagInheritance::new();
        inheritance.push_role_tags(&["role1".to_string(), "role2".to_string()]);
        inheritance.pop_role_tags(2);

        assert!(!inheritance.has_inherited_tags());
    }

    #[test]
    fn test_pop_block_tags() {
        let mut inheritance = TagInheritance::new();
        inheritance.push_block_tags(&["block1".to_string()]);
        inheritance.pop_block_tags(1);

        assert!(!inheritance.has_inherited_tags());
    }

    #[test]
    fn test_child_context() {
        let mut parent = TagInheritance::new().with_play_tags(vec!["play".to_string()]);
        parent.push_role_tags(&["role".to_string()]);
        parent.set_include_tags(vec!["include".to_string()]);

        let child = parent.child();

        // Play and role tags should be inherited
        assert_eq!(child.play_tags(), &["play".to_string()]);
        assert_eq!(child.role_tags(), &["role".to_string()]);
        // Include tags should NOT be inherited
        assert!(child.include_tags().is_empty());
    }

    #[test]
    fn test_builder() {
        let inheritance = TagInheritanceBuilder::new()
            .play_tags(["play1", "play2"])
            .role_tags(["role1"])
            .block_tags(["block1"])
            .include_tags(["include1"])
            .build();

        assert_eq!(inheritance.play_tags().len(), 2);
        assert_eq!(inheritance.role_tags().len(), 1);
        assert_eq!(inheritance.block_tags().len(), 1);
        assert_eq!(inheritance.include_tags().len(), 1);
    }

    #[test]
    fn test_has_inherited_tags() {
        let empty = TagInheritance::new();
        assert!(!empty.has_inherited_tags());

        let with_play = TagInheritance::new().with_play_tags(vec!["tag".to_string()]);
        assert!(with_play.has_inherited_tags());
    }
}
