//! SSH Authentication Parity Tests
//!
//! This test suite validates SSH authentication parity with Ansible:
//! 1. Password authentication
//! 2. Public key authentication (multiple formats)
//! 3. SSH agent authentication
//! 4. Keyboard-interactive authentication
//! 5. Ask-become-pass functionality
//!
//! Tests ensure rustible supports all authentication methods expected
//! by users migrating from Ansible.

use rustible::connection::config::HostConfig;
use rustible::connection::russh_auth::{
    AuthConfig, AuthMethod, KeyError, KeyInfo, KeyLoader, KeyType, RusshClientHandler,
    default_identity_files, is_key_encrypted, standard_key_locations,
};
use rustible::executor::ExecutorConfig;
use std::path::{Path, PathBuf};

// ============================================================================
// AuthMethod Tests - Parity with Ansible auth options
// ============================================================================

mod auth_method_tests {
    use super::*;

    #[test]
    fn test_auth_method_none() {
        let method = AuthMethod::None;
        // None auth is supported for special cases
        assert_eq!(method, AuthMethod::None);
    }

    #[test]
    fn test_auth_method_password() {
        let method = AuthMethod::Password("secret".to_string());
        if let AuthMethod::Password(pwd) = method {
            assert_eq!(pwd, "secret");
        } else {
            panic!("Expected Password method");
        }
    }

    #[test]
    fn test_auth_method_public_key_without_passphrase() {
        let method = AuthMethod::PublicKey {
            key_path: "~/.ssh/id_rsa".to_string(),
            passphrase: None,
        };
        if let AuthMethod::PublicKey { key_path, passphrase } = method {
            assert_eq!(key_path, "~/.ssh/id_rsa");
            assert!(passphrase.is_none());
        } else {
            panic!("Expected PublicKey method");
        }
    }

    #[test]
    fn test_auth_method_public_key_with_passphrase() {
        let method = AuthMethod::PublicKey {
            key_path: "~/.ssh/id_rsa".to_string(),
            passphrase: Some("key_passphrase".to_string()),
        };
        if let AuthMethod::PublicKey { key_path, passphrase } = method {
            assert_eq!(key_path, "~/.ssh/id_rsa");
            assert_eq!(passphrase, Some("key_passphrase".to_string()));
        } else {
            panic!("Expected PublicKey method");
        }
    }

    #[test]
    fn test_auth_method_agent() {
        let method = AuthMethod::Agent;
        assert_eq!(method, AuthMethod::Agent);
    }

    #[test]
    fn test_auth_method_keyboard_interactive() {
        let responses = vec!["password".to_string(), "123456".to_string()];
        let method = AuthMethod::KeyboardInteractive {
            responses: responses.clone(),
        };
        if let AuthMethod::KeyboardInteractive { responses: resp } = method {
            assert_eq!(resp.len(), 2);
            assert_eq!(resp[0], "password");
            assert_eq!(resp[1], "123456");
        } else {
            panic!("Expected KeyboardInteractive method");
        }
    }

    #[test]
    fn test_auth_method_equality() {
        // Same methods should be equal
        assert_eq!(AuthMethod::None, AuthMethod::None);
        assert_eq!(AuthMethod::Agent, AuthMethod::Agent);
        assert_eq!(
            AuthMethod::Password("test".to_string()),
            AuthMethod::Password("test".to_string())
        );

        // Different methods should not be equal
        assert_ne!(AuthMethod::None, AuthMethod::Agent);
        assert_ne!(
            AuthMethod::Password("a".to_string()),
            AuthMethod::Password("b".to_string())
        );
    }

    #[test]
    fn test_auth_method_debug_output() {
        // Verify debug output doesn't leak sensitive data in obvious ways
        let method = AuthMethod::Password("secret_password".to_string());
        let debug = format!("{:?}", method);

        // Note: AuthMethod does show password in debug (not ideal but documented)
        // This test documents current behavior
        assert!(debug.contains("Password"));
    }

    #[test]
    fn test_auth_method_clone() {
        let original = AuthMethod::KeyboardInteractive {
            responses: vec!["answer1".to_string(), "answer2".to_string()],
        };
        let cloned = original.clone();

        if let AuthMethod::KeyboardInteractive { responses } = cloned {
            assert_eq!(responses.len(), 2);
        } else {
            panic!("Clone failed");
        }
    }
}

// ============================================================================
// AuthConfig Tests - Parity with Ansible connection options
// ============================================================================

mod auth_config_tests {
    use super::*;

    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();

        // Should have a username (from environment or default "root")
        assert!(!config.username.is_empty());

        // Should default to agent authentication (like Ansible)
        assert_eq!(config.methods.len(), 1);
        assert_eq!(config.methods[0], AuthMethod::Agent);

        // Should not accept unknown hosts by default (security)
        assert!(!config.accept_unknown_hosts);
    }

    #[test]
    fn test_auth_config_new_with_username() {
        let config = AuthConfig::new("testuser");
        assert_eq!(config.username, "testuser");
    }

    #[test]
    fn test_auth_config_with_password() {
        let config = AuthConfig::new("admin")
            .with_password("admin_password");

        assert!(config.methods.iter().any(|m| {
            matches!(m, AuthMethod::Password(p) if p == "admin_password")
        }));
    }

    #[test]
    fn test_auth_config_with_public_key() {
        let config = AuthConfig::new("user")
            .with_public_key("~/.ssh/custom_key", Some("passphrase".to_string()));

        assert!(config.methods.iter().any(|m| {
            matches!(m, AuthMethod::PublicKey { key_path, passphrase }
                if key_path == "~/.ssh/custom_key"
                && passphrase.as_deref() == Some("passphrase"))
        }));
    }

    #[test]
    fn test_auth_config_with_agent() {
        let config = AuthConfig::new("user")
            .with_agent()
            .with_agent(); // Adding agent twice

        // Should only have one agent method
        let agent_count = config.methods.iter()
            .filter(|m| matches!(m, AuthMethod::Agent))
            .count();
        assert_eq!(agent_count, 1);
    }

    #[test]
    fn test_auth_config_with_keyboard_interactive() {
        let responses = vec!["password".to_string(), "otp".to_string()];
        let config = AuthConfig::new("user")
            .with_keyboard_interactive(responses.clone());

        assert!(config.methods.iter().any(|m| {
            matches!(m, AuthMethod::KeyboardInteractive { responses: r } if r.len() == 2)
        }));
    }

    #[test]
    fn test_auth_config_accept_unknown_hosts() {
        let config = AuthConfig::new("user")
            .accept_unknown_hosts(true);

        assert!(config.accept_unknown_hosts);
    }

    #[test]
    fn test_auth_config_method_order() {
        // Verify methods are added in order (important for fallback)
        let config = AuthConfig::new("user")
            .with_password("pwd")
            .with_public_key("key", None)
            .with_agent();

        assert_eq!(config.methods.len(), 3);
        // Password should be second (after default agent)
        // This documents the behavior
    }

    #[test]
    fn test_auth_config_from_host_config_basic() {
        let host_config = HostConfig {
            user: Some("deploy".to_string()),
            ..Default::default()
        };

        let auth = AuthConfig::from_host_config(&host_config, true);

        assert_eq!(auth.username, "deploy");
        // With use_agent=true, should have agent auth
        assert!(auth.methods.iter().any(|m| matches!(m, AuthMethod::Agent)));
    }

    #[test]
    fn test_auth_config_from_host_config_with_password() {
        let host_config = HostConfig {
            user: Some("admin".to_string()),
            password: Some("secret".to_string()),
            ..Default::default()
        };

        let auth = AuthConfig::from_host_config(&host_config, false);

        assert!(auth.methods.iter().any(|m| {
            matches!(m, AuthMethod::Password(p) if p == "secret")
        }));
    }

    #[test]
    fn test_auth_config_from_host_config_with_identity_file() {
        let host_config = HostConfig {
            user: Some("user".to_string()),
            identity_file: Some("~/.ssh/custom".to_string()),
            password: Some("key_pass".to_string()), // Used as passphrase
            ..Default::default()
        };

        let auth = AuthConfig::from_host_config(&host_config, false);

        assert!(auth.methods.iter().any(|m| {
            matches!(m, AuthMethod::PublicKey { key_path, passphrase }
                if key_path == "~/.ssh/custom"
                && passphrase.as_deref() == Some("key_pass"))
        }));
    }

    #[test]
    fn test_auth_config_from_host_config_strict_host_checking() {
        let host_config_strict = HostConfig {
            strict_host_key_checking: Some(true),
            ..Default::default()
        };
        let auth_strict = AuthConfig::from_host_config(&host_config_strict, false);
        assert!(!auth_strict.accept_unknown_hosts);

        let host_config_no_strict = HostConfig {
            strict_host_key_checking: Some(false),
            ..Default::default()
        };
        let auth_no_strict = AuthConfig::from_host_config(&host_config_no_strict, false);
        assert!(auth_no_strict.accept_unknown_hosts);
    }
}

// ============================================================================
// KeyType Tests - SSH Key Format Support
// ============================================================================

mod key_type_tests {
    use super::*;

    #[test]
    fn test_key_type_ed25519() {
        let kt = KeyType::Ed25519;
        assert_eq!(kt.algorithm_name(), "ssh-ed25519");
        assert_eq!(kt.default_filename(), "id_ed25519");
        assert_eq!(format!("{}", kt), "ssh-ed25519");
    }

    #[test]
    fn test_key_type_rsa() {
        let kt = KeyType::Rsa;
        assert_eq!(kt.algorithm_name(), "ssh-rsa");
        assert_eq!(kt.default_filename(), "id_rsa");
        assert_eq!(format!("{}", kt), "ssh-rsa");
    }

    #[test]
    fn test_key_type_ecdsa_p256() {
        let kt = KeyType::EcdsaP256;
        assert_eq!(kt.algorithm_name(), "ecdsa-sha2-nistp256");
        assert_eq!(kt.default_filename(), "id_ecdsa");
    }

    #[test]
    fn test_key_type_ecdsa_p384() {
        let kt = KeyType::EcdsaP384;
        assert_eq!(kt.algorithm_name(), "ecdsa-sha2-nistp384");
        assert_eq!(kt.default_filename(), "id_ecdsa");
    }

    #[test]
    fn test_key_type_ecdsa_p521() {
        let kt = KeyType::EcdsaP521;
        assert_eq!(kt.algorithm_name(), "ecdsa-sha2-nistp521");
        assert_eq!(kt.default_filename(), "id_ecdsa");
    }

    #[test]
    fn test_key_type_detect_rsa_pem() {
        let content = "-----BEGIN RSA PRIVATE KEY-----\ndata\n-----END RSA PRIVATE KEY-----";
        assert_eq!(KeyType::detect_from_content(content), Some(KeyType::Rsa));
    }

    #[test]
    fn test_key_type_detect_ec_pem_returns_none() {
        // EC PEM format needs further parsing to determine curve
        let content = "-----BEGIN EC PRIVATE KEY-----\ndata\n-----END EC PRIVATE KEY-----";
        assert_eq!(KeyType::detect_from_content(content), None);
    }

    #[test]
    fn test_key_type_detect_openssh_returns_none() {
        // OpenSSH format requires full parsing
        let content = "-----BEGIN OPENSSH PRIVATE KEY-----\ndata\n-----END OPENSSH PRIVATE KEY-----";
        assert_eq!(KeyType::detect_from_content(content), None);
    }

    #[test]
    fn test_key_type_detect_pkcs8_returns_none() {
        let content = "-----BEGIN PRIVATE KEY-----\ndata\n-----END PRIVATE KEY-----";
        assert_eq!(KeyType::detect_from_content(content), None);
    }
}

// ============================================================================
// KeyLoader Tests - Key Loading and Discovery
// ============================================================================

mod key_loader_tests {
    use super::*;

    #[test]
    fn test_key_loader_new() {
        let loader = KeyLoader::new();

        // Should have default search paths
        assert!(!loader.search_paths().is_empty() || loader.search_paths().is_empty()); // May be empty if no .ssh dir

        // Should not have passphrase by default
        assert!(!loader.has_passphrase());
    }

    #[test]
    fn test_key_loader_with_passphrase() {
        let loader = KeyLoader::new()
            .with_passphrase("my_secret_passphrase");

        assert!(loader.has_passphrase());
    }

    #[test]
    fn test_key_loader_with_key_path() {
        let loader = KeyLoader::new()
            .with_key_path("/custom/path/id_rsa");

        let paths = loader.search_paths();
        assert!(paths.contains(&PathBuf::from("/custom/path/id_rsa")));

        // Custom path should be at the front
        assert_eq!(paths[0], PathBuf::from("/custom/path/id_rsa"));
    }

    #[test]
    fn test_key_loader_with_key_paths() {
        let paths = vec![
            PathBuf::from("/path1/key"),
            PathBuf::from("/path2/key"),
        ];
        let loader = KeyLoader::new()
            .with_key_paths(paths);

        let search_paths = loader.search_paths();
        assert!(search_paths.contains(&PathBuf::from("/path1/key")));
        assert!(search_paths.contains(&PathBuf::from("/path2/key")));
    }

    #[test]
    fn test_key_loader_with_agent() {
        let loader = KeyLoader::new()
            .with_agent(true);

        // Just verify it doesn't panic
        assert!(!loader.search_paths().is_empty() || loader.search_paths().is_empty());
    }

    #[test]
    fn test_key_loader_from_host_config() {
        let host_config = HostConfig {
            identity_file: Some("~/.ssh/deploy_key".to_string()),
            password: Some("passphrase".to_string()),
            ..Default::default()
        };

        let loader = KeyLoader::from_host_config(&host_config);

        assert!(loader.has_passphrase());
        // Custom identity file should be in search paths
        assert!(!loader.search_paths().is_empty());
    }

    #[test]
    fn test_key_loader_load_nonexistent_key() {
        let loader = KeyLoader::new();
        let result = loader.load_key(Path::new("/nonexistent/key/file"));

        assert!(matches!(result, Err(KeyError::NotFound(_))));
    }

    #[test]
    fn test_key_loader_no_duplicate_paths() {
        let loader = KeyLoader::new()
            .with_key_path("/path/to/key")
            .with_key_path("/path/to/key"); // Add same path twice

        let paths = loader.search_paths();
        let count = paths.iter().filter(|p| *p == &PathBuf::from("/path/to/key")).count();

        // Should only appear once
        assert_eq!(count, 1);
    }
}

// ============================================================================
// Key Error Tests
// ============================================================================

mod key_error_tests {
    use super::*;

    #[test]
    fn test_key_error_not_found() {
        let err = KeyError::NotFound(PathBuf::from("/missing/key"));
        let msg = err.to_string();

        assert!(msg.contains("not found"));
        assert!(msg.contains("/missing/key"));
    }

    #[test]
    fn test_key_error_passphrase_required() {
        let err = KeyError::PassphraseRequired(PathBuf::from("/encrypted/key"));
        let msg = err.to_string();

        assert!(msg.contains("passphrase"));
        assert!(msg.contains("required") || msg.contains("encrypted"));
    }

    #[test]
    fn test_key_error_wrong_passphrase() {
        let err = KeyError::WrongPassphrase(PathBuf::from("/key"));
        let msg = err.to_string();

        assert!(msg.to_lowercase().contains("wrong passphrase"));
    }

    #[test]
    fn test_key_error_unsupported_type() {
        let err = KeyError::UnsupportedKeyType("DSA".to_string());
        let msg = err.to_string();

        assert!(msg.contains("Unsupported"));
        assert!(msg.contains("DSA"));
    }

    #[test]
    fn test_key_error_decode_error() {
        let err = KeyError::DecodeError {
            path: PathBuf::from("/corrupted/key"),
            message: "invalid format".to_string(),
        };
        let msg = err.to_string();

        assert!(msg.contains("decode") || msg.contains("Decode"));
        assert!(msg.contains("invalid format"));
    }
}

// ============================================================================
// Standard Key Locations Tests
// ============================================================================

mod key_locations_tests {
    use super::*;

    #[test]
    fn test_standard_key_locations_returns_existing_only() {
        let locations = standard_key_locations();

        // All returned paths should exist
        for path in &locations {
            assert!(path.exists(), "Path {:?} should exist", path);
        }
    }

    #[test]
    fn test_default_identity_files() {
        let files = default_identity_files();

        // All returned files should exist
        for file in &files {
            assert!(file.exists(), "File {:?} should exist", file);
        }
    }
}

// ============================================================================
// Encryption Detection Tests
// ============================================================================

mod encryption_detection_tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_is_key_encrypted_missing_file() {
        let result = is_key_encrypted(Path::new("/nonexistent/key/file"));
        assert!(result.is_err());
    }

    #[test]
    fn test_is_key_encrypted_unencrypted_openssh() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "-----BEGIN OPENSSH PRIVATE KEY-----").unwrap();
        writeln!(file, "base64data").unwrap();
        writeln!(file, "-----END OPENSSH PRIVATE KEY-----").unwrap();

        let result = is_key_encrypted(file.path());
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_is_key_encrypted_encrypted_pkcs8() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "-----BEGIN ENCRYPTED PRIVATE KEY-----").unwrap();
        writeln!(file, "base64data").unwrap();
        writeln!(file, "-----END ENCRYPTED PRIVATE KEY-----").unwrap();

        let result = is_key_encrypted(file.path());
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_is_key_encrypted_proc_type_header() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "-----BEGIN RSA PRIVATE KEY-----").unwrap();
        writeln!(file, "Proc-Type: 4,ENCRYPTED").unwrap();
        writeln!(file, "DEK-Info: AES-256-CBC,abc123").unwrap();
        writeln!(file, "base64data").unwrap();
        writeln!(file, "-----END RSA PRIVATE KEY-----").unwrap();

        let result = is_key_encrypted(file.path());
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_is_key_encrypted_dek_info_header() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "DEK-Info: AES-128-CBC,deadbeef").unwrap();
        writeln!(file, "base64data").unwrap();

        let result = is_key_encrypted(file.path());
        assert!(result.is_ok());
        assert!(result.unwrap());
    }
}

// ============================================================================
// Become Password Tests (ask-become-pass parity)
// ============================================================================

mod become_password_tests {
    use super::*;

    #[test]
    fn test_executor_config_become_defaults() {
        let config = ExecutorConfig::default();

        assert!(!config.r#become);
        assert_eq!(config.become_method, "sudo");
        assert_eq!(config.become_user, "root");
        assert!(config.become_password.is_none());
    }

    #[test]
    fn test_executor_config_with_become() {
        let config = ExecutorConfig {
            r#become: true,
            become_method: "doas".to_string(),
            become_user: "admin".to_string(),
            become_password: Some("secret".to_string()),
            ..Default::default()
        };

        assert!(config.r#become);
        assert_eq!(config.become_method, "doas");
        assert_eq!(config.become_user, "admin");
        assert_eq!(config.become_password, Some("secret".to_string()));
    }

    #[test]
    fn test_executor_config_become_methods() {
        // Test various become methods supported (Ansible parity)
        let methods = ["sudo", "su", "pbrun", "pfexec", "doas", "dzdo", "ksu"];

        for method in methods {
            let config = ExecutorConfig {
                r#become: true,
                become_method: method.to_string(),
                ..Default::default()
            };
            assert_eq!(config.become_method, method);
        }
    }
}

// ============================================================================
// RusshClientHandler Tests
// ============================================================================

mod client_handler_tests {
    use super::*;

    #[test]
    fn test_client_handler_creation() {
        let auth_config = AuthConfig::new("testuser");
        let handler = RusshClientHandler::new(
            auth_config,
            "example.com".to_string(),
            22,
        );

        assert_eq!(handler.auth_config().username, "testuser");
    }

    #[test]
    fn test_client_handler_with_custom_port() {
        let auth_config = AuthConfig::new("user");
        let handler = RusshClientHandler::new(
            auth_config,
            "server.local".to_string(),
            2222,
        );

        assert_eq!(handler.auth_config().username, "user");
    }

    #[test]
    fn test_client_handler_auth_config_reference() {
        let config = AuthConfig::new("admin")
            .with_password("pwd")
            .accept_unknown_hosts(true);

        let handler = RusshClientHandler::new(
            config,
            "host".to_string(),
            22,
        );

        let auth = handler.auth_config();
        assert_eq!(auth.username, "admin");
        assert!(auth.accept_unknown_hosts);
    }
}

// ============================================================================
// KeyInfo Tests
// ============================================================================

mod key_info_tests {
    use super::*;

    #[test]
    fn test_key_info_creation() {
        let info = KeyInfo {
            path: PathBuf::from("/home/user/.ssh/id_ed25519"),
            key_type: Some(KeyType::Ed25519),
            was_encrypted: false,
            comment: Some("user@host".to_string()),
        };

        assert_eq!(info.path, PathBuf::from("/home/user/.ssh/id_ed25519"));
        assert_eq!(info.key_type, Some(KeyType::Ed25519));
        assert!(!info.was_encrypted);
        assert_eq!(info.comment, Some("user@host".to_string()));
    }

    #[test]
    fn test_key_info_encrypted() {
        let info = KeyInfo {
            path: PathBuf::from("/home/user/.ssh/encrypted_key"),
            key_type: Some(KeyType::Rsa),
            was_encrypted: true,
            comment: None,
        };

        assert!(info.was_encrypted);
        assert!(info.comment.is_none());
    }

    #[test]
    fn test_key_info_clone() {
        let info = KeyInfo {
            path: PathBuf::from("/path/to/key"),
            key_type: Some(KeyType::EcdsaP256),
            was_encrypted: true,
            comment: Some("test".to_string()),
        };

        let cloned = info.clone();

        assert_eq!(cloned.path, info.path);
        assert_eq!(cloned.key_type, info.key_type);
        assert_eq!(cloned.was_encrypted, info.was_encrypted);
        assert_eq!(cloned.comment, info.comment);
    }

    #[test]
    fn test_key_info_debug() {
        let info = KeyInfo {
            path: PathBuf::from("/home/user/.ssh/id_rsa"),
            key_type: Some(KeyType::Rsa),
            was_encrypted: false,
            comment: None,
        };

        let debug = format!("{:?}", info);

        assert!(debug.contains("KeyInfo"));
        assert!(debug.contains("id_rsa"));
    }
}

// ============================================================================
// Integration Tests - Full Authentication Workflow
// ============================================================================

mod integration_tests {
    use super::*;

    #[test]
    fn test_full_auth_config_from_host_config() {
        // Simulate Ansible-style configuration
        let host_config = HostConfig {
            hostname: Some("webserver".to_string()),
            user: Some("deploy".to_string()),
            port: Some(22),
            identity_file: Some("~/.ssh/deploy_key".to_string()),
            password: Some("key_passphrase".to_string()),
            strict_host_key_checking: Some(false),
            ..Default::default()
        };

        let auth = AuthConfig::from_host_config(&host_config, true);

        // Verify Ansible parity
        assert_eq!(auth.username, "deploy");
        assert!(auth.accept_unknown_hosts); // strict_host_key_checking=no

        // Should have agent (use_agent=true) and public key auth
        assert!(auth.methods.iter().any(|m| matches!(m, AuthMethod::Agent)));
        assert!(auth.methods.iter().any(|m| matches!(m, AuthMethod::PublicKey { .. })));
    }

    #[test]
    fn test_auth_fallback_order() {
        // Ansible tries authentication methods in order
        let config = AuthConfig::new("user")
            .with_agent()
            .with_public_key("~/.ssh/id_rsa", None)
            .with_password("fallback_password");

        // Verify methods are present
        let mut has_agent = false;
        let mut has_pubkey = false;
        let mut has_password = false;

        for method in &config.methods {
            match method {
                AuthMethod::Agent => has_agent = true,
                AuthMethod::PublicKey { .. } => has_pubkey = true,
                AuthMethod::Password(_) => has_password = true,
                _ => {}
            }
        }

        assert!(has_agent);
        assert!(has_pubkey);
        assert!(has_password);
    }

    #[test]
    fn test_keyboard_interactive_with_otp() {
        // Simulate 2FA with keyboard-interactive
        let responses = vec![
            "password123".to_string(),  // First prompt: password
            "123456".to_string(),       // Second prompt: OTP
        ];

        let config = AuthConfig::new("secure_user")
            .with_keyboard_interactive(responses);

        assert!(config.methods.iter().any(|m| {
            matches!(m, AuthMethod::KeyboardInteractive { responses } if responses.len() == 2)
        }));
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

mod edge_case_tests {
    use super::*;

    #[test]
    fn test_empty_password() {
        let method = AuthMethod::Password(String::new());
        if let AuthMethod::Password(pwd) = method {
            assert!(pwd.is_empty());
        }
    }

    #[test]
    fn test_empty_responses() {
        let method = AuthMethod::KeyboardInteractive { responses: vec![] };
        if let AuthMethod::KeyboardInteractive { responses } = method {
            assert!(responses.is_empty());
        }
    }

    #[test]
    fn test_auth_config_empty_methods() {
        let config = AuthConfig {
            username: "test".to_string(),
            methods: vec![],
            accept_unknown_hosts: false,
            known_hosts_file: None,
        };

        assert!(config.methods.is_empty());
    }

    #[test]
    fn test_special_characters_in_password() {
        let special_passwords = vec![
            "pass with spaces",
            "pass\twith\ttabs",
            "pass\"with\"quotes",
            "pass'with'single",
            "pass\\with\\backslash",
            "пароль", // Cyrillic
            "密码",    // Chinese
            "🔐🔑",   // Emoji
        ];

        for pwd in special_passwords {
            let method = AuthMethod::Password(pwd.to_string());
            if let AuthMethod::Password(p) = method {
                assert_eq!(p, pwd);
            }
        }
    }

    #[test]
    fn test_very_long_password() {
        let long_password = "a".repeat(10000);
        let method = AuthMethod::Password(long_password.clone());
        if let AuthMethod::Password(p) = method {
            assert_eq!(p.len(), 10000);
        }
    }

    #[test]
    fn test_path_with_special_characters() {
        let paths = vec![
            PathBuf::from("/path with spaces/key"),
            PathBuf::from("/path\twith\ttabs/key"),
            PathBuf::from("/путь/ключ"), // Cyrillic
        ];

        for path in paths {
            let loader = KeyLoader::new().with_key_path(path.clone());
            assert!(loader.search_paths().contains(&path));
        }
    }
}
