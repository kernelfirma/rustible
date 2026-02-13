//! Forensics bundle serialization and verification
//!
//! Provides JSON export and structural verification for forensics bundles.

use super::collector::BundleData;

/// Handles exporting and verifying forensics bundles.
pub struct ForensicsBundle;

impl ForensicsBundle {
    /// Serialize a [`BundleData`] into a pretty-printed JSON string.
    pub fn export_json(data: &BundleData) -> String {
        serde_json::to_string_pretty(data)
            .unwrap_or_else(|e| format!("{{\"error\": \"serialization failed: {}\"}}", e))
    }

    /// Verify that a JSON string has the expected forensics bundle structure.
    ///
    /// Returns `true` if the JSON can be deserialized into a valid [`BundleData`]
    /// and has a non-empty manifest version.
    pub fn verify_bundle(json: &str) -> bool {
        match serde_json::from_str::<BundleData>(json) {
            Ok(data) => !data.manifest.version.is_empty(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::forensics::collector::{CollectorConfig, ForensicsCollector};

    #[test]
    fn test_export_and_verify_round_trip() {
        let collector = ForensicsCollector::new(CollectorConfig::default());
        let data = collector.collect();

        let json = ForensicsBundle::export_json(&data);
        assert!(
            ForensicsBundle::verify_bundle(&json),
            "exported bundle should pass verification"
        );
    }

    #[test]
    fn test_verify_rejects_invalid_json() {
        assert!(!ForensicsBundle::verify_bundle("not json at all"));
        assert!(!ForensicsBundle::verify_bundle("{}"));
        assert!(!ForensicsBundle::verify_bundle("{\"manifest\": {}}"));
    }
}
