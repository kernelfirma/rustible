//! Built-in operations baseline policy pack.
//!
//! Rules:
//! - `max-forks`             -- warn when forks configuration is excessive
//! - `require-limit`         -- require a limit pattern for production runs
//! - `deny-localhost-in-prod` -- deny using localhost in production plays

use crate::policy::pack::manifest::{PackCategory, PackParameter, PolicyPackManifest};

/// Return the manifest for the built-in operations baseline pack.
pub fn manifest() -> PolicyPackManifest {
    PolicyPackManifest {
        name: "operations-baseline".into(),
        version: "1.0.0".into(),
        description:
            "Operations baseline rules: safe fork limits, require limits, deny localhost in prod"
                .into(),
        category: PackCategory::Operations,
        rules: vec![
            "max-forks".into(),
            "require-limit".into(),
            "deny-localhost-in-prod".into(),
        ],
        parameters: vec![PackParameter {
            name: "max_forks".into(),
            description: "Maximum allowed fork count before warning".into(),
            param_type: "integer".into(),
            default_value: Some("50".into()),
            required: false,
        }],
    }
}
