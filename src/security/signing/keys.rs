//! Key management for artifact signing
//!
//! Provides key generation, storage, and retrieval using blake3 keyed hashing
//! as the HMAC-based signing primitive.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Signing algorithm used for artifact signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SigningAlgorithm {
    /// HMAC-based signing using blake3 keyed hashing (32-byte key).
    Blake3Hmac,
}

impl fmt::Display for SigningAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SigningAlgorithm::Blake3Hmac => write!(f, "blake3-hmac"),
        }
    }
}

/// A unique identifier for a signing key.
pub type KeyId = String;

/// A signing key pair based on blake3 keyed hashing.
///
/// The key is a 32-byte secret used for both signing and verification
/// (symmetric HMAC scheme).
#[derive(Clone)]
pub struct SigningKeyPair {
    /// Human-readable identifier for this key.
    pub id: KeyId,
    /// The algorithm this key uses.
    pub algorithm: SigningAlgorithm,
    /// The 32-byte secret key material.
    key_bytes: [u8; 32],
}

impl fmt::Debug for SigningKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigningKeyPair")
            .field("id", &self.id)
            .field("algorithm", &self.algorithm)
            .field("key_bytes", &"[REDACTED]")
            .finish()
    }
}

impl SigningKeyPair {
    /// Generate a new random signing key pair.
    pub fn generate(id: impl Into<String>) -> Self {
        use rand::RngCore;
        let mut key_bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key_bytes);
        Self {
            id: id.into(),
            algorithm: SigningAlgorithm::Blake3Hmac,
            key_bytes,
        }
    }

    /// Create a key pair from existing raw bytes.
    ///
    /// Returns `None` if the slice is not exactly 32 bytes.
    pub fn from_bytes(id: impl Into<String>, bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 32 {
            return None;
        }
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(bytes);
        Some(Self {
            id: id.into(),
            algorithm: SigningAlgorithm::Blake3Hmac,
            key_bytes,
        })
    }

    /// Return the public key bytes.
    ///
    /// For a symmetric HMAC scheme the "public" identifier is the blake3 hash
    /// of the secret key, so the actual secret is never exposed.
    pub fn public_key_bytes(&self) -> Vec<u8> {
        blake3::hash(&self.key_bytes).as_bytes().to_vec()
    }

    /// Sign arbitrary data, returning the raw signature bytes.
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new_keyed(&self.key_bytes);
        hasher.update(data);
        hasher.finalize().as_bytes().to_vec()
    }

    /// Verify that `signature` is valid for `data`.
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> bool {
        let expected = self.sign(data);
        // Constant-time comparison to avoid timing attacks.
        if expected.len() != signature.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (a, b) in expected.iter().zip(signature.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }

    /// Return the raw secret key bytes.
    ///
    /// This should be treated as sensitive material.
    pub fn secret_bytes(&self) -> &[u8; 32] {
        &self.key_bytes
    }
}

/// An in-memory store for signing keys.
#[derive(Debug, Default)]
pub struct KeyStore {
    keys: HashMap<KeyId, SigningKeyPair>,
}

impl KeyStore {
    /// Create an empty key store.
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    /// Add a key to the store, returning any previous key with the same id.
    pub fn add_key(&mut self, key: SigningKeyPair) -> Option<SigningKeyPair> {
        self.keys.insert(key.id.clone(), key)
    }

    /// Retrieve a key by its identifier.
    pub fn get_key(&self, id: &str) -> Option<&SigningKeyPair> {
        self.keys.get(id)
    }

    /// List all key identifiers in the store.
    pub fn list_keys(&self) -> Vec<&KeyId> {
        self.keys.keys().collect()
    }

    /// Return the number of keys in the store.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Check whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_sign_verify() {
        let kp = SigningKeyPair::generate("test-key-1");
        assert_eq!(kp.id, "test-key-1");
        assert_eq!(kp.algorithm, SigningAlgorithm::Blake3Hmac);

        let data = b"hello, artifact";
        let sig = kp.sign(data);
        assert!(kp.verify(data, &sig));

        // Tampered data must fail verification.
        assert!(!kp.verify(b"tampered", &sig));
    }

    #[test]
    fn test_from_bytes_roundtrip() {
        let kp = SigningKeyPair::generate("roundtrip");
        let restored = SigningKeyPair::from_bytes("roundtrip", kp.secret_bytes()).unwrap();

        let data = b"roundtrip-test";
        let sig = kp.sign(data);
        assert!(restored.verify(data, &sig));
    }

    #[test]
    fn test_from_bytes_rejects_wrong_length() {
        assert!(SigningKeyPair::from_bytes("bad", &[0u8; 16]).is_none());
        assert!(SigningKeyPair::from_bytes("bad", &[]).is_none());
    }

    #[test]
    fn test_public_key_bytes_is_stable() {
        let kp = SigningKeyPair::generate("stable");
        let pk1 = kp.public_key_bytes();
        let pk2 = kp.public_key_bytes();
        assert_eq!(pk1, pk2);
        // Public key should differ from raw secret.
        assert_ne!(pk1.as_slice(), kp.secret_bytes().as_slice());
    }

    #[test]
    fn test_keystore_operations() {
        let mut store = KeyStore::new();
        assert!(store.is_empty());

        let k1 = SigningKeyPair::generate("key-a");
        let k2 = SigningKeyPair::generate("key-b");

        store.add_key(k1);
        store.add_key(k2);

        assert_eq!(store.len(), 2);
        assert!(store.get_key("key-a").is_some());
        assert!(store.get_key("key-b").is_some());
        assert!(store.get_key("key-c").is_none());

        let ids = store.list_keys();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn test_keystore_add_replaces_existing() {
        let mut store = KeyStore::new();
        let k1 = SigningKeyPair::generate("dup");
        let k2 = SigningKeyPair::generate("dup");

        let prev = store.add_key(k1);
        assert!(prev.is_none());

        let prev = store.add_key(k2);
        assert!(prev.is_some());
        assert_eq!(store.len(), 1);
    }
}
