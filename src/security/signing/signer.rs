//! Artifact signing
//!
//! Produces [`SignatureBundle`] values that attest to the integrity and
//! provenance of arbitrary byte data or on-disk files.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::keys::{SigningAlgorithm, SigningKeyPair};

/// A self-contained signature bundle that can be stored alongside an artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureBundle {
    /// Identifier of the key that produced this signature.
    pub key_id: String,
    /// Algorithm used.
    pub algorithm: SigningAlgorithm,
    /// Hex-encoded signature bytes.
    pub signature_hex: String,
    /// Hex-encoded blake3 hash of the artifact content.
    pub artifact_hash: String,
    /// When the signature was created.
    pub timestamp: DateTime<Utc>,
}

/// Signs artifacts and produces [`SignatureBundle`] values.
#[derive(Debug, Default)]
pub struct ArtifactSigner;

impl ArtifactSigner {
    /// Create a new signer.
    pub fn new() -> Self {
        Self
    }

    /// Sign raw bytes with the given key.
    pub fn sign_bytes(&self, data: &[u8], key: &SigningKeyPair) -> SignatureBundle {
        let artifact_hash = blake3::hash(data);
        let signature = key.sign(data);

        SignatureBundle {
            key_id: key.id.clone(),
            algorithm: key.algorithm,
            signature_hex: hex_encode(&signature),
            artifact_hash: artifact_hash.to_hex().to_string(),
            timestamp: Utc::now(),
        }
    }

    /// Sign a file at `path` with the given key.
    ///
    /// Reads the entire file into memory, hashes it, and signs it.
    pub fn sign_file(&self, path: &Path, key: &SigningKeyPair) -> std::io::Result<SignatureBundle> {
        let data = std::fs::read(path)?;
        Ok(self.sign_bytes(&data, key))
    }
}

/// Encode bytes as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::signing::keys::SigningKeyPair;

    #[test]
    fn test_sign_bytes_produces_valid_bundle() {
        let key = SigningKeyPair::generate("sign-test");
        let signer = ArtifactSigner::new();
        let data = b"artifact content";

        let bundle = signer.sign_bytes(data, &key);

        assert_eq!(bundle.key_id, "sign-test");
        assert_eq!(
            bundle.algorithm,
            super::super::keys::SigningAlgorithm::Blake3Hmac
        );
        assert!(!bundle.signature_hex.is_empty());
        assert!(!bundle.artifact_hash.is_empty());

        // artifact_hash should match blake3 of data.
        let expected_hash = blake3::hash(data).to_hex().to_string();
        assert_eq!(bundle.artifact_hash, expected_hash);
    }

    #[test]
    fn test_sign_file() {
        let key = SigningKeyPair::generate("file-test");
        let signer = ArtifactSigner::new();

        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("artifact.bin");
        std::fs::write(&file_path, b"file artifact content").unwrap();

        let bundle = signer.sign_file(&file_path, &key).unwrap();
        assert_eq!(bundle.key_id, "file-test");

        let expected_hash = blake3::hash(b"file artifact content").to_hex().to_string();
        assert_eq!(bundle.artifact_hash, expected_hash);
    }

    #[test]
    fn test_signature_hex_is_valid_hex() {
        let key = SigningKeyPair::generate("hex-test");
        let signer = ArtifactSigner::new();
        let bundle = signer.sign_bytes(b"data", &key);

        // Every character should be a hex digit.
        assert!(bundle.signature_hex.chars().all(|c| c.is_ascii_hexdigit()));
        // blake3 output is 32 bytes = 64 hex chars.
        assert_eq!(bundle.signature_hex.len(), 64);
    }
}
