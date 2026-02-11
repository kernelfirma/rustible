//! Trust policy evaluation
//!
//! Defines which signing keys are trusted, unknown, or revoked.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::verifier::TrustLevel;

/// A trust policy that governs which signing keys are accepted.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrustPolicy {
    /// Set of key identifiers that are explicitly trusted.
    pub trusted_keys: HashSet<String>,
    /// Whether a valid signature is required for all artifacts.
    #[serde(default)]
    pub require_signature: bool,
    /// Set of key identifiers that have been revoked.
    #[serde(default)]
    pub revoked_keys: HashSet<String>,
}

impl TrustPolicy {
    /// Check if a key is in the trusted set.
    pub fn is_trusted(&self, key_id: &str) -> bool {
        self.trusted_keys.contains(key_id)
    }

    /// Check if a key has been revoked.
    pub fn is_revoked(&self, key_id: &str) -> bool {
        self.revoked_keys.contains(key_id)
    }

    /// Evaluate the trust level for a given key identifier.
    ///
    /// Revocation takes precedence over trust.
    pub fn evaluate(&self, key_id: &str) -> TrustLevel {
        if self.is_revoked(key_id) {
            TrustLevel::Revoked
        } else if self.is_trusted(key_id) {
            TrustLevel::Trusted
        } else {
            TrustLevel::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_trust_levels() {
        let mut policy = TrustPolicy::default();
        policy.trusted_keys.insert("good-key".into());
        policy.revoked_keys.insert("bad-key".into());

        assert_eq!(policy.evaluate("good-key"), TrustLevel::Trusted);
        assert_eq!(policy.evaluate("bad-key"), TrustLevel::Revoked);
        assert_eq!(policy.evaluate("unknown-key"), TrustLevel::Unknown);
    }

    #[test]
    fn test_revocation_takes_precedence() {
        let mut policy = TrustPolicy::default();
        policy.trusted_keys.insert("both".into());
        policy.revoked_keys.insert("both".into());

        // Revoked wins when a key appears in both sets.
        assert_eq!(policy.evaluate("both"), TrustLevel::Revoked);
    }

    #[test]
    fn test_is_trusted_and_is_revoked() {
        let mut policy = TrustPolicy::default();
        policy.trusted_keys.insert("t".into());
        policy.revoked_keys.insert("r".into());

        assert!(policy.is_trusted("t"));
        assert!(!policy.is_trusted("r"));
        assert!(policy.is_revoked("r"));
        assert!(!policy.is_revoked("t"));
    }
}
