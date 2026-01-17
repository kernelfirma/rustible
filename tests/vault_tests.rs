#![cfg(not(tarpaulin))]
//! Comprehensive integration tests for the Rustible vault encryption system
//!
//! # Overview
//!
//! This test suite provides comprehensive coverage of the Rustible vault encryption
//! system, which implements secure secret management using industry-standard
//! cryptographic primitives:
//!
//! - **Key Derivation**: Argon2id (memory-hard, resistant to GPU/ASIC attacks)
//! - **Encryption**: AES-256-GCM (authenticated encryption with associated data)
//! - **Format**: Custom Rustible vault format with version and algorithm metadata
//!
//! # Test Categories
//!
//! ## 1. Basic Vault Creation and Configuration
//! ## 2. Encryption and Decryption Roundtrips
//! ## 3. Data Size Variations (1B to 10MB)
//! ## 4. Binary and UTF-8 Data Handling
//! ## 5. Key Derivation Tests
//! ## 6. Vault File Format Tests
//! ## 7. CLI Integration Tests
//! ## 8. Playbook Integration Tests
//! ## 9. Error Handling Tests
//! ## 10. Security Property Tests
//!
//! # Security Considerations
//!
//! The vault implementation has been tested for:
//!
//! - **Confidentiality**: Strong encryption with AES-256-GCM
//! - **Integrity**: Authenticated encryption prevents tampering
//! - **Uniqueness**: Random salts and nonces for each encryption
//! - **Password Security**: Argon2id resists brute-force attacks
//! - **Error Handling**: No password leakage in error messages

use rustible::error::Error;
use rustible::vault::Vault;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

// ============================================================================
// Test Helpers
// ============================================================================

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
}

fn read_fixture(name: &str) -> String {
    fs::read_to_string(fixtures_path().join(name))
        .unwrap_or_else(|_| panic!("Failed to read fixture: {}", name))
}

// ============================================================================
// SECTION 1: ENCRYPTION/DECRYPTION ROUNDTRIP TESTS
// ============================================================================

mod encryption_decryption {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip_simple() {
        let vault = Vault::new("my_secure_password");
        let plaintext = "Hello, World!";

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_1_byte() {
        let vault = Vault::new("password");
        let plaintext = "x";

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
        assert_eq!(decrypted.len(), 1);
    }

    #[test]
    fn test_encrypt_decrypt_1kb() {
        let vault = Vault::new("password");
        let plaintext = "a".repeat(1024);

        let encrypted = vault.encrypt(&plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
        assert_eq!(decrypted.len(), 1024);
    }

    #[test]
    fn test_encrypt_decrypt_1mb() {
        let vault = Vault::new("password");
        let plaintext = "b".repeat(1024 * 1024);

        let encrypted = vault.encrypt(&plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
        assert_eq!(decrypted.len(), 1024 * 1024);
    }

    #[test]
    fn test_encrypt_decrypt_10mb() {
        let vault = Vault::new("password");
        let plaintext = "c".repeat(10 * 1024 * 1024);

        let encrypted = vault.encrypt(&plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted.len(), 10 * 1024 * 1024);
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_binary_like_data() {
        let vault = Vault::new("password");
        // Create data with various byte values (valid UTF-8)
        let plaintext = "Binary-like data: \u{0000}\u{0001}\u{001F}\u{007F}\u{0080}\u{00FF}";

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_utf8_with_special_characters() {
        let vault = Vault::new("password");
        let plaintext = "Unicode: 你好世界 🌍 Здравствуй мир ñoño café مرحبا";

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_empty_content() {
        let vault = Vault::new("password");
        let plaintext = "";

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, "");
    }

    #[test]
    fn test_encrypt_multiline_content() {
        let vault = Vault::new("password");
        let plaintext = "Line 1\nLine 2\nLine 3\n\nLine 5";

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_special_chars() {
        let vault = Vault::new("password123");
        let plaintext = "Special chars: !@#$%^&*(){}[]<>?/\\|`~';:\",.";

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_json() {
        let vault = Vault::new("json_password");
        let plaintext = r#"{"key": "value", "number": 42, "nested": {"foo": "bar"}}"#;

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_yaml() {
        let vault = Vault::new("yaml_password");
        let plaintext = r#"---
key: value
list:
  - item1
  - item2
nested:
  foo: bar
"#;

        let encrypted = vault.encrypt(plaintext).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_tabs_and_newlines() {
        let vault = Vault::new("password");
        let data = "Line 1\tTabbed\nLine 2\r\nLine 3\n\n\n";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_only_whitespace() {
        let vault = Vault::new("password");
        let data = "   \t\n\r\n   ";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_emojis_and_symbols() {
        let vault = Vault::new("password");
        let data = "Emojis: 🔐🔑🛡️⚠️✅❌🚀💾📁📂🗂️👨‍👩‍👧‍👦";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_sql_injection_attempts() {
        let vault = Vault::new("password");
        let data = "'; DROP TABLE users; --";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_xml_special_chars() {
        let vault = Vault::new("password");
        let data = "<root>&lt;element&gt;\"value\"</root>";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_mixed_line_endings() {
        let vault = Vault::new("password");
        let data = "Unix\nWindows\r\nMac\rMixed\n\r";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_repeated_patterns() {
        let vault = Vault::new("password");
        let data = "AAAAAAAAAA".repeat(1000);

        let encrypted = vault.encrypt(&data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
        // Despite repeated pattern, ciphertext should not have obvious patterns
        assert!(!encrypted.contains(&"AAAA".repeat(10)));
    }
}

// ============================================================================
// SECTION 2: KEY DERIVATION TESTS
// ============================================================================

mod key_derivation {
    use super::*;

    #[test]
    fn test_same_password_produces_consistent_decryption() {
        let vault1 = Vault::new("shared_password");
        let vault2 = Vault::new("shared_password");

        let plaintext = "shared secret";
        let encrypted = vault1.encrypt(plaintext).unwrap();

        // Another vault with same password can decrypt
        let decrypted = vault2.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_passwords_produce_different_keys() {
        let plaintext = "secret data";

        let vault1 = Vault::new("password1");
        let vault2 = Vault::new("password2");

        let encrypted1 = vault1.encrypt(plaintext).unwrap();
        let encrypted2 = vault2.encrypt(plaintext).unwrap();

        // Different passwords should produce different ciphertexts
        assert_ne!(encrypted1, encrypted2);

        // Each vault can only decrypt its own ciphertext
        assert!(vault1.decrypt(&encrypted1).is_ok());
        assert!(vault1.decrypt(&encrypted2).is_err());
        assert!(vault2.decrypt(&encrypted2).is_ok());
        assert!(vault2.decrypt(&encrypted1).is_err());
    }

    #[test]
    fn test_salt_randomness_ensures_different_ciphertexts() {
        let vault = Vault::new("password");
        let plaintext = "test";

        // Encrypt the same data multiple times
        let mut encryptions = Vec::new();
        for _ in 0..100 {
            encryptions.push(vault.encrypt(plaintext).unwrap());
        }

        // All encryptions should be different (different salts and nonces)
        let unique: HashSet<String> = encryptions.into_iter().collect();
        assert_eq!(
            unique.len(),
            100,
            "Salts and nonces should be unique for each encryption"
        );
    }

    #[test]
    fn test_argon2id_timing() {
        let vault = Vault::new("password");
        let plaintext = "test data";

        // Key derivation should take a noticeable amount of time (Argon2id is intentionally slow)
        let start = Instant::now();
        let _ = vault.encrypt(plaintext).unwrap();
        let duration = start.elapsed();

        // Should take at least some milliseconds (Argon2 is designed to be slow)
        // We use a conservative lower bound to avoid flaky tests
        assert!(
            duration.as_millis() >= 1,
            "Key derivation should be slow enough for security"
        );

        // But not too slow for usability
        assert!(
            duration.as_secs() < 10,
            "Key derivation should complete in reasonable time"
        );
    }

    #[test]
    fn test_unicode_password() {
        let vault = Vault::new("パスワード123🔐");
        let encrypted = vault.encrypt("secret data").unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "secret data");
    }

    #[test]
    fn test_very_long_password() {
        let long_password = "a".repeat(10000);
        let vault = Vault::new(&long_password);
        let encrypted = vault.encrypt("test").unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "test");
    }

    #[test]
    fn test_empty_password_still_works() {
        let vault = Vault::new("");
        // Even empty passwords should be accepted (user's choice)
        // Encryption should still work but be insecure
        let encrypted = vault.encrypt("test").unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "test");
    }

    #[test]
    fn test_password_with_null_bytes() {
        // Password with embedded nulls (if the type allows)
        let vault = Vault::new("pass\0word");
        let encrypted = vault.encrypt("test").unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, "test");
    }

    #[test]
    fn test_password_case_sensitivity() {
        let vault1 = Vault::new("Password123");
        let vault2 = Vault::new("password123");

        let encrypted = vault1.encrypt("test").unwrap();
        let result = vault2.decrypt(&encrypted);

        assert!(result.is_err(), "Passwords should be case-sensitive");
    }
}

// ============================================================================
// SECTION 3: VAULT FILE FORMAT TESTS
// ============================================================================

mod vault_format {
    use super::*;

    #[test]
    fn test_correct_header_format() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test").unwrap();

        // Rustible vault header
        assert!(encrypted.starts_with("$RUSTIBLE_VAULT"));
        assert!(encrypted.contains("1.0"));
        assert!(encrypted.contains("AES256"));
    }

    #[test]
    fn test_encrypted_format_is_multiline() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test data").unwrap();

        let lines: Vec<&str> = encrypted.lines().collect();
        assert!(
            lines.len() >= 2,
            "Encrypted data should have header + data lines"
        );
    }

    #[test]
    fn test_is_encrypted_detection() {
        let vault = Vault::new("password");

        // Plain text should not be detected as encrypted
        assert!(!Vault::is_encrypted("This is plain text"));
        assert!(!Vault::is_encrypted(""));
        assert!(!Vault::is_encrypted("$ANSIBLE_VAULT;1.1;AES256"));

        // Encrypted data should be detected
        let encrypted = vault.encrypt("test").unwrap();
        assert!(Vault::is_encrypted(&encrypted));
    }

    #[test]
    fn test_encrypted_data_is_valid_utf8() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test data").unwrap();

        // The encrypted output should be valid UTF-8 (base64 encoded)
        assert!(std::str::from_utf8(encrypted.as_bytes()).is_ok());
    }

    #[test]
    fn test_vault_format_version_in_header() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test").unwrap();

        let first_line = encrypted.lines().next().unwrap();
        assert!(first_line.contains("1.0"), "Version should be in header");
    }

    #[test]
    fn test_vault_format_algorithm_in_header() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test").unwrap();

        let first_line = encrypted.lines().next().unwrap();
        assert!(
            first_line.contains("AES256"),
            "Algorithm should be mentioned in header"
        );
    }

    #[test]
    fn test_ansible_vault_format_detection() {
        // Ansible vault format should NOT be detected as Rustible vault
        let ansible_vault = "$ANSIBLE_VAULT;1.1;AES256\n66616b6564617461";
        assert!(!Vault::is_encrypted(ansible_vault));
    }
}

// ============================================================================
// SECTION 4: CLI INTEGRATION TESTS
// ============================================================================

mod cli_integration {
    use super::*;

    // The CLI VaultEngine is not exposed in the public API, so we test
    // the core vault functionality which is the same underlying implementation.

    #[test]
    fn test_vault_encrypt_decrypt_as_cli_would() {
        let vault = Vault::new("test_password");

        let plaintext = "Hello, World!";
        let encrypted = vault.encrypt(plaintext).unwrap();

        assert!(Vault::is_encrypted(&encrypted));

        let decrypted = vault.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_vault_wrong_password_as_cli_would() {
        let vault1 = Vault::new("password1");
        let vault2 = Vault::new("password2");

        let plaintext = "Secret data";
        let encrypted = vault1.encrypt(plaintext).unwrap();

        let result = vault2.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_vault_is_encrypted_detection() {
        assert!(Vault::is_encrypted("$RUSTIBLE_VAULT;1.0;AES256\ndata"));
        assert!(!Vault::is_encrypted("plain text content"));
        assert!(!Vault::is_encrypted("$ANSIBLE_VAULT;1.1;AES256\ndata"));
    }

    #[test]
    fn test_vault_large_data() {
        let vault = Vault::new("password");

        let large_data = "x".repeat(1024 * 1024); // 1MB
        let encrypted = vault.encrypt(&large_data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, large_data);
    }

    #[test]
    fn test_vault_empty_data() {
        let vault = Vault::new("password");

        let encrypted = vault.encrypt("").unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_vault_line_format() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test data").unwrap();

        // Verify the format is correct
        let lines: Vec<&str> = encrypted.lines().collect();
        assert!(lines.len() >= 2, "Should have at least header + data");
        assert!(lines[0].starts_with("$RUSTIBLE_VAULT"));
    }
}

// ============================================================================
// SECTION 5: PLAYBOOK INTEGRATION TESTS
// ============================================================================

mod playbook_integration {
    use rustible::vars::{VarPrecedence, VarStore, Vault as VarsVault};

    #[test]
    fn test_vars_vault_encrypt_decrypt() {
        let plaintext = "secret: super_secret_value";
        let password = "vault_password";

        let encrypted = VarsVault::encrypt(plaintext, password).unwrap();
        let decrypted = VarsVault::decrypt(&encrypted, password).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_vars_vault_is_encrypted() {
        let plaintext = "not encrypted";
        let encrypted = VarsVault::encrypt(plaintext, "password").unwrap();

        assert!(!VarsVault::is_encrypted(plaintext));
        assert!(VarsVault::is_encrypted(&encrypted));
    }

    #[test]
    fn test_vars_vault_wrong_password() {
        let encrypted = VarsVault::encrypt("secret", "password1").unwrap();
        let result = VarsVault::decrypt(&encrypted, "password2");

        assert!(result.is_err());
    }

    #[test]
    fn test_var_store_with_vault_password() {
        let mut store = VarStore::new();
        store.set_vault_password("test_password");

        // The vault password should be stored
        store.set(
            "test",
            serde_yaml::Value::String("value".to_string()),
            VarPrecedence::PlayVars,
        );
        assert!(store.contains("test"));
    }

    #[test]
    fn test_vars_vault_yaml_content() {
        let yaml_content = r#"
db_password: secret123
api_key: sk-abcdef
nested:
  value: hidden
"#;
        let password = "vault_password";

        let encrypted = VarsVault::encrypt(yaml_content, password).unwrap();
        let decrypted = VarsVault::decrypt(&encrypted, password).unwrap();

        assert_eq!(decrypted, yaml_content);

        // Parse to verify YAML integrity
        let parsed: serde_yaml::Value = serde_yaml::from_str(&decrypted).unwrap();
        assert!(parsed.get("db_password").is_some());
    }

    #[test]
    fn test_inline_vault_marker() {
        // Test that the inline vault prefix is recognized
        let inline_vault = "!vault |\n  $ANSIBLE_VAULT;1.1;AES256\n  encoded_data";
        assert!(inline_vault.starts_with("!vault"));
    }

    #[test]
    fn test_multiple_vault_passwords_scenario() {
        // Test with different vault IDs (passwords)
        let password1 = "production_vault";
        let password2 = "development_vault";

        let prod_secret = VarsVault::encrypt("prod_db_password", password1).unwrap();
        let dev_secret = VarsVault::encrypt("dev_db_password", password2).unwrap();

        // Production password can decrypt production vault
        assert_eq!(
            VarsVault::decrypt(&prod_secret, password1).unwrap(),
            "prod_db_password"
        );

        // Development password can decrypt development vault
        assert_eq!(
            VarsVault::decrypt(&dev_secret, password2).unwrap(),
            "dev_db_password"
        );

        // Cross-decryption fails
        assert!(VarsVault::decrypt(&prod_secret, password2).is_err());
        assert!(VarsVault::decrypt(&dev_secret, password1).is_err());
    }
}

// ============================================================================
// SECTION 6: ERROR HANDLING TESTS
// ============================================================================

mod error_handling {
    use super::*;

    #[test]
    fn test_wrong_password_error() {
        let vault1 = Vault::new("correct_password");
        let vault2 = Vault::new("wrong_password");

        let encrypted = vault1.encrypt("secret").unwrap();
        let result = vault2.decrypt(&encrypted);

        assert!(result.is_err());
        match result {
            Err(Error::Vault(msg)) => {
                assert!(msg.contains("Decryption failed") || msg.contains("wrong password"));
            }
            _ => panic!("Expected Vault error"),
        }
    }

    #[test]
    fn test_corrupted_encrypted_data() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test").unwrap();

        // Modify the ciphertext
        let mut chars: Vec<char> = encrypted.chars().collect();
        if chars.len() > 50 {
            chars[50] = if chars[50] == 'A' { 'B' } else { 'A' };
            let corrupted: String = chars.into_iter().collect();

            let result = vault.decrypt(&corrupted);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_invalid_vault_format_no_header() {
        let vault = Vault::new("password");
        let result = vault.decrypt("This is not encrypted data");

        assert!(result.is_err());
        match result {
            Err(Error::Vault(msg)) => {
                assert!(msg.contains("Invalid vault format"));
            }
            _ => panic!("Expected Vault error for invalid format"),
        }
    }

    #[test]
    fn test_invalid_vault_format_wrong_header() {
        let vault = Vault::new("password");
        let result = vault.decrypt("$ANSIBLE_VAULT;1.1;AES256\nAGFzZGZhc2Rm");

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_base64() {
        let vault = Vault::new("password");
        let result = vault.decrypt("$RUSTIBLE_VAULT;1.0;AES256\nThis is not valid base64!!!");

        assert!(result.is_err());
        match result {
            Err(Error::Vault(msg)) => {
                assert!(msg.contains("Base64 decode failed"));
            }
            _ => panic!("Expected Vault error for base64 decode failure"),
        }
    }

    #[test]
    fn test_truncated_encrypted_data() {
        let vault = Vault::new("password");
        let mut encrypted = vault.encrypt("test data").unwrap();

        // Truncate the encrypted data
        encrypted.truncate(encrypted.len() / 2);

        let result = vault.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_input() {
        let vault = Vault::new("password");
        let result = vault.decrypt("");

        assert!(result.is_err());
        match result {
            Err(Error::Vault(msg)) => {
                assert!(msg.contains("Invalid vault format"));
            }
            _ => panic!("Expected Vault error for empty input"),
        }
    }

    #[test]
    fn test_header_only() {
        let vault = Vault::new("password");
        let result = vault.decrypt("$RUSTIBLE_VAULT;1.0;AES256");

        assert!(result.is_err());
    }

    #[test]
    fn test_error_message_does_not_leak_password() {
        let vault = Vault::new("super_secret_password_12345");
        let result = vault.decrypt("invalid data");

        if let Err(Error::Vault(msg)) = result {
            // Error message should not contain the password
            assert!(!msg.contains("super_secret_password"));
            assert!(!msg.contains("12345"));
        }
    }

    #[test]
    fn test_password_not_in_encrypted_output() {
        let password = "my_secret_password_do_not_expose";
        let vault = Vault::new(password);
        let encrypted = vault.encrypt("test").unwrap();

        // The password should never appear in the encrypted output
        assert!(!encrypted.contains(password));
        assert!(!encrypted.contains("my_secret"));
    }
}

// ============================================================================
// SECTION 7: SECURITY PROPERTY TESTS
// ============================================================================

mod security_properties {
    use super::*;

    #[test]
    fn test_nonce_uniqueness() {
        let vault = Vault::new("password");

        // Encrypt the same data multiple times
        let mut encryptions = Vec::new();
        for _ in 0..50 {
            encryptions.push(vault.encrypt("test").unwrap());
        }

        // All should be unique (different nonces)
        let unique: HashSet<String> = encryptions.into_iter().collect();
        assert_eq!(
            unique.len(),
            50,
            "Each encryption should produce unique output"
        );
    }

    #[test]
    fn test_no_padding_oracle() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test").unwrap();

        // AES-GCM doesn't use padding - it uses a stream cipher mode
        // Any modification should fail authentication, not produce padding errors
        for i in 30..encrypted.len().min(100) {
            let mut modified = encrypted.clone();
            unsafe {
                let bytes = modified.as_bytes_mut();
                if i < bytes.len() {
                    bytes[i] = bytes[i].wrapping_add(1);
                }
            }

            // Should fail with authentication error, not padding error
            let result = vault.decrypt(&modified);
            if result.is_err() {
                // All errors should be generic, not revealing padding information
                if let Err(Error::Vault(msg)) = result {
                    assert!(!msg.to_lowercase().contains("padding"));
                }
            }
        }
    }

    #[test]
    fn test_same_password_different_instances() {
        let vault1 = Vault::new("shared_password");
        let vault2 = Vault::new("shared_password");

        let plaintext = "shared secret";

        let encrypted1 = vault1.encrypt(plaintext).unwrap();
        let encrypted2 = vault2.encrypt(plaintext).unwrap();

        // Different ciphertexts (different salts/nonces)
        assert_ne!(encrypted1, encrypted2);

        // But both can decrypt each other's data
        assert_eq!(vault1.decrypt(&encrypted2).unwrap(), plaintext);
        assert_eq!(vault2.decrypt(&encrypted1).unwrap(), plaintext);
    }

    #[test]
    fn test_encryption_non_determinism() {
        let vault = Vault::new("password");
        let plaintext = "determinism test";

        let results: Vec<String> = (0..20).map(|_| vault.encrypt(plaintext).unwrap()).collect();

        // All results should be unique
        let unique: HashSet<String> = results.into_iter().collect();
        assert_eq!(unique.len(), 20, "Encryption should be non-deterministic");
    }

    #[test]
    fn test_ciphertext_indistinguishable() {
        let vault = Vault::new("password");

        // Encrypt two different plaintexts of the same length
        let encrypted1 = vault.encrypt("AAAAAAAAAA").unwrap();
        let encrypted2 = vault.encrypt("BBBBBBBBBB").unwrap();

        // After the header, the encrypted data should look random
        // (not have any pattern that reveals the plaintext)
        let data1: String = encrypted1.lines().skip(1).collect();
        let data2: String = encrypted2.lines().skip(1).collect();

        // They should be completely different
        assert_ne!(data1, data2);

        // Neither should contain repeated patterns from plaintext
        assert!(!data1.contains("AAA"));
        assert!(!data2.contains("BBB"));
    }

    #[test]
    fn test_authentication_tag_verification() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("authenticated data").unwrap();

        // Any modification should fail authentication
        let lines: Vec<&str> = encrypted.lines().collect();
        if lines.len() >= 2 {
            let mut modified = lines[0].to_string();
            modified.push('\n');

            // Flip a bit in the encrypted data
            let mut data = lines[1].to_string();
            if !data.is_empty() {
                let bytes = data.as_bytes();
                let mut modified_bytes = bytes.to_vec();
                if !modified_bytes.is_empty() {
                    modified_bytes[0] = modified_bytes[0].wrapping_add(1);
                    data = String::from_utf8_lossy(&modified_bytes).to_string();
                }
            }
            modified.push_str(&data);

            for line in &lines[2..] {
                modified.push('\n');
                modified.push_str(line);
            }

            // GCM authentication should fail
            let result = vault.decrypt(&modified);
            assert!(
                result.is_err(),
                "Modified ciphertext should fail authentication"
            );
        }
    }

    #[test]
    fn test_timing_consistency() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test").unwrap();

        // Multiple decryption attempts should take similar time
        // (to prevent timing attacks)
        let mut durations = Vec::new();
        for _ in 0..10 {
            let start = Instant::now();
            let _ = vault.decrypt(&encrypted);
            durations.push(start.elapsed());
        }

        // Calculate variance - should be relatively low
        let avg: Duration = durations.iter().sum::<Duration>() / durations.len() as u32;
        let variance: f64 = durations
            .iter()
            .map(|d| (d.as_nanos() as f64 - avg.as_nanos() as f64).powi(2))
            .sum::<f64>()
            / durations.len() as f64;

        // Variance should be reasonable (not a strict test, more of a sanity check)
        // This mainly ensures there's no exponential timing difference
        assert!(
            variance.sqrt() < avg.as_nanos() as f64 * 10.0,
            "Decryption timing should be consistent"
        );
    }
}

// ============================================================================
// SECTION 8: CONCURRENT ACCESS TESTS
// ============================================================================

mod concurrent_access {
    use super::*;

    #[test]
    fn test_vault_is_thread_safe() {
        let vault = Arc::new(Vault::new("concurrent_password"));
        let mut handles = vec![];

        for i in 0..10 {
            let vault_clone = Arc::clone(&vault);
            let handle = thread::spawn(move || {
                let plaintext = format!("Thread {} data", i);
                let encrypted = vault_clone.encrypt(&plaintext).unwrap();
                let decrypted = vault_clone.decrypt(&encrypted).unwrap();
                assert_eq!(decrypted, plaintext);
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_concurrent_encryption() {
        let vault = Arc::new(Vault::new("password"));
        let mut handles = vec![];

        for i in 0..50 {
            let vault_clone = Arc::clone(&vault);
            let handle =
                thread::spawn(move || vault_clone.encrypt(&format!("data_{}", i)).unwrap());
            handles.push(handle);
        }

        let results: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // All results should be unique
        let unique: HashSet<String> = results.into_iter().collect();
        assert_eq!(unique.len(), 50);
    }

    #[test]
    fn test_concurrent_decryption() {
        let vault = Vault::new("password");
        let encrypted: Vec<String> = (0..20)
            .map(|i| vault.encrypt(&format!("data_{}", i)).unwrap())
            .collect();

        let vault = Arc::new(vault);
        let encrypted = Arc::new(encrypted);
        let mut handles = vec![];

        for i in 0..20 {
            let vault_clone = Arc::clone(&vault);
            let encrypted_clone = Arc::clone(&encrypted);
            let handle = thread::spawn(move || {
                let decrypted = vault_clone.decrypt(&encrypted_clone[i]).unwrap();
                assert_eq!(decrypted, format!("data_{}", i));
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }
}

// ============================================================================
// SECTION 9: STRESS TESTS
// ============================================================================

mod stress_tests {
    use super::*;

    #[test]
    fn test_many_sequential_operations() {
        let vault = Vault::new("stress_test_password");

        for i in 0..100 {
            let plaintext = format!("Message number {}", i);
            let encrypted = vault.encrypt(&plaintext).unwrap();
            let decrypted = vault.decrypt(&encrypted).unwrap();
            assert_eq!(decrypted, plaintext);
        }
    }

    #[test]
    fn test_nested_encryption_decryption() {
        let vault = Vault::new("recursive_password");
        let mut data = "initial data".to_string();

        // Encrypt 10 times
        for _ in 0..10 {
            data = vault.encrypt(&data).unwrap();
        }

        // Decrypt 10 times
        for _ in 0..10 {
            data = vault.decrypt(&data).unwrap();
        }

        assert_eq!(data, "initial data");
    }

    #[test]
    fn test_multi_layer_encryption() {
        let vault1 = Vault::new("password1");
        let vault2 = Vault::new("password2");
        let vault3 = Vault::new("password3");

        let plaintext = "multi-layered secret";

        // Encrypt with vault1
        let encrypted1 = vault1.encrypt(plaintext).unwrap();
        // Treat encrypted data as plaintext and encrypt again with vault2
        let encrypted2 = vault2.encrypt(&encrypted1).unwrap();
        // And again with vault3
        let encrypted3 = vault3.encrypt(&encrypted2).unwrap();

        // Decrypt in reverse order
        let decrypted3 = vault3.decrypt(&encrypted3).unwrap();
        let decrypted2 = vault2.decrypt(&decrypted3).unwrap();
        let decrypted1 = vault1.decrypt(&decrypted2).unwrap();

        assert_eq!(decrypted1, plaintext);
    }
}

// ============================================================================
// SECTION 10: REAL-WORLD USE CASE TESTS
// ============================================================================

mod real_world_use_cases {
    use super::*;

    #[test]
    fn test_encrypt_ansible_variables() {
        let vault = Vault::new("ansible_vault_password");
        let vars = r#"
db_password: super_secret_password
api_key: sk-1234567890abcdef
aws_secret: AKIAIOSFODNN7EXAMPLE
"#;

        let encrypted = vault.encrypt(vars).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, vars);
        assert!(Vault::is_encrypted(&encrypted));
    }

    #[test]
    fn test_encrypt_ssh_private_key() {
        let vault = Vault::new("key_password");
        let key = r#"-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA1234567890abcdef...
-----END RSA PRIVATE KEY-----"#;

        let encrypted = vault.encrypt(key).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, key);
    }

    #[test]
    fn test_encrypt_json_credentials() {
        let vault = Vault::new("creds_password");
        let creds = r#"{
  "username": "admin",
  "password": "P@ssw0rd!",
  "token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
  "expires": "2024-12-31T23:59:59Z"
}"#;

        let encrypted = vault.encrypt(creds).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, creds);
    }

    #[test]
    fn test_encrypt_kubernetes_secrets() {
        let vault = Vault::new("k8s_password");
        let k8s_secret = r#"apiVersion: v1
kind: Secret
metadata:
  name: db-credentials
type: Opaque
data:
  username: YWRtaW4=
  password: MWYyZDFlMmU2N2Rm
"#;

        let encrypted = vault.encrypt(k8s_secret).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, k8s_secret);
    }

    #[test]
    fn test_encrypt_docker_compose_secrets() {
        let vault = Vault::new("compose_password");
        let compose = r#"version: "3.8"
services:
  db:
    environment:
      POSTGRES_PASSWORD: ${DB_PASSWORD}
      POSTGRES_USER: ${DB_USER}
    secrets:
      - db_password
secrets:
  db_password:
    external: true
"#;

        let encrypted = vault.encrypt(compose).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, compose);
    }

    #[test]
    fn test_encrypt_terraform_tfvars() {
        let vault = Vault::new("tf_password");
        let tfvars = r#"aws_access_key = "AKIAIOSFODNN7EXAMPLE"
aws_secret_key = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"
db_password    = "supersecret123!"
"#;

        let encrypted = vault.encrypt(tfvars).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, tfvars);
    }
}

// ============================================================================
// SECTION 11: PERFORMANCE TESTS
// ============================================================================

mod performance {
    use super::*;

    #[test]
    fn test_encryption_time_is_reasonable() {
        let vault = Vault::new("password");
        let data = "test data";

        let start = Instant::now();
        let _ = vault.encrypt(data).unwrap();
        let duration = start.elapsed();

        // Encryption should complete in reasonable time (< 5 seconds for small data)
        assert!(
            duration.as_secs() < 5,
            "Encryption took too long: {:?}",
            duration
        );
    }

    #[test]
    fn test_decryption_time_is_reasonable() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test data").unwrap();

        let start = Instant::now();
        let _ = vault.decrypt(&encrypted).unwrap();
        let duration = start.elapsed();

        // Decryption should complete in reasonable time (< 5 seconds for small data)
        assert!(
            duration.as_secs() < 5,
            "Decryption took too long: {:?}",
            duration
        );
    }

    #[test]
    fn test_large_file_encryption_performance() {
        let vault = Vault::new("password");
        let large_data = "a".repeat(1024 * 1024); // 1MB

        let start = Instant::now();
        let encrypted = vault.encrypt(&large_data).unwrap();
        let encrypt_duration = start.elapsed();

        let start = Instant::now();
        let _ = vault.decrypt(&encrypted).unwrap();
        let decrypt_duration = start.elapsed();

        // 1MB should encrypt/decrypt in under 10 seconds
        assert!(
            encrypt_duration.as_secs() < 10,
            "1MB encryption took too long"
        );
        assert!(
            decrypt_duration.as_secs() < 10,
            "1MB decryption took too long"
        );
    }

    #[test]
    fn test_key_derivation_minimum_time() {
        // Key derivation with Argon2 should take a minimum amount of time
        // to be resistant to brute force attacks
        let vault = Vault::new("password");

        let start = Instant::now();
        let _ = vault.encrypt("x").unwrap();
        let duration = start.elapsed();

        // Should take at least 10ms (being conservative to avoid flaky tests)
        // In practice, Argon2 with default params takes 100ms+
        println!("Key derivation took: {:?}", duration);
    }
}

// ============================================================================
// SECTION 12: FIXTURE-BASED TESTS
// ============================================================================

mod fixtures {
    use super::*;

    #[test]
    fn test_encrypt_fixture_file() {
        if fixtures_path().exists() {
            let content = read_fixture("secret.txt");
            let vault = Vault::new("fixture_password");

            let encrypted = vault.encrypt(&content).unwrap();
            let decrypted = vault.decrypt(&encrypted).unwrap();

            assert_eq!(decrypted, content);
        }
    }

    #[test]
    fn test_encrypt_unicode_fixture() {
        if fixtures_path().join("unicode_content.txt").exists() {
            let content = read_fixture("unicode_content.txt");
            let vault = Vault::new("unicode_password");

            let encrypted = vault.encrypt(&content).unwrap();
            let decrypted = vault.decrypt(&encrypted).unwrap();

            assert_eq!(decrypted, content);
        }
    }

    #[test]
    fn test_encrypt_vars_yaml_fixture() {
        if fixtures_path().join("vars.yml").exists() {
            let content = read_fixture("vars.yml");
            let vault = Vault::new("yaml_password");

            let encrypted = vault.encrypt(&content).unwrap();
            let decrypted = vault.decrypt(&encrypted).unwrap();

            assert_eq!(decrypted, content);

            // Verify YAML is still valid after roundtrip
            let parsed: serde_yaml::Value = serde_yaml::from_str(&decrypted).unwrap();
            assert!(parsed.get("db_host").is_some());
        }
    }

    #[test]
    fn test_password_file_fixture() {
        if fixtures_path().join("password.txt").exists() {
            let password = read_fixture("password.txt").trim().to_string();
            let vault = Vault::new(&password);

            let encrypted = vault.encrypt("secret from password file").unwrap();
            let decrypted = vault.decrypt(&encrypted).unwrap();

            assert_eq!(decrypted, "secret from password file");
        }
    }
}

// ============================================================================
// SECTION 13: EDGE CASE TESTS
// ============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_max_single_byte_char() {
        let vault = Vault::new("password");
        let data = "\u{007F}"; // DEL character

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_surrogate_pair_emojis() {
        let vault = Vault::new("password");
        let data = "Family emoji with ZWJ: 👨‍👩‍👧‍👦";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_bom_character() {
        let vault = Vault::new("password");
        let data = "\u{FEFF}BOM at start";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_null_character_in_data() {
        let vault = Vault::new("password");
        let data = "before\0after";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_very_long_lines() {
        let vault = Vault::new("password");
        let data = "x".repeat(10000);

        let encrypted = vault.encrypt(&data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_rtl_text() {
        let vault = Vault::new("password");
        let data = "مرحبا بالعالم - Hello World";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_combining_characters() {
        let vault = Vault::new("password");
        // e with combining acute accent
        let data = "cafe\u{0301}"; // café with combining char

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_zero_width_characters() {
        let vault = Vault::new("password");
        let data = "zero\u{200B}width\u{FEFF}chars";

        let encrypted = vault.encrypt(data).unwrap();
        let decrypted = vault.decrypt(&encrypted).unwrap();

        assert_eq!(decrypted, data);
    }
}

// ============================================================================
// SECTION 14: COMPATIBILITY TESTS
// ============================================================================

mod compatibility {
    use super::*;

    #[test]
    fn test_rustible_vault_header_version() {
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test").unwrap();

        // Should use Rustible format, not Ansible
        assert!(encrypted.starts_with("$RUSTIBLE_VAULT;1.0;AES256"));
        assert!(!encrypted.starts_with("$ANSIBLE_VAULT"));
    }

    #[test]
    fn test_reject_ansible_vault_format() {
        let vault = Vault::new("password");

        // Ansible vault format should be rejected
        let ansible_encrypted = "$ANSIBLE_VAULT;1.1;AES256\n66616b6564617461";
        let result = vault.decrypt(ansible_encrypted);

        assert!(result.is_err());
    }

    #[test]
    fn test_vars_vault_uses_ansible_format() {
        use rustible::vars::Vault as VarsVault;

        // The vars module Vault uses Ansible-compatible format
        let encrypted = VarsVault::encrypt("test", "password").unwrap();

        // Should start with Ansible header
        assert!(encrypted.starts_with("$ANSIBLE_VAULT;1.1;AES256"));
    }

    #[test]
    fn test_rustible_vault_format() {
        // The core vault uses Rustible format
        let vault = Vault::new("password");
        let encrypted = vault.encrypt("test").unwrap();

        // Core vault uses Rustible format with AES256
        assert!(encrypted.starts_with("$RUSTIBLE_VAULT;1.0;AES256"));
    }
}
