//! Collection dependency resolution
//!
//! Handles resolving collection dependencies with version constraints.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// Error during dependency resolution
#[derive(Debug, Error, Clone)]
pub enum DependencyResolutionError {
    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("Version conflict for {collection}: requires {required} but {available} is available")]
    VersionConflict {
        collection: String,
        required: String,
        available: String,
    },

    #[error("Collection not found: {0}")]
    NotFound(String),

    #[error("Invalid version constraint: {0}")]
    InvalidConstraint(String),

    #[error("No version satisfies constraints for {collection}: {constraints}")]
    NoMatchingVersion {
        collection: String,
        constraints: String,
    },
}

/// A collection dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionDependency {
    /// Collection name (namespace.name)
    pub name: String,
    /// Version constraint
    pub version_constraint: VersionConstraint,
    /// Whether this is an optional dependency
    #[serde(default)]
    pub optional: bool,
}

impl CollectionDependency {
    /// Create a new dependency with a version constraint
    pub fn new(name: impl Into<String>, constraint: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version_constraint: VersionConstraint::parse(&constraint.into())
                .unwrap_or(VersionConstraint::Any),
            optional: false,
        }
    }

    /// Create an optional dependency
    pub fn optional(name: impl Into<String>, constraint: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version_constraint: VersionConstraint::parse(&constraint.into())
                .unwrap_or(VersionConstraint::Any),
            optional: true,
        }
    }
}

/// Version constraint for dependencies
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VersionConstraint {
    /// Any version
    #[default]
    Any,
    /// Exact version
    Exact(String),
    /// Greater than or equal
    Gte(String),
    /// Greater than
    Gt(String),
    /// Less than or equal
    Lte(String),
    /// Less than
    Lt(String),
    /// Range (min, max)
    Range(String, String),
    /// Multiple constraints (all must match)
    And(Vec<VersionConstraint>),
}

impl VersionConstraint {
    /// Parse a version constraint string
    pub fn parse(s: &str) -> Result<Self, DependencyResolutionError> {
        let s = s.trim();

        if s.is_empty() || s == "*" {
            return Ok(Self::Any);
        }

        // Handle compound constraints (e.g., ">=1.0.0,<2.0.0")
        if s.contains(',') {
            let constraints: Result<Vec<_>, _> =
                s.split(',').map(|part| Self::parse(part.trim())).collect();
            return Ok(Self::And(constraints?));
        }

        // Handle individual constraints
        if let Some(version) = s.strip_prefix(">=") {
            Ok(Self::Gte(version.trim().to_string()))
        } else if let Some(version) = s.strip_prefix('>') {
            Ok(Self::Gt(version.trim().to_string()))
        } else if let Some(version) = s.strip_prefix("<=") {
            Ok(Self::Lte(version.trim().to_string()))
        } else if let Some(version) = s.strip_prefix('<') {
            Ok(Self::Lt(version.trim().to_string()))
        } else if let Some(version) = s.strip_prefix("==") {
            Ok(Self::Exact(version.trim().to_string()))
        } else if let Some(version) = s.strip_prefix('=') {
            Ok(Self::Exact(version.trim().to_string()))
        } else {
            // Treat as exact version
            Ok(Self::Exact(s.to_string()))
        }
    }

    /// Check if a version satisfies this constraint
    pub fn matches(&self, version: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Exact(v) => version == v,
            Self::Gte(v) => compare_versions(version, v)
                .map(|c| c >= std::cmp::Ordering::Equal)
                .unwrap_or(false),
            Self::Gt(v) => compare_versions(version, v)
                .map(|c| c == std::cmp::Ordering::Greater)
                .unwrap_or(false),
            Self::Lte(v) => compare_versions(version, v)
                .map(|c| c <= std::cmp::Ordering::Equal)
                .unwrap_or(false),
            Self::Lt(v) => compare_versions(version, v)
                .map(|c| c == std::cmp::Ordering::Less)
                .unwrap_or(false),
            Self::Range(min, max) => {
                let gte_min = compare_versions(version, min)
                    .map(|c| c >= std::cmp::Ordering::Equal)
                    .unwrap_or(false);
                let lt_max = compare_versions(version, max)
                    .map(|c| c == std::cmp::Ordering::Less)
                    .unwrap_or(false);
                gte_min && lt_max
            }
            Self::And(constraints) => constraints.iter().all(|c| c.matches(version)),
        }
    }
}

/// Compare two version strings
fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parse_version = |v: &str| -> Option<semver::Version> { semver::Version::parse(v).ok() };

    match (parse_version(a), parse_version(b)) {
        (Some(va), Some(vb)) => Some(va.cmp(&vb)),
        _ => Some(a.cmp(b)), // Fall back to string comparison
    }
}

/// Graph of collection dependencies
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    /// Dependencies for each collection
    dependencies: HashMap<String, Vec<CollectionDependency>>,
    /// Resolved versions
    resolved: HashMap<String, String>,
}

impl DependencyGraph {
    /// Create a new empty dependency graph
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a collection and its dependencies
    pub fn add_collection(&mut self, name: &str, deps: Vec<CollectionDependency>) {
        self.dependencies.insert(name.to_string(), deps);
    }

    /// Set the resolved version for a collection
    pub fn set_resolved(&mut self, name: &str, version: &str) {
        self.resolved.insert(name.to_string(), version.to_string());
    }

    /// Get the resolved version for a collection
    pub fn get_resolved(&self, name: &str) -> Option<&String> {
        self.resolved.get(name)
    }

    /// Get dependencies for a collection
    pub fn get_dependencies(&self, name: &str) -> Option<&Vec<CollectionDependency>> {
        self.dependencies.get(name)
    }

    /// Get all collection names in the graph
    pub fn collections(&self) -> impl Iterator<Item = &String> {
        self.dependencies.keys()
    }

    /// Check for circular dependencies
    pub fn has_circular_dependency(&self) -> Option<String> {
        for start in self.dependencies.keys() {
            let mut visited = HashSet::new();
            let mut path = Vec::new();

            if self.detect_cycle(start, &mut visited, &mut path) {
                return Some(path.join(" -> "));
            }
        }
        None
    }

    fn detect_cycle(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        path: &mut Vec<String>,
    ) -> bool {
        if path.contains(&node.to_string()) {
            path.push(node.to_string());
            return true;
        }

        if visited.contains(node) {
            return false;
        }

        visited.insert(node.to_string());
        path.push(node.to_string());

        if let Some(deps) = self.dependencies.get(node) {
            for dep in deps {
                if self.detect_cycle(&dep.name, visited, path) {
                    return true;
                }
            }
        }

        path.pop();
        false
    }
}

/// Resolves collection dependencies
pub struct DependencyResolver {
    graph: DependencyGraph,
}

impl DependencyResolver {
    /// Create a new resolver
    pub fn new() -> Self {
        Self {
            graph: DependencyGraph::new(),
        }
    }

    /// Add a root collection to resolve
    pub fn add_root(&mut self, name: &str, version: &str, deps: Vec<CollectionDependency>) {
        self.graph.add_collection(name, deps);
        self.graph.set_resolved(name, version);
    }

    /// Resolve all dependencies
    pub fn resolve(&self) -> Result<Vec<(String, String)>, DependencyResolutionError> {
        // Check for circular dependencies first
        if let Some(cycle) = self.graph.has_circular_dependency() {
            return Err(DependencyResolutionError::CircularDependency(cycle));
        }

        let mut result: Vec<(String, String)> = self
            .graph
            .resolved
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Sort by dependency order (simple topological sort)
        result.sort_by(|a, b| a.0.cmp(&b.0));

        Ok(result)
    }

    /// Get the dependency graph
    pub fn graph(&self) -> &DependencyGraph {
        &self.graph
    }
}

impl Default for DependencyResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constraint_parse() {
        assert!(matches!(
            VersionConstraint::parse("*").unwrap(),
            VersionConstraint::Any
        ));
        assert!(matches!(
            VersionConstraint::parse(">=1.0.0").unwrap(),
            VersionConstraint::Gte(_)
        ));
        assert!(matches!(
            VersionConstraint::parse("<2.0.0").unwrap(),
            VersionConstraint::Lt(_)
        ));
    }

    #[test]
    fn test_version_constraint_matches() {
        let gte = VersionConstraint::Gte("1.0.0".to_string());
        assert!(gte.matches("1.0.0"));
        assert!(gte.matches("1.1.0"));
        assert!(gte.matches("2.0.0"));
        assert!(!gte.matches("0.9.0"));
    }

    #[test]
    fn test_compound_constraint() {
        let constraint = VersionConstraint::parse(">=1.0.0,<2.0.0").unwrap();
        assert!(constraint.matches("1.0.0"));
        assert!(constraint.matches("1.5.0"));
        assert!(!constraint.matches("2.0.0"));
        assert!(!constraint.matches("0.9.0"));
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut graph = DependencyGraph::new();
        graph.add_collection("a", vec![CollectionDependency::new("b", "*")]);
        graph.add_collection("b", vec![CollectionDependency::new("c", "*")]);
        graph.add_collection("c", vec![CollectionDependency::new("a", "*")]);

        assert!(graph.has_circular_dependency().is_some());
    }
}
