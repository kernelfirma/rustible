//! Built-in compliance baseline policy pack.
//!
//! Rules:
//! - `require-tags`  -- every play must have tags
//! - `max-tasks`     -- limit tasks per play
//! - `require-name`  -- every task must have a name

use crate::policy::pack::manifest::{PackCategory, PackParameter, PolicyPackManifest};

/// Return the manifest for the built-in compliance baseline pack.
pub fn manifest() -> PolicyPackManifest {
    PolicyPackManifest {
        name: "compliance-baseline".into(),
        version: "1.0.0".into(),
        description: "Compliance baseline rules: enforce tagging, naming, and task limits".into(),
        category: PackCategory::Compliance,
        rules: vec![
            "require-tags".into(),
            "max-tasks".into(),
            "require-name".into(),
        ],
        parameters: vec![PackParameter {
            name: "max_tasks_per_play".into(),
            description: "Maximum number of tasks allowed per play".into(),
            param_type: "integer".into(),
            default_value: Some("20".into()),
            required: false,
        }],
    }
}
