//! Built-in security baseline policy pack.
//!
//! Rules:
//! - `no-shell` -- deny the `shell` module
//! - `no-raw`   -- deny the `raw` module
//! - `require-become-explicit` -- require explicit `become` declarations

use crate::policy::pack::manifest::{PackCategory, PolicyPackManifest};

/// Return the manifest for the built-in security baseline pack.
pub fn manifest() -> PolicyPackManifest {
    PolicyPackManifest {
        name: "security-baseline".into(),
        version: "1.0.0".into(),
        description: "Security baseline rules: deny dangerous modules and require explicit privilege escalation".into(),
        category: PackCategory::Security,
        rules: vec![
            "no-shell".into(),
            "no-raw".into(),
            "require-become-explicit".into(),
        ],
        parameters: vec![],
    }
}
