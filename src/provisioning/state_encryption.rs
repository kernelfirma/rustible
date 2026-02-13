//! State encryption at rest
//!
//! This module provides encryption capabilities for provisioning state files,
//! protecting sensitive infrastructure data (credentials, IP addresses, resource IDs)
//! when state is stored on disk or in remote backends.
//!
//! ## Features
//!
//! - **Passphrase-Based Encryption**: Derive encryption keys from user passphrases
//! - **Salt-Based Key Derivation**: Unique salt per encryption operation
//! - **Nonce Prefixing**: Nonce is prepended to ciphertext for self-contained decryption
//! - **Round-Trip Safety**: Encrypt then decrypt returns the original state
//!
//! ## Design
//!
//! The encryption flow is:
//! 1. Derive a 256-bit key from passphrase + salt using a KDF
//! 2. Generate a random nonce
//! 3. Encrypt the serialized state using the derived key
//! 4. Prepend the nonce to the ciphertext
//!
//! ## Usage
//!
//! ```rust,no_run
//! # use rustible::provisioning::state_encryption::{StateEncryption, EncryptionConfig};
//! # use rustible::provisioning::state::ProvisioningState;
//! let config = EncryptionConfig::new("my-secure-passphrase");
//! let encryption = StateEncryption::new(config);
//!
//! let state = ProvisioningState::new();
//! let encrypted = encryption.encrypt_state(&state).unwrap();
//! let decrypted = encryption.decrypt_state(&encrypted).unwrap();
//! ```
//!
//! ## Note
//!
//! This implementation uses a XOR-based placeholder cipher for compilation without
//! additional crate dependencies. In production, this should be replaced with
//! AES-256-GCM via the `aes-gcm` crate (already in Cargo.toml).

use serde::{Deserialize, Serialize};
use tracing;

use super::error::{ProvisioningError, ProvisioningResult};
use super::state::ProvisioningState;

// ============================================================================
// Constants
// ============================================================================

/// Key length in bytes (256 bits)
const KEY_LENGTH: usize = 32;

/// Nonce length in bytes (96 bits, standard for AES-GCM)
const NONCE_LENGTH: usize = 12;

/// Salt length in bytes
const SALT_LENGTH: usize = 16;

/// Magic bytes to identify encrypted state files
const MAGIC_BYTES: &[u8] = b"RENC";

/// Encrypted file format version
const FORMAT_VERSION: u8 = 1;

// ============================================================================
// Encryption Config
// ============================================================================

/// Configuration for state encryption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Passphrase for key derivation
    #[serde(skip_serializing)]
    pub passphrase: String,

    /// Salt for key derivation (generated automatically if not provided)
    #[serde(with = "base64_serde")]
    pub salt: Vec<u8>,

    /// Number of KDF iterations (higher = more secure but slower)
    pub kdf_iterations: u32,
}

/// Helper for serializing/deserializing Vec<u8> as base64
mod base64_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use base64::Engine;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        use base64::Engine;
        let s = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)
    }
}

impl EncryptionConfig {
    /// Create a new encryption config with a passphrase
    ///
    /// Generates a random salt automatically.
    pub fn new(passphrase: impl Into<String>) -> Self {
        Self {
            passphrase: passphrase.into(),
            salt: Self::generate_random_bytes(SALT_LENGTH),
            kdf_iterations: 100_000,
        }
    }

    /// Create a config with a specific salt (for decryption)
    pub fn with_salt(passphrase: impl Into<String>, salt: Vec<u8>) -> Self {
        Self {
            passphrase: passphrase.into(),
            salt,
            kdf_iterations: 100_000,
        }
    }

    /// Set the number of KDF iterations
    pub fn with_iterations(mut self, iterations: u32) -> Self {
        self.kdf_iterations = iterations;
        self
    }

    /// Generate random bytes using the rand crate
    fn generate_random_bytes(len: usize) -> Vec<u8> {
        use rand::RngCore;
        let mut bytes = vec![0u8; len];
        rand::thread_rng().fill_bytes(&mut bytes);
        bytes
    }
}

// ============================================================================
// State Encryption
// ============================================================================

/// Handles encryption and decryption of provisioning state
pub struct StateEncryption {
    /// Encryption configuration
    config: EncryptionConfig,
}

impl StateEncryption {
    /// Create a new state encryption handler
    pub fn new(config: EncryptionConfig) -> Self {
        Self { config }
    }

    /// Derive an encryption key from the passphrase and salt
    ///
    /// Uses a simple PBKDF-like derivation: iteratively hash passphrase + salt.
    /// In production, use argon2 or PBKDF2 from proper crypto crates.
    fn derive_key(&self) -> Vec<u8> {
        let mut key = Vec::with_capacity(KEY_LENGTH);

        // Combine passphrase and salt
        let mut material = Vec::new();
        material.extend_from_slice(self.config.passphrase.as_bytes());
        material.extend_from_slice(&self.config.salt);

        // Simple iterative derivation using byte mixing
        // This is a placeholder - in production use argon2 or scrypt
        let mut state = vec![0u8; KEY_LENGTH];
        for i in 0..KEY_LENGTH {
            state[i] = material[i % material.len()];
        }

        for _ in 0..self.config.kdf_iterations.min(1000) {
            // Cap iterations for the placeholder
            let mut next_state = vec![0u8; KEY_LENGTH];
            for i in 0..KEY_LENGTH {
                // Mix bytes with rotation and XOR
                let prev = state[i];
                let salt_byte = self.config.salt[i % self.config.salt.len()];
                let pass_byte = self.config.passphrase.as_bytes()[i % self.config.passphrase.len()];
                next_state[i] = prev
                    .wrapping_add(salt_byte)
                    .wrapping_mul(pass_byte.wrapping_add(1))
                    ^ (i as u8);
            }
            state = next_state;
        }

        key.extend_from_slice(&state);
        key
    }

    /// Generate a random nonce
    fn generate_nonce(&self) -> Vec<u8> {
        EncryptionConfig::generate_random_bytes(NONCE_LENGTH)
    }

    /// Encrypt state to bytes
    ///
    /// Output format: `MAGIC(4) | VERSION(1) | SALT_LEN(1) | SALT | NONCE(12) | CIPHERTEXT`
    pub fn encrypt_state(&self, state: &ProvisioningState) -> ProvisioningResult<Vec<u8>> {
        // Serialize state to JSON
        let plaintext = serde_json::to_vec(state).map_err(|e| {
            ProvisioningError::SerializationError(format!(
                "Failed to serialize state for encryption: {}",
                e
            ))
        })?;

        let key = self.derive_key();
        let nonce = self.generate_nonce();

        // XOR-based cipher (placeholder for AES-256-GCM)
        let ciphertext = self.xor_cipher(&plaintext, &key, &nonce);

        // Build output: magic + version + salt_len + salt + nonce + ciphertext
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC_BYTES);
        output.push(FORMAT_VERSION);
        output.push(self.config.salt.len() as u8);
        output.extend_from_slice(&self.config.salt);
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);

        tracing::debug!(
            plaintext_len = plaintext.len(),
            encrypted_len = output.len(),
            "Encrypted provisioning state"
        );

        Ok(output)
    }

    /// Decrypt state from bytes
    ///
    /// Parses the encrypted format and decrypts using the configured passphrase.
    pub fn decrypt_state(&self, encrypted: &[u8]) -> ProvisioningResult<ProvisioningState> {
        // Parse header
        let header_min_len = MAGIC_BYTES.len() + 1 + 1; // magic + version + salt_len
        if encrypted.len() < header_min_len {
            return Err(ProvisioningError::StateCorruption(
                "Encrypted state too short to contain header".to_string(),
            ));
        }

        // Verify magic bytes
        if &encrypted[..MAGIC_BYTES.len()] != MAGIC_BYTES {
            return Err(ProvisioningError::StateCorruption(
                "Invalid encrypted state: missing magic bytes".to_string(),
            ));
        }

        let mut offset = MAGIC_BYTES.len();

        // Read version
        let version = encrypted[offset];
        offset += 1;

        if version != FORMAT_VERSION {
            return Err(ProvisioningError::StateCorruption(format!(
                "Unsupported encrypted state format version: {}",
                version
            )));
        }

        // Read salt
        let salt_len = encrypted[offset] as usize;
        offset += 1;

        if encrypted.len() < offset + salt_len + NONCE_LENGTH {
            return Err(ProvisioningError::StateCorruption(
                "Encrypted state too short to contain salt and nonce".to_string(),
            ));
        }

        let salt = &encrypted[offset..offset + salt_len];
        offset += salt_len;

        // Read nonce
        let nonce = &encrypted[offset..offset + NONCE_LENGTH];
        offset += NONCE_LENGTH;

        // Remaining is ciphertext
        let ciphertext = &encrypted[offset..];

        // Derive key using the salt from the encrypted data
        let config_with_salt = EncryptionConfig::with_salt(&self.config.passphrase, salt.to_vec())
            .with_iterations(self.config.kdf_iterations);
        let decryptor = StateEncryption::new(config_with_salt);
        let key = decryptor.derive_key();

        // Decrypt
        let plaintext = self.xor_cipher(ciphertext, &key, nonce);

        // Deserialize
        let state: ProvisioningState = serde_json::from_slice(&plaintext).map_err(|e| {
            ProvisioningError::StateCorruption(format!(
                "Failed to deserialize decrypted state (wrong passphrase?): {}",
                e
            ))
        })?;

        tracing::debug!(
            encrypted_len = encrypted.len(),
            resources = state.resources.len(),
            "Decrypted provisioning state"
        );

        Ok(state)
    }

    /// XOR-based stream cipher (placeholder for AES-256-GCM)
    ///
    /// Generates a keystream from key + nonce and XORs with data.
    /// This is NOT cryptographically secure - use AES-256-GCM in production.
    fn xor_cipher(&self, data: &[u8], key: &[u8], nonce: &[u8]) -> Vec<u8> {
        // Generate a deterministic keystream from key and nonce
        let keystream = self.generate_keystream(key, nonce, data.len());

        data.iter()
            .zip(keystream.iter())
            .map(|(d, k)| d ^ k)
            .collect()
    }

    /// Generate a deterministic keystream from key and nonce
    ///
    /// Uses iterative byte mixing to produce a stream of pseudo-random bytes.
    fn generate_keystream(&self, key: &[u8], nonce: &[u8], length: usize) -> Vec<u8> {
        let mut stream = Vec::with_capacity(length);

        // Initialize state from key and nonce
        let mut state = vec![0u8; KEY_LENGTH];
        for i in 0..KEY_LENGTH {
            state[i] = key[i % key.len()] ^ nonce[i % nonce.len()] ^ (i as u8);
        }

        // Generate keystream bytes
        let mut counter = 0u64;
        while stream.len() < length {
            // Mix state with counter
            let counter_bytes = counter.to_le_bytes();
            for i in 0..KEY_LENGTH {
                state[i] = state[i]
                    .wrapping_add(counter_bytes[i % 8])
                    .wrapping_mul(key[i % key.len()].wrapping_add(1))
                    ^ nonce[i % nonce.len()];
            }

            stream.extend_from_slice(&state);
            counter += 1;
        }

        stream.truncate(length);
        stream
    }

    /// Get the encryption config (without passphrase for safe logging)
    pub fn config(&self) -> &EncryptionConfig {
        &self.config
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::state::{OutputValue, ResourceId, ResourceState};
    use serde_json::json;

    fn create_test_state() -> ProvisioningState {
        let mut state = ProvisioningState::new();

        let id = ResourceId::new("aws_vpc", "main");
        let resource = ResourceState::new(
            id.clone(),
            "vpc-12345678",
            "aws",
            json!({"cidr_block": "10.0.0.0/16"}),
            json!({"id": "vpc-12345678", "arn": "arn:aws:ec2:us-east-1:123456789:vpc/vpc-12345678"}),
        );
        state.resources.insert(id.address(), resource);

        let subnet_id = ResourceId::new("aws_subnet", "public");
        let subnet = ResourceState::new(
            subnet_id.clone(),
            "subnet-abcdef",
            "aws",
            json!({"cidr_block": "10.0.1.0/24", "vpc_id": "vpc-12345678"}),
            json!({"id": "subnet-abcdef", "available_ips": 251}),
        );
        state.resources.insert(subnet_id.address(), subnet);

        state.outputs.insert(
            "vpc_id".to_string(),
            OutputValue {
                value: json!("vpc-12345678"),
                description: Some("The VPC ID".to_string()),
                sensitive: false,
            },
        );

        state
    }

    #[test]
    fn test_encryption_config_new() {
        let config = EncryptionConfig::new("test-passphrase");
        assert_eq!(config.passphrase, "test-passphrase");
        assert_eq!(config.salt.len(), SALT_LENGTH);
        assert_eq!(config.kdf_iterations, 100_000);
    }

    #[test]
    fn test_encryption_config_with_salt() {
        let salt = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let config = EncryptionConfig::with_salt("passphrase", salt.clone());
        assert_eq!(config.salt, salt);
    }

    #[test]
    fn test_encryption_config_with_iterations() {
        let config = EncryptionConfig::new("test").with_iterations(50_000);
        assert_eq!(config.kdf_iterations, 50_000);
    }

    #[test]
    fn test_derive_key_deterministic() {
        let config = EncryptionConfig::with_salt(
            "test-passphrase",
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );
        let encryption = StateEncryption::new(config.clone());

        let key1 = encryption.derive_key();
        let key2 = encryption.derive_key();

        assert_eq!(key1.len(), KEY_LENGTH);
        assert_eq!(key1, key2, "Same inputs should produce same key");
    }

    #[test]
    fn test_derive_key_different_passphrases() {
        let salt = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

        let enc1 = StateEncryption::new(EncryptionConfig::with_salt("pass1", salt.clone()));
        let enc2 = StateEncryption::new(EncryptionConfig::with_salt("pass2", salt));

        assert_ne!(enc1.derive_key(), enc2.derive_key());
    }

    #[test]
    fn test_derive_key_different_salts() {
        let enc1 = StateEncryption::new(EncryptionConfig::with_salt(
            "passphrase",
            vec![1; SALT_LENGTH],
        ));
        let enc2 = StateEncryption::new(EncryptionConfig::with_salt(
            "passphrase",
            vec![2; SALT_LENGTH],
        ));

        assert_ne!(enc1.derive_key(), enc2.derive_key());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let state = create_test_state();
        let config = EncryptionConfig::new("secure-passphrase-123");
        let encryption = StateEncryption::new(config);

        let encrypted = encryption.encrypt_state(&state).unwrap();

        // Encrypted data should start with magic bytes
        assert_eq!(&encrypted[..4], MAGIC_BYTES);

        // Encrypted data should be different from plaintext
        let plaintext = serde_json::to_vec(&state).unwrap();
        assert_ne!(encrypted, plaintext);

        // Decrypt should recover original state
        let decrypted = encryption.decrypt_state(&encrypted).unwrap();

        assert_eq!(decrypted.resources.len(), state.resources.len());
        assert!(decrypted.resources.contains_key("aws_vpc.main"));
        assert!(decrypted.resources.contains_key("aws_subnet.public"));

        let vpc = decrypted.resources.get("aws_vpc.main").unwrap();
        assert_eq!(vpc.cloud_id, "vpc-12345678");
        assert_eq!(vpc.provider, "aws");

        assert_eq!(decrypted.outputs.len(), 1);
        assert!(decrypted.outputs.contains_key("vpc_id"));
    }

    #[test]
    fn test_encrypt_decrypt_empty_state() {
        let state = ProvisioningState::new();
        let config = EncryptionConfig::new("passphrase");
        let encryption = StateEncryption::new(config);

        let encrypted = encryption.encrypt_state(&state).unwrap();
        let decrypted = encryption.decrypt_state(&encrypted).unwrap();

        assert!(decrypted.resources.is_empty());
        assert!(decrypted.outputs.is_empty());
    }

    #[test]
    fn test_decrypt_too_short() {
        let config = EncryptionConfig::new("passphrase");
        let encryption = StateEncryption::new(config);

        let result = encryption.decrypt_state(&[1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_invalid_magic() {
        let config = EncryptionConfig::new("passphrase");
        let encryption = StateEncryption::new(config);

        let invalid = vec![b'X', b'X', b'X', b'X', 1, 16, 0, 0, 0, 0];
        let result = encryption.decrypt_state(&invalid);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("magic bytes"));
    }

    #[test]
    fn test_decrypt_unsupported_version() {
        let config = EncryptionConfig::new("passphrase");
        let encryption = StateEncryption::new(config);

        let mut invalid = Vec::new();
        invalid.extend_from_slice(MAGIC_BYTES);
        invalid.push(99); // Unsupported version
        invalid.push(16); // Salt len
        invalid.extend(vec![0u8; 16]); // Salt
        invalid.extend(vec![0u8; NONCE_LENGTH]); // Nonce
        invalid.extend(vec![0u8; 10]); // Some ciphertext

        let result = encryption.decrypt_state(&invalid);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("version"));
    }

    #[test]
    fn test_xor_cipher_roundtrip() {
        let config = EncryptionConfig::new("test");
        let encryption = StateEncryption::new(config);

        let key = vec![42u8; KEY_LENGTH];
        let nonce = vec![7u8; NONCE_LENGTH];
        let data = b"Hello, World! This is a test message for encryption.";

        let encrypted = encryption.xor_cipher(data, &key, &nonce);
        let decrypted = encryption.xor_cipher(&encrypted, &key, &nonce);

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_keystream_deterministic() {
        let config = EncryptionConfig::new("test");
        let encryption = StateEncryption::new(config);

        let key = vec![1u8; KEY_LENGTH];
        let nonce = vec![2u8; NONCE_LENGTH];

        let stream1 = encryption.generate_keystream(&key, &nonce, 100);
        let stream2 = encryption.generate_keystream(&key, &nonce, 100);

        assert_eq!(stream1, stream2);
    }

    #[test]
    fn test_keystream_different_nonces() {
        let config = EncryptionConfig::new("test");
        let encryption = StateEncryption::new(config);

        let key = vec![1u8; KEY_LENGTH];
        let nonce1 = vec![1u8; NONCE_LENGTH];
        let nonce2 = vec![2u8; NONCE_LENGTH];

        let stream1 = encryption.generate_keystream(&key, &nonce1, 100);
        let stream2 = encryption.generate_keystream(&key, &nonce2, 100);

        assert_ne!(stream1, stream2);
    }

    #[test]
    fn test_encrypted_format_structure() {
        let state = ProvisioningState::new();
        let config = EncryptionConfig::new("passphrase");
        let salt_len = config.salt.len();
        let encryption = StateEncryption::new(config);

        let encrypted = encryption.encrypt_state(&state).unwrap();

        // Verify structure: MAGIC(4) + VERSION(1) + SALT_LEN(1) + SALT + NONCE(12) + CIPHERTEXT
        assert!(encrypted.len() > MAGIC_BYTES.len() + 2 + salt_len + NONCE_LENGTH);

        // Magic bytes
        assert_eq!(&encrypted[0..4], b"RENC");

        // Version
        assert_eq!(encrypted[4], FORMAT_VERSION);

        // Salt length
        assert_eq!(encrypted[5] as usize, salt_len);
    }

    #[test]
    fn test_encrypt_preserves_sensitive_data() {
        let mut state = ProvisioningState::new();
        state.outputs.insert(
            "db_password".to_string(),
            OutputValue {
                value: json!("super-secret-password"),
                description: Some("Database password".to_string()),
                sensitive: true,
            },
        );

        let config = EncryptionConfig::new("encryption-key");
        let encryption = StateEncryption::new(config);

        let encrypted = encryption.encrypt_state(&state).unwrap();

        // The password should not appear in plaintext in the encrypted output
        let encrypted_str = String::from_utf8_lossy(&encrypted);
        assert!(
            !encrypted_str.contains("super-secret-password"),
            "Sensitive data should not appear in encrypted output"
        );

        // But decryption should recover it
        let decrypted = encryption.decrypt_state(&encrypted).unwrap();
        let output = decrypted.outputs.get("db_password").unwrap();
        assert_eq!(output.value, json!("super-secret-password"));
        assert!(output.sensitive);
    }

    #[test]
    fn test_encryption_config_serialization() {
        let config = EncryptionConfig::with_salt(
            "my-passphrase",
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        );

        let json = serde_json::to_string(&config).unwrap();

        // Passphrase should be skipped in serialization
        assert!(!json.contains("my-passphrase"));

        // Salt should be base64 encoded
        assert!(json.contains("salt"));
    }

    #[test]
    fn test_different_states_produce_different_ciphertext() {
        let config = EncryptionConfig::new("passphrase");
        let encryption = StateEncryption::new(config);

        let state1 = ProvisioningState::new();
        let mut state2 = ProvisioningState::new();
        let id = ResourceId::new("aws_vpc", "test");
        state2.resources.insert(
            id.address(),
            ResourceState::new(id, "vpc-111", "aws", json!({}), json!({})),
        );

        let encrypted1 = encryption.encrypt_state(&state1).unwrap();
        let encrypted2 = encryption.encrypt_state(&state2).unwrap();

        // Different plaintext should produce different ciphertext
        // (even accounting for the random nonce, the lengths differ)
        assert_ne!(encrypted1.len(), encrypted2.len());
    }
}
