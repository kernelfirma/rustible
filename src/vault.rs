//! Vault for encrypted secrets management
//!
//! This module provides AES-256-GCM encryption with Argon2 key derivation
//! for storing sensitive data. The vault password is stored securely
//! using `SecretString` which zeroes memory on drop.

use crate::error::{Error, Result};
use crate::security::SecretString;
use aes_gcm::aead::generic_array::typenum;
use aes_gcm::{
    aead::{generic_array::GenericArray, Aead},
    Aes256Gcm, KeyInit,
};
use argon2::password_hash::SaltString;
use argon2::Argon2;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::rngs::OsRng;
use zeroize::Zeroizing;

/// Vault header marker
const VAULT_HEADER: &str = "$RUSTIBLE_VAULT;1.0;AES256";

/// Vault for encrypting/decrypting secrets
///
/// The password is stored in a `SecretString` which is automatically
/// zeroed from memory when dropped, preventing secret leakage.
pub struct Vault {
    password: SecretString,
}

impl Vault {
    /// Create a new vault with password
    pub fn new(password: impl Into<String>) -> Self {
        Self {
            password: SecretString::new(password),
        }
    }

    /// Encrypt content
    pub fn encrypt(&self, content: &str) -> Result<String> {
        let salt = SaltString::generate(&mut OsRng);
        let key = self.derive_key(&salt)?;

        let cipher = Aes256Gcm::new(&key);
        use rand::RngCore;
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = GenericArray::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, content.as_bytes())
            .map_err(|e| Error::Vault(format!("Encryption failed: {}", e)))?;

        let mut encrypted = Vec::new();
        encrypted.extend_from_slice(salt.as_str().as_bytes());
        encrypted.push(b'\n');
        encrypted.extend_from_slice(&nonce_bytes);
        encrypted.extend_from_slice(&ciphertext);

        Ok(format!("{}\n{}", VAULT_HEADER, BASE64.encode(&encrypted)))
    }

    /// Decrypt content
    pub fn decrypt(&self, content: &str) -> Result<String> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() || !lines[0].starts_with("$RUSTIBLE_VAULT") {
            return Err(Error::Vault("Invalid vault format".into()));
        }

        let encrypted = BASE64
            .decode(lines[1..].join(""))
            .map_err(|e| Error::Vault(format!("Base64 decode failed: {}", e)))?;

        // Parse salt, nonce, and ciphertext
        let salt_end = encrypted
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| Error::Vault("Invalid vault format".into()))?;
        let salt_str = std::str::from_utf8(&encrypted[..salt_end])
            .map_err(|_| Error::Vault("Invalid salt".into()))?;
        let salt =
            SaltString::from_b64(salt_str).map_err(|_| Error::Vault("Invalid salt".into()))?;

        let nonce_start = salt_end + 1;
        let nonce = GenericArray::from_slice(&encrypted[nonce_start..nonce_start + 12]);
        let ciphertext = &encrypted[nonce_start + 12..];

        let key = self.derive_key(&salt)?;
        let cipher = Aes256Gcm::new(&key);

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| Error::Vault("Decryption failed - wrong password?".into()))?;

        String::from_utf8(plaintext)
            .map_err(|_| Error::Vault("Invalid UTF-8 in decrypted content".into()))
    }

    /// Check if content is vault encrypted
    pub fn is_encrypted(content: &str) -> bool {
        content.starts_with("$RUSTIBLE_VAULT")
    }

    fn derive_key(&self, salt: &SaltString) -> Result<GenericArray<u8, typenum::U32>> {
        let argon2 = Argon2::default();
        // Use Zeroizing for the key buffer to ensure it's cleared after use
        let mut key = Zeroizing::new([0u8; 32]);
        argon2
            .hash_password_into(
                self.password.as_bytes(),
                salt.as_str().as_bytes(),
                &mut *key,
            )
            .map_err(|e| Error::Vault(format!("Key derivation failed: {}", e)))?;
        Ok(GenericArray::clone_from_slice(&*key))
    }
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_encryption_decryption() {
        let vault = Vault::new("secret_password");
        let content = "sensitive data";

        let encrypted = vault.encrypt(content).unwrap();
        assert!(Vault::is_encrypted(&encrypted));
        assert_ne!(content, encrypted);

        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(content, decrypted);
    }

    #[test]
    fn test_vault_wrong_password() {
        let vault1 = Vault::new("password_one");
        let vault2 = Vault::new("password_two");
        let content = "sensitive data";

        let encrypted = vault1.encrypt(content).unwrap();
        let result = vault2.decrypt(&encrypted);

        assert!(result.is_err());
    }

    #[test]
    fn test_vault_debug_redacts_password() {
        let vault = Vault::new("super_secret");
        let debug_output = format!("{:?}", vault);
        assert!(debug_output.contains("[REDACTED]"));
        assert!(!debug_output.contains("super_secret"));
    }
}
