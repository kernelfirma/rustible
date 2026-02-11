//! Artifact verification
//!
//! Verifies [`SignatureBundle`] values against artifact content and signing
//! keys, producing a [`VerificationResult`].

use serde::{Deserialize, Serialize};

use super::keys::SigningKeyPair;
use super::signer::SignatureBundle;
use super::trust::TrustPolicy;

/// Trust level assigned to a verified signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    /// The key is explicitly trusted by the active policy.
    Trusted,
    /// The key is not in the trusted set but is not revoked either.
    Unknown,
    /// The key has been revoked and should not be accepted.
    Revoked,
}

/// Result of verifying an artifact signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the cryptographic signature is valid.
    pub valid: bool,
    /// The key identifier used.
    pub key_id: String,
    /// Trust level of the signing key.
    pub trust_level: TrustLevel,
    /// Human-readable message describing the outcome.
    pub message: String,
}

/// Verifies artifact signatures.
#[derive(Debug, Default)]
pub struct ArtifactVerifier;

impl ArtifactVerifier {
    /// Create a new verifier.
    pub fn new() -> Self {
        Self
    }

    /// Verify `data` against a [`SignatureBundle`] using the given key.
    ///
    /// Checks both the artifact hash and the cryptographic signature.
    pub fn verify(
        &self,
        data: &[u8],
        bundle: &SignatureBundle,
        key: &SigningKeyPair,
    ) -> VerificationResult {
        self.verify_with_policy(data, bundle, key, None)
    }

    /// Verify with an optional [`TrustPolicy`] to determine trust level.
    pub fn verify_with_policy(
        &self,
        data: &[u8],
        bundle: &SignatureBundle,
        key: &SigningKeyPair,
        policy: Option<&TrustPolicy>,
    ) -> VerificationResult {
        // 1. Check artifact hash matches.
        let actual_hash = blake3::hash(data).to_hex().to_string();
        if actual_hash != bundle.artifact_hash {
            return VerificationResult {
                valid: false,
                key_id: bundle.key_id.clone(),
                trust_level: TrustLevel::Unknown,
                message: "Artifact hash mismatch - content has been tampered with".into(),
            };
        }

        // 2. Decode the signature hex and verify.
        let sig_bytes = match hex_decode(&bundle.signature_hex) {
            Some(b) => b,
            None => {
                return VerificationResult {
                    valid: false,
                    key_id: bundle.key_id.clone(),
                    trust_level: TrustLevel::Unknown,
                    message: "Invalid hex encoding in signature".into(),
                };
            }
        };

        if !key.verify(data, &sig_bytes) {
            return VerificationResult {
                valid: false,
                key_id: bundle.key_id.clone(),
                trust_level: TrustLevel::Unknown,
                message: "Signature verification failed - wrong key or tampered data".into(),
            };
        }

        // 3. Determine trust level.
        let trust_level = match policy {
            Some(p) => p.evaluate(&bundle.key_id),
            None => TrustLevel::Unknown,
        };

        let message = match trust_level {
            TrustLevel::Trusted => "Signature valid - key is trusted".into(),
            TrustLevel::Unknown => "Signature valid - key trust is unknown".into(),
            TrustLevel::Revoked => "Signature valid but key has been revoked".into(),
        };

        VerificationResult {
            valid: true,
            key_id: bundle.key_id.clone(),
            trust_level,
            message,
        }
    }
}

/// Decode a hex string into bytes. Returns `None` on invalid input.
fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for chunk in hex.as_bytes().chunks(2) {
        let hi = hex_val(chunk[0])?;
        let lo = hex_val(chunk[1])?;
        bytes.push((hi << 4) | lo);
    }
    Some(bytes)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::signing::keys::SigningKeyPair;
    use crate::security::signing::signer::ArtifactSigner;

    #[test]
    fn test_verify_valid_signature() {
        let key = SigningKeyPair::generate("verify-ok");
        let signer = ArtifactSigner::new();
        let verifier = ArtifactVerifier::new();

        let data = b"trusted artifact";
        let bundle = signer.sign_bytes(data, &key);
        let result = verifier.verify(data, &bundle, &key);

        assert!(result.valid);
        assert_eq!(result.key_id, "verify-ok");
    }

    #[test]
    fn test_verify_tampered_content() {
        let key = SigningKeyPair::generate("verify-tamper");
        let signer = ArtifactSigner::new();
        let verifier = ArtifactVerifier::new();

        let data = b"original content";
        let bundle = signer.sign_bytes(data, &key);

        let tampered = b"modified content";
        let result = verifier.verify(tampered, &bundle, &key);

        assert!(!result.valid);
        assert!(result.message.contains("hash mismatch"));
    }

    #[test]
    fn test_verify_wrong_key() {
        let key1 = SigningKeyPair::generate("key-1");
        let key2 = SigningKeyPair::generate("key-2");
        let signer = ArtifactSigner::new();
        let verifier = ArtifactVerifier::new();

        let data = b"some data";
        let bundle = signer.sign_bytes(data, &key1);

        // Re-create a bundle with correct hash but sign under key1,
        // then verify with key2 -- should fail.
        let result = verifier.verify(data, &bundle, &key2);
        assert!(!result.valid);
        assert!(result.message.contains("Signature verification failed"));
    }

    #[test]
    fn test_verify_with_trust_policy() {
        let key = SigningKeyPair::generate("trusted-key");
        let signer = ArtifactSigner::new();
        let verifier = ArtifactVerifier::new();

        let mut policy = TrustPolicy::default();
        policy.trusted_keys.insert("trusted-key".into());

        let data = b"policy artifact";
        let bundle = signer.sign_bytes(data, &key);
        let result = verifier.verify_with_policy(data, &bundle, &key, Some(&policy));

        assert!(result.valid);
        assert_eq!(result.trust_level, TrustLevel::Trusted);
    }

    #[test]
    fn test_verify_revoked_key() {
        let key = SigningKeyPair::generate("revoked-key");
        let signer = ArtifactSigner::new();
        let verifier = ArtifactVerifier::new();

        let mut policy = TrustPolicy::default();
        policy.revoked_keys.insert("revoked-key".into());

        let data = b"revoked artifact";
        let bundle = signer.sign_bytes(data, &key);
        let result = verifier.verify_with_policy(data, &bundle, &key, Some(&policy));

        assert!(result.valid); // Cryptographically valid ...
        assert_eq!(result.trust_level, TrustLevel::Revoked); // ... but revoked.
    }
}
