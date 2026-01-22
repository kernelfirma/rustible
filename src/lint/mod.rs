//! Playbook linting and validation.
//!
//! This module provides comprehensive linting capabilities for Ansible/Rustible
//! playbooks, including:
//!
//! - YAML syntax validation
//! - Module parameter checking
//! - Best practices enforcement
//! - Security vulnerability detection
//!
//! # Example
//!
//! ```rust,ignore,no_run
//! # #[tokio::main]
//! # async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
//! use rustible::prelude::*;
//! use rustible::lint::{LintConfig, YamlChecker};
//! use std::path::Path;
//!
//! let config = LintConfig::default();
//! let checker = YamlChecker::new();
//! let result = checker.check_file(Path::new("playbook.yml"), &config)?;
//!
//! for issue in &result.issues {
//!     println!("{}: {}", issue.severity, issue.message);
//! }
//! # Ok(())
//! # }
//! ```

mod best_practices;
mod params;
mod types;
mod yaml;

pub use best_practices::BestPracticesChecker;
pub use params::{ModuleDef, ParamDef, ParamType, ParamValidator};
pub use types::{
    LintConfig, LintError, LintIssue, LintOpResult, LintResult, Location, RuleCategory, Severity,
};
pub use yaml::YamlChecker;
