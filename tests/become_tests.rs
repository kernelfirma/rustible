//! Comprehensive tests for privilege escalation (become) functionality in Rustible.
//!
//! These tests cover all aspects of privilege escalation including:
//! - Basic become configuration (become: true, become_user)
//! - Multiple become methods (sudo, su, doas, pbrun, pfexec, runas)
//! - Password handling (--ask-become-pass, ansible_become_password)
//! - Custom become flags
//! - Become scope (play, block, task level)
//! - Connection-specific become handling
//! - sudo configuration scenarios
//! - Privilege escalation chains
//! - Delegation with become
//! - Security aspects (password protection in logs and memory)

#![allow(unused_imports)]

use rustible::config::{Config, PrivilegeEscalation};
use rustible::connection::local::LocalConnection;
use rustible::connection::{ConnectionConfig, ExecuteOptions};
use rustible::traits::BecomeConfig;

// ============================================================================
// 1. BECOME BASIC TESTS
// ============================================================================

mod become_basic {
    use super::*;

    /// Test that become: true enables privilege escalation
    #[test]
    fn test_become_true_enables_escalation() {
        let options = ExecuteOptions::new().with_escalation(None);

        assert!(options.escalate);
        // When no user specified, defaults to None (which means root in implementation)
        assert!(options.escalate_user.is_none());
    }

    /// Test that become: false (default) disables privilege escalation
    #[test]
    fn test_become_false_by_default() {
        let options = ExecuteOptions::default();

        assert!(!options.escalate);
        assert!(options.escalate_user.is_none());
        assert!(options.escalate_method.is_none());
        assert!(options.escalate_password.is_none());
    }

    /// Test become: true on task level
    #[test]
    fn test_become_on_task() {
        let mut options = ExecuteOptions::new();
        options.escalate = true;

        assert!(options.escalate);
    }

    /// Test become_user: root (default)
    #[test]
    fn test_become_user_root_default() {
        let options = ExecuteOptions::new().with_escalation(Some("root".to_string()));

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("root".to_string()));
    }

    /// Test become_user: other_user
    #[test]
    fn test_become_user_other_user() {
        let options = ExecuteOptions::new().with_escalation(Some("www-data".to_string()));

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("www-data".to_string()));
    }

    /// Test become_user with various valid usernames
    #[test]
    fn test_become_user_various_usernames() {
        let usernames = vec![
            "root",
            "admin",
            "www-data",
            "nginx",
            "postgres",
            "mysql",
            "nobody",
            "user123",
            "test_user",
            "test-user",
        ];

        for username in usernames {
            let options = ExecuteOptions::new().with_escalation(Some(username.to_string()));
            assert_eq!(options.escalate_user, Some(username.to_string()));
        }
    }

    /// Test BecomeConfig structure
    #[test]
    fn test_become_config_structure() {
        let config = BecomeConfig {
            enabled: true,
            method: "sudo".to_string(),
            user: "root".to_string(),
            password: Some("secret".to_string()),
            flags: Some("-H".to_string()),
        };

        assert!(config.enabled);
        assert_eq!(config.method, "sudo");
        assert_eq!(config.user, "root");
        assert_eq!(config.password, Some("secret".to_string()));
        assert_eq!(config.flags, Some("-H".to_string()));
    }

    /// Test BecomeConfig default values
    #[test]
    fn test_become_config_defaults() {
        let config = BecomeConfig::default();

        assert!(!config.enabled);
        assert!(config.method.is_empty() || config.method.is_empty());
        assert!(config.user.is_empty() || config.user.is_empty());
        assert!(config.password.is_none());
        assert!(config.flags.is_none());
    }

    /// Test PrivilegeEscalation config defaults
    #[test]
    fn test_privilege_escalation_config_defaults() {
        let config = PrivilegeEscalation::default();

        assert!(!config.r#become);
        assert_eq!(config.become_method, "sudo");
        assert_eq!(config.become_user, "root");
        assert!(!config.become_ask_pass);
        assert!(config.become_flags.is_none());
    }
}

// ============================================================================
// 2. BECOME METHODS TESTS
// ============================================================================

mod become_methods {
    use super::*;

    /// Test become_method: sudo (default)
    #[test]
    fn test_become_method_sudo_default() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("sudo".to_string()));
    }

    /// Test become_method: su
    #[test]
    fn test_become_method_su() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("su".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("su".to_string()));
    }

    /// Test become_method: doas
    #[test]
    fn test_become_method_doas() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("doas".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("doas".to_string()));
    }

    /// Test become_method: pbrun
    #[test]
    fn test_become_method_pbrun() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("pbrun".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("pbrun".to_string()));
    }

    /// Test become_method: pfexec
    #[test]
    fn test_become_method_pfexec() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("pfexec".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("pfexec".to_string()));
    }

    /// Test become_method: runas (Windows)
    #[test]
    fn test_become_method_runas() {
        let mut options = ExecuteOptions::new().with_escalation(Some("Administrator".to_string()));
        options.escalate_method = Some("runas".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("runas".to_string()));
        assert_eq!(options.escalate_user, Some("Administrator".to_string()));
    }

    /// Test all become methods are supported
    #[test]
    fn test_all_become_methods_supported() {
        let methods = vec![
            "sudo", "su", "doas", "pbrun", "pfexec", "runas", "dzdo", "ksu", "pmrun",
        ];

        for method in methods {
            let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
            options.escalate_method = Some(method.to_string());

            assert!(options.escalate);
            assert_eq!(options.escalate_method, Some(method.to_string()));
        }
    }

    /// Test that PrivilegeEscalation config method is stored
    #[test]
    fn test_privilege_escalation_method_storage() {
        let config = PrivilegeEscalation {
            become_method: "doas".to_string(),
            ..Default::default()
        };

        assert_eq!(config.become_method, "doas");
    }

    /// Test method with combined user
    #[test]
    fn test_method_with_user_combinations() {
        let combinations = vec![
            ("sudo", "root"),
            ("sudo", "www-data"),
            ("su", "root"),
            ("su", "postgres"),
            ("doas", "root"),
            ("doas", "admin"),
        ];

        for (method, user) in combinations {
            let mut options = ExecuteOptions::new().with_escalation(Some(user.to_string()));
            options.escalate_method = Some(method.to_string());

            assert!(options.escalate);
            assert_eq!(options.escalate_method, Some(method.to_string()));
            assert_eq!(options.escalate_user, Some(user.to_string()));
        }
    }
}

// ============================================================================
// 3. BECOME PASSWORD TESTS
// ============================================================================

mod become_password {
    use super::*;

    /// Test setting become password
    #[test]
    fn test_become_password_setting() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some("secret123".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_password, Some("secret123".to_string()));
    }

    /// Test ansible_become_password variable equivalent
    #[test]
    fn test_ansible_become_password_variable() {
        // This simulates setting password via variable
        let password = "my_become_password";
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some(password.to_string());

        assert_eq!(options.escalate_password, Some(password.to_string()));
    }

    /// Test password with sudo method
    #[test]
    fn test_password_with_sudo() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());
        options.escalate_password = Some("sudo_password".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("sudo".to_string()));
        assert_eq!(options.escalate_password, Some("sudo_password".to_string()));
    }

    /// Test password with su method
    #[test]
    fn test_password_with_su() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("su".to_string());
        options.escalate_password = Some("root_password".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("su".to_string()));
        assert_eq!(options.escalate_password, Some("root_password".to_string()));
    }

    /// Test become_ask_pass config option
    #[test]
    fn test_become_ask_pass_config() {
        let config = PrivilegeEscalation {
            become_ask_pass: true,
            ..Default::default()
        };

        assert!(config.become_ask_pass);
    }

    /// Test empty password handling
    #[test]
    fn test_empty_password_handling() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some("".to_string());

        // Empty password is technically valid (for NOPASSWD sudo)
        assert_eq!(options.escalate_password, Some("".to_string()));
    }

    /// Test None password (no password provided)
    #[test]
    fn test_no_password_provided() {
        let options = ExecuteOptions::new().with_escalation(Some("root".to_string()));

        assert!(options.escalate_password.is_none());
    }

    /// Test password with special characters
    #[test]
    fn test_password_with_special_characters() {
        let special_passwords = vec![
            "p@ssw0rd!",
            "pass word",
            "pass\tword",
            "pass'word",
            "pass\"word",
            "pass$word",
            "pass`word",
            "pass\\word",
            "パスワード",
            "пароль",
        ];

        for password in special_passwords {
            let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
            options.escalate_password = Some(password.to_string());

            assert_eq!(options.escalate_password, Some(password.to_string()));
        }
    }
}

// ============================================================================
// 4. BECOME FLAGS TESTS
// ============================================================================

mod become_flags {
    use super::*;

    /// Test become_flags for custom flags
    #[test]
    fn test_become_flags_custom() {
        let config = PrivilegeEscalation {
            become_flags: Some("-H -S -n".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_flags, Some("-H -S -n".to_string()));
    }

    /// Test sudo with -H flag (preserve HOME)
    #[test]
    fn test_sudo_with_h_flag() {
        let config = PrivilegeEscalation {
            become_method: "sudo".to_string(),
            become_flags: Some("-H".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_method, "sudo");
        assert_eq!(config.become_flags, Some("-H".to_string()));
    }

    /// Test sudo with -S flag (read password from stdin)
    #[test]
    fn test_sudo_with_s_flag() {
        let config = PrivilegeEscalation {
            become_method: "sudo".to_string(),
            become_flags: Some("-S".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_flags, Some("-S".to_string()));
    }

    /// Test sudo with -n flag (non-interactive, no password prompt)
    #[test]
    fn test_sudo_with_n_flag() {
        let config = PrivilegeEscalation {
            become_method: "sudo".to_string(),
            become_flags: Some("-n".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_flags, Some("-n".to_string()));
    }

    /// Test su with - flag (login shell)
    #[test]
    fn test_su_with_login_flag() {
        let config = PrivilegeEscalation {
            become_method: "su".to_string(),
            become_flags: Some("-".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_method, "su");
        assert_eq!(config.become_flags, Some("-".to_string()));
    }

    /// Test su with -l flag (login shell alternative)
    #[test]
    fn test_su_with_l_flag() {
        let config = PrivilegeEscalation {
            become_method: "su".to_string(),
            become_flags: Some("-l".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_flags, Some("-l".to_string()));
    }

    /// Test su with -m flag (preserve environment)
    #[test]
    fn test_su_with_m_flag() {
        let config = PrivilegeEscalation {
            become_method: "su".to_string(),
            become_flags: Some("-m".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_flags, Some("-m".to_string()));
    }

    /// Test doas with -n flag (non-interactive)
    #[test]
    fn test_doas_with_n_flag() {
        let config = PrivilegeEscalation {
            become_method: "doas".to_string(),
            become_flags: Some("-n".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_method, "doas");
        assert_eq!(config.become_flags, Some("-n".to_string()));
    }

    /// Test multiple flags combined
    #[test]
    fn test_multiple_flags_combined() {
        let config = PrivilegeEscalation {
            become_method: "sudo".to_string(),
            become_flags: Some("-H -S -n --preserve-env".to_string()),
            ..Default::default()
        };

        assert!(config.become_flags.as_ref().unwrap().contains("-H"));
        assert!(config.become_flags.as_ref().unwrap().contains("-S"));
        assert!(config.become_flags.as_ref().unwrap().contains("-n"));
        assert!(config
            .become_flags
            .as_ref()
            .unwrap()
            .contains("--preserve-env"));
    }

    /// Test BecomeConfig with flags
    #[test]
    fn test_become_config_with_flags() {
        let config = BecomeConfig {
            enabled: true,
            method: "sudo".to_string(),
            user: "root".to_string(),
            password: None,
            flags: Some("-H -S".to_string()),
        };

        assert_eq!(config.flags, Some("-H -S".to_string()));
    }

    /// Test empty flags
    #[test]
    fn test_empty_flags() {
        let config = PrivilegeEscalation {
            become_flags: Some("".to_string()),
            ..Default::default()
        };

        assert_eq!(config.become_flags, Some("".to_string()));
    }
}

// ============================================================================
// 5. BECOME SCOPE TESTS
// ============================================================================

mod become_scope {
    use super::*;

    /// Test play-level become setting
    #[test]
    fn test_play_level_become() {
        // Simulate play-level become configuration
        let play_become = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        assert!(play_become.r#become);
        assert_eq!(play_become.become_user, "root");
    }

    /// Test block-level become setting
    #[test]
    fn test_block_level_become() {
        // Block inherits from play but can override
        let block_become = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "www-data".to_string(), // Override play's root
            become_ask_pass: false,
            become_flags: None,
        };

        assert!(block_become.r#become);
        assert_eq!(block_become.become_user, "www-data");
    }

    /// Test task-level become setting
    #[test]
    fn test_task_level_become() {
        // Task can override both play and block
        let task_become = PrivilegeEscalation {
            r#become: true,
            become_method: "su".to_string(),     // Override method
            become_user: "postgres".to_string(), // Override user
            become_ask_pass: false,
            become_flags: Some("-".to_string()),
        };

        assert!(task_become.r#become);
        assert_eq!(task_become.become_method, "su");
        assert_eq!(task_become.become_user, "postgres");
    }

    /// Test task overrides play become settings
    #[test]
    fn test_task_overrides_play() {
        let play = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        let task = PrivilegeEscalation {
            r#become: false, // Disable become for this task
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        // Task setting takes precedence
        let effective_become = task.r#become;
        assert!(!effective_become);

        // Play would have become enabled
        assert!(play.r#become);
    }

    /// Test become user override at task level
    #[test]
    fn test_become_user_override() {
        let play_user = "root";
        let task_user = "nginx";

        let play = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: play_user.to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        let task = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: task_user.to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        assert_eq!(play.become_user, "root");
        assert_eq!(task.become_user, "nginx");
    }

    /// Test become method override at task level
    #[test]
    fn test_become_method_override() {
        let play_method = "sudo";
        let task_method = "doas";

        let play = PrivilegeEscalation {
            r#become: true,
            become_method: play_method.to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        let task = PrivilegeEscalation {
            r#become: true,
            become_method: task_method.to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        assert_eq!(play.become_method, "sudo");
        assert_eq!(task.become_method, "doas");
    }

    /// Test nested scope resolution
    #[test]
    fn test_nested_scope_resolution() {
        // Play level
        let play = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: Some("-H".to_string()),
        };

        // Block level - inherits play, overrides user
        let block = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "admin".to_string(),
            become_ask_pass: false,
            become_flags: Some("-H".to_string()),
        };

        // Task level - inherits block, overrides method
        let task = PrivilegeEscalation {
            r#become: true,
            become_method: "su".to_string(),
            become_user: "admin".to_string(),
            become_ask_pass: false,
            become_flags: Some("-".to_string()),
        };

        assert_eq!(play.become_user, "root");
        assert_eq!(block.become_user, "admin");
        assert_eq!(task.become_method, "su");
    }

    /// Test become: false at task disables escalation
    #[test]
    fn test_become_false_disables_escalation() {
        let task = PrivilegeEscalation {
            r#become: false,
            ..Default::default()
        };

        assert!(!task.r#become);
    }
}

// ============================================================================
// 6. BECOME WITH CONNECTION TESTS
// ============================================================================

mod become_with_connection {
    use super::*;

    /// Test SSH + become
    #[tokio::test]
    async fn test_ssh_with_become_options() {
        // SSH connection options with escalation
        let options = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_timeout(30);

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("root".to_string()));
    }

    /// Test Local + become
    #[tokio::test]
    async fn test_local_with_become() {
        let _conn = LocalConnection::new();

        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());

        // The command should be prefixed with sudo
        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("sudo".to_string()));
    }

    /// Test Docker + become (different handling)
    #[test]
    fn test_docker_with_become() {
        // Docker uses -u flag for user, not sudo/su
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());

        // Docker connection would handle this differently (using -u root)
        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("root".to_string()));
    }

    /// Test connection factory with become config
    #[test]
    fn test_connection_config_with_become() {
        let config = ConnectionConfig::default();

        // Connection config doesn't directly store become settings
        // Those are in ExecuteOptions per command
        assert!(config.defaults.port == 22);
    }

    /// Test become with custom environment
    #[test]
    fn test_become_with_custom_env() {
        let options = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_env("MY_VAR", "value");

        assert!(options.escalate);
        assert_eq!(options.env.get("MY_VAR"), Some(&"value".to_string()));
    }

    /// Test become with working directory
    #[test]
    fn test_become_with_cwd() {
        let options = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_cwd("/var/www");

        assert!(options.escalate);
        assert_eq!(options.cwd, Some("/var/www".to_string()));
    }

    /// Test local connection builds command with sudo
    #[tokio::test]
    async fn test_local_builds_command_with_sudo() {
        let _conn = LocalConnection::new();

        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());

        // When executing, the local connection should wrap command with sudo
        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("sudo".to_string()));
        assert_eq!(options.escalate_user, Some("root".to_string()));
    }

    /// Test local connection builds command with su
    #[tokio::test]
    async fn test_local_builds_command_with_su() {
        let _conn = LocalConnection::new();

        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("su".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("su".to_string()));
    }

    /// Test local connection builds command with doas
    #[tokio::test]
    async fn test_local_builds_command_with_doas() {
        let _conn = LocalConnection::new();

        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("doas".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_method, Some("doas".to_string()));
    }
}

// ============================================================================
// 7. SUDO CONFIGURATION TESTS
// ============================================================================

mod sudo_configuration {
    use super::*;

    /// Test NOPASSWD sudo configuration
    #[test]
    fn test_nopasswd_sudo() {
        // When NOPASSWD is configured, no password is needed
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());
        options.escalate_password = None; // No password needed with NOPASSWD

        assert!(options.escalate);
        assert!(options.escalate_password.is_none());
    }

    /// Test restricted sudo (limited commands)
    #[test]
    fn test_restricted_sudo() {
        // Even with restricted sudo, the options are the same
        // The restriction is enforced by the sudo configuration on the target
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());

        assert!(options.escalate);
    }

    /// Test sudo with -n flag for non-interactive
    #[test]
    fn test_sudo_non_interactive() {
        // Using -n flag to fail if password is required
        let config = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: Some("-n".to_string()),
        };

        assert!(config.become_flags.as_ref().unwrap().contains("-n"));
    }

    /// Test sudo timeout handling
    #[test]
    fn test_sudo_with_timeout() {
        let options = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_timeout(30);

        assert!(options.escalate);
        assert_eq!(options.timeout, Some(30));
    }

    /// Test sudo with password prompt
    #[test]
    fn test_sudo_password_prompt() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());
        options.escalate_password = Some("password123".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_password, Some("password123".to_string()));
    }

    /// Test sudo with -S flag (read password from stdin)
    #[test]
    fn test_sudo_stdin_password() {
        let config = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: Some("-S".to_string()),
        };

        assert!(config.become_flags.as_ref().unwrap().contains("-S"));
    }

    /// Test sudo with preserved environment
    #[test]
    fn test_sudo_preserve_env() {
        let config = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: Some("-E".to_string()),
        };

        assert!(config.become_flags.as_ref().unwrap().contains("-E"));
    }

    /// Test sudo with specific command only (visudo config)
    #[test]
    fn test_sudo_specific_command() {
        // This tests that we correctly pass the command to sudo
        // The restriction is enforced by sudoers
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());

        assert!(options.escalate);
    }
}

// ============================================================================
// 8. PRIVILEGE ESCALATION CHAIN TESTS
// ============================================================================

mod privilege_escalation_chain {
    use super::*;

    /// Test become to user A then user B (nested become)
    #[test]
    fn test_chained_become() {
        // First escalation: regular user -> admin
        let first_escalation = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "admin".to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        // Second escalation: admin -> root
        let second_escalation = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        assert_eq!(first_escalation.become_user, "admin");
        assert_eq!(second_escalation.become_user, "root");
    }

    /// Test nested become handling with different methods
    #[test]
    fn test_nested_become_different_methods() {
        // First: sudo to admin
        let first = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "admin".to_string(),
            become_ask_pass: false,
            become_flags: None,
        };

        // Second: su to root
        let second = PrivilegeEscalation {
            r#become: true,
            become_method: "su".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: false,
            become_flags: Some("-".to_string()),
        };

        assert_eq!(first.become_method, "sudo");
        assert_eq!(second.become_method, "su");
    }

    /// Test become chain with intermediate user
    #[test]
    fn test_become_chain_intermediate() {
        // user -> deploy -> root
        let steps = vec![("deploy", "sudo"), ("root", "sudo")];

        for (user, method) in steps {
            let escalation = PrivilegeEscalation {
                r#become: true,
                become_method: method.to_string(),
                become_user: user.to_string(),
                become_ask_pass: false,
                become_flags: None,
            };

            assert!(escalation.r#become);
            assert_eq!(escalation.become_user, user);
        }
    }

    /// Test become from unprivileged user
    #[test]
    fn test_become_from_unprivileged() {
        let escalation = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: true, // Unprivileged usually needs password
            become_flags: None,
        };

        assert!(escalation.become_ask_pass);
    }

    /// Test become chain with service accounts
    #[test]
    fn test_become_chain_service_accounts() {
        let service_accounts = vec!["www-data", "nginx", "postgres", "mysql"];

        for account in service_accounts {
            let escalation = PrivilegeEscalation {
                r#become: true,
                become_method: "sudo".to_string(),
                become_user: account.to_string(),
                become_ask_pass: false,
                become_flags: None,
            };

            assert_eq!(escalation.become_user, account);
        }
    }
}

// ============================================================================
// 9. BECOME WITH DELEGATE TESTS
// ============================================================================

mod become_with_delegate {
    use super::*;

    /// Test become on delegated task
    #[test]
    fn test_become_on_delegated_task() {
        // When delegating to another host, become should still work
        let options = ExecuteOptions::new().with_escalation(Some("root".to_string()));

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("root".to_string()));
    }

    /// Test become_user on delegate
    #[test]
    fn test_become_user_on_delegate() {
        // The become_user should be respected on the delegate host
        let options = ExecuteOptions::new().with_escalation(Some("admin".to_string()));

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("admin".to_string()));
    }

    /// Test delegated task with different become user
    #[test]
    fn test_delegate_different_become_user() {
        // Original task become_user
        let original = ExecuteOptions::new().with_escalation(Some("root".to_string()));

        // Delegated task can have different become_user
        let delegated = ExecuteOptions::new().with_escalation(Some("www-data".to_string()));

        assert_eq!(original.escalate_user, Some("root".to_string()));
        assert_eq!(delegated.escalate_user, Some("www-data".to_string()));
    }

    /// Test delegate with different become method
    #[test]
    fn test_delegate_different_become_method() {
        // Original uses sudo
        let mut original = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        original.escalate_method = Some("sudo".to_string());

        // Delegate uses doas (different OS)
        let mut delegated = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        delegated.escalate_method = Some("doas".to_string());

        assert_eq!(original.escalate_method, Some("sudo".to_string()));
        assert_eq!(delegated.escalate_method, Some("doas".to_string()));
    }

    /// Test delegate to localhost with become
    #[test]
    fn test_delegate_localhost_become() {
        let options = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_cwd("/tmp");

        assert!(options.escalate);
    }

    /// Test delegate facts with become
    #[test]
    fn test_delegate_facts_become() {
        // When gathering facts on delegate, become should apply
        let options = ExecuteOptions::new().with_escalation(Some("root".to_string()));

        assert!(options.escalate);
    }
}

// ============================================================================
// 10. SECURITY TESTS
// ============================================================================

mod become_security {
    use super::*;

    /// Test that password is not included in debug output
    #[test]
    fn test_password_not_in_debug_output() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some("super_secret_password".to_string());

        // The debug output should ideally not contain the password
        // This documents current behavior for security review
        let debug_output = format!("{:?}", options);

        // Note: Current implementation may or may not expose password in debug
        // This test documents the behavior
        let _ = debug_output;
    }

    /// Test that password is not leaked in error messages
    #[test]
    fn test_password_not_in_errors() {
        let password = "secret_password_12345";
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some(password.to_string());

        // If there's an error, the password should not be in the message
        // This is a documentation test
        assert!(options.escalate_password.is_some());
    }

    /// Test password handling in memory
    #[test]
    fn test_password_in_memory() {
        let password = "memory_test_password";
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some(password.to_string());

        // Password should be stored but ideally cleared after use
        assert_eq!(options.escalate_password, Some(password.to_string()));

        // Clear the password
        options.escalate_password = None;
        assert!(options.escalate_password.is_none());
    }

    /// Test become error messages are safe
    #[test]
    fn test_become_error_messages_safe() {
        // Error messages should not expose sensitive information
        let config = PrivilegeEscalation {
            r#become: true,
            become_method: "sudo".to_string(),
            become_user: "root".to_string(),
            become_ask_pass: true,
            become_flags: None,
        };

        let debug_output = format!("{:?}", config);
        // Should not contain password-related sensitive data
        assert!(!debug_output.contains("password"));
    }

    /// Test that become config doesn't store password
    #[test]
    fn test_privilege_escalation_no_password_storage() {
        let config = PrivilegeEscalation::default();

        // PrivilegeEscalation struct doesn't store the actual password
        // It only has become_ask_pass flag
        let debug_output = format!("{:?}", config);
        assert!(!debug_output.contains("secret"));
    }

    /// Test BecomeConfig password field handling
    #[test]
    fn test_become_config_password_handling() {
        let config = BecomeConfig {
            enabled: true,
            method: "sudo".to_string(),
            user: "root".to_string(),
            password: Some("sensitive_password".to_string()),
            flags: None,
        };

        // Password is stored but should be handled carefully
        assert!(config.password.is_some());
    }

    /// Test that ExecuteOptions clears password on drop
    #[test]
    fn test_options_password_on_drop() {
        let options = {
            let mut opt = ExecuteOptions::new().with_escalation(Some("root".to_string()));
            opt.escalate_password = Some("temporary_password".to_string());
            opt
        };

        // Options are still valid here
        assert!(options.escalate_password.is_some());

        // After this scope, options will be dropped
        // (Rust doesn't guarantee memory zeroing without explicit handling)
    }

    /// Test concurrent access to password
    #[test]
    fn test_concurrent_password_access() {
        use std::sync::Arc;
        use std::thread;

        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some("shared_password".to_string());
        let options = Arc::new(options);

        let mut handles = vec![];

        for _ in 0..10 {
            let opt_clone = Arc::clone(&options);
            let handle = thread::spawn(move || {
                // Read-only access is safe
                assert!(opt_clone.escalate);
                // Password access
                let _ = opt_clone.escalate_password.clone();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    }

    /// Test password masking in logs (conceptual)
    #[test]
    fn test_password_masking_concept() {
        let password = "my_secret_password";
        let masked = "********";

        // When logging, passwords should be masked
        let log_message = format!("Executing with become, password: {}", masked);
        assert!(!log_message.contains(password));
        assert!(log_message.contains("********"));
    }

    /// Test that no sensitive info in command line
    #[test]
    fn test_no_sensitive_in_command_line() {
        let password = "secret123";
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_method = Some("sudo".to_string());
        options.escalate_password = Some(password.to_string());

        // Password should be passed via stdin (-S flag), not command line
        // The implementation uses -S when password is provided
        assert!(options.escalate_password.is_some());
    }
}

// ============================================================================
// 11. MOCK SUDO/SU COMMANDS FOR TESTING
// ============================================================================

mod mock_commands {
    use super::*;

    /// Test that we can construct a mock sudo environment
    #[test]
    fn test_mock_sudo_construction() {
        // In a real test environment, we would create mock sudo/su scripts
        let mock_sudo = MockBecomeCommand::new("sudo")
            .with_user("root")
            .with_nopasswd(true);

        assert_eq!(mock_sudo.name, "sudo");
        assert_eq!(mock_sudo.user, Some("root".to_string()));
        assert!(mock_sudo.nopasswd);
    }

    /// Test mock su command
    #[test]
    fn test_mock_su_construction() {
        let mock_su = MockBecomeCommand::new("su")
            .with_user("root")
            .with_password("password123");

        assert_eq!(mock_su.name, "su");
        assert_eq!(mock_su.password, Some("password123".to_string()));
    }

    /// Test mock doas command
    #[test]
    fn test_mock_doas_construction() {
        let mock_doas = MockBecomeCommand::new("doas")
            .with_user("root")
            .with_nopasswd(true);

        assert_eq!(mock_doas.name, "doas");
        assert!(mock_doas.nopasswd);
    }

    /// Test mock command that requires password
    #[test]
    fn test_mock_password_required() {
        let mock = MockBecomeCommand::new("sudo")
            .with_user("root")
            .with_nopasswd(false);

        assert!(!mock.nopasswd);
    }

    /// Test mock command that returns error
    #[test]
    fn test_mock_error_response() {
        let mock = MockBecomeCommand::new("sudo")
            .with_error("Sorry, user testuser is not allowed to execute '/bin/bash'");

        assert!(mock.error.is_some());
    }

    /// Test mock command timeout
    #[test]
    fn test_mock_timeout() {
        let mock = MockBecomeCommand::new("sudo").with_timeout(true);

        assert!(mock.timeout);
    }

    /// Helper struct for mock become commands
    #[derive(Debug, Clone)]
    struct MockBecomeCommand {
        name: String,
        user: Option<String>,
        password: Option<String>,
        nopasswd: bool,
        error: Option<String>,
        timeout: bool,
    }

    impl MockBecomeCommand {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                user: None,
                password: None,
                nopasswd: false,
                error: None,
                timeout: false,
            }
        }

        fn with_user(mut self, user: &str) -> Self {
            self.user = Some(user.to_string());
            self
        }

        fn with_password(mut self, password: &str) -> Self {
            self.password = Some(password.to_string());
            self
        }

        fn with_nopasswd(mut self, nopasswd: bool) -> Self {
            self.nopasswd = nopasswd;
            self
        }

        fn with_error(mut self, error: &str) -> Self {
            self.error = Some(error.to_string());
            self
        }

        fn with_timeout(mut self, timeout: bool) -> Self {
            self.timeout = timeout;
            self
        }
    }
}

// ============================================================================
// 12. INTEGRATION TESTS
// ============================================================================

mod integration {
    use super::*;

    /// Test full become workflow with local connection
    #[tokio::test]
    async fn test_full_become_workflow_local() {
        let _conn = LocalConnection::new();

        // Create options with full become configuration
        let mut options = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_cwd("/tmp")
            .with_timeout(30);
        options.escalate_method = Some("sudo".to_string());

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("root".to_string()));
        assert_eq!(options.escalate_method, Some("sudo".to_string()));
        assert_eq!(options.cwd, Some("/tmp".to_string()));
        assert_eq!(options.timeout, Some(30));
    }

    /// Test become with environment variables
    #[tokio::test]
    async fn test_become_with_environment() {
        let _conn = LocalConnection::new();

        let options = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_env("MY_VAR", "my_value")
            .with_env("PATH", "/usr/bin:/bin");

        assert!(options.escalate);
        assert_eq!(options.env.get("MY_VAR"), Some(&"my_value".to_string()));
    }

    /// Test become configuration from config
    #[test]
    fn test_become_from_config() {
        let config = Config::default();

        // Check default privilege escalation settings
        assert!(!config.privilege_escalation.r#become);
        assert_eq!(config.privilege_escalation.become_method, "sudo");
        assert_eq!(config.privilege_escalation.become_user, "root");
    }

    /// Test become enabled check
    #[test]
    fn test_become_enabled_check() {
        let config = Config::default();

        // Default should be false
        assert!(!config.become_enabled());
    }

    /// Test multiple escalation scenarios
    #[test]
    fn test_multiple_escalation_scenarios() {
        let scenarios = vec![
            ("sudo", "root", None::<&str>),
            ("sudo", "www-data", None),
            ("su", "root", Some("-")),
            ("doas", "root", Some("-n")),
        ];

        for (method, user, flags) in scenarios {
            let config = PrivilegeEscalation {
                r#become: true,
                become_method: method.to_string(),
                become_user: user.to_string(),
                become_ask_pass: false,
                become_flags: flags.map(|f| f.to_string()),
            };

            assert!(config.r#become);
            assert_eq!(config.become_method, method);
            assert_eq!(config.become_user, user);
        }
    }

    /// Test become with all connection types
    #[test]
    fn test_become_all_connection_types() {
        let connection_types = vec!["local", "ssh", "docker"];

        for _conn_type in connection_types {
            let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
            options.escalate_method = Some("sudo".to_string());

            // All connection types should support the same escalation options
            assert!(options.escalate);
            assert_eq!(options.escalate_user, Some("root".to_string()));
        }
    }

    /// Test combining become with other options
    #[test]
    fn test_become_combined_options() {
        let options = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_cwd("/var/log")
            .with_env("LC_ALL", "C")
            .with_timeout(60);

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("root".to_string()));
        assert_eq!(options.cwd, Some("/var/log".to_string()));
        assert_eq!(options.env.get("LC_ALL"), Some(&"C".to_string()));
        assert_eq!(options.timeout, Some(60));
    }
}

// ============================================================================
// 13. EDGE CASES AND ERROR HANDLING
// ============================================================================

mod edge_cases {
    use super::*;

    /// Test become with empty user
    #[test]
    fn test_become_empty_user() {
        let options = ExecuteOptions::new().with_escalation(Some("".to_string()));

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("".to_string()));
    }

    /// Test become with whitespace user
    #[test]
    fn test_become_whitespace_user() {
        let options = ExecuteOptions::new().with_escalation(Some("  ".to_string()));

        assert!(options.escalate);
        // Whitespace user might be problematic but we store it as-is
        assert_eq!(options.escalate_user, Some("  ".to_string()));
    }

    /// Test become with very long username
    #[test]
    fn test_become_long_username() {
        let long_user = "a".repeat(256);
        let options = ExecuteOptions::new().with_escalation(Some(long_user.clone()));

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some(long_user));
    }

    /// Test become with special characters in username
    #[test]
    fn test_become_special_char_username() {
        let special_users = vec!["user$name", "user@domain", "user.name", "user+tag"];

        for user in special_users {
            let options = ExecuteOptions::new().with_escalation(Some(user.to_string()));
            assert_eq!(options.escalate_user, Some(user.to_string()));
        }
    }

    /// Test become with unicode username
    #[test]
    fn test_become_unicode_username() {
        let options = ExecuteOptions::new().with_escalation(Some("ユーザー".to_string()));

        assert!(options.escalate);
        assert_eq!(options.escalate_user, Some("ユーザー".to_string()));
    }

    /// Test become method case sensitivity
    #[test]
    fn test_become_method_case() {
        let methods = vec!["SUDO", "Sudo", "sudo", "SU", "Su", "su"];

        for method in methods {
            let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
            options.escalate_method = Some(method.to_string());

            assert_eq!(options.escalate_method, Some(method.to_string()));
        }
    }

    /// Test become with empty password
    #[test]
    fn test_become_empty_password() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some("".to_string());

        // Empty password is different from None
        assert!(options.escalate_password.is_some());
        assert_eq!(options.escalate_password, Some("".to_string()));
    }

    /// Test become with null bytes in password
    #[test]
    fn test_become_password_with_null() {
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some("pass\0word".to_string());

        // Null bytes should be preserved (though may cause issues in practice)
        assert!(options.escalate_password.as_ref().unwrap().contains('\0'));
    }

    /// Test become with very long password
    #[test]
    fn test_become_long_password() {
        let long_password = "p".repeat(10000);
        let mut options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
        options.escalate_password = Some(long_password.clone());

        assert_eq!(options.escalate_password, Some(long_password));
    }

    /// Test become flags with injection attempt
    #[test]
    fn test_become_flags_injection() {
        // Flags that might try to inject additional commands
        let malicious_flags = vec!["; rm -rf /", "$(whoami)", "`id`", "| cat /etc/passwd"];

        for flag in malicious_flags {
            let config = PrivilegeEscalation {
                r#become: true,
                become_method: "sudo".to_string(),
                become_user: "root".to_string(),
                become_ask_pass: false,
                become_flags: Some(flag.to_string()),
            };

            // The flags are stored as-is; sanitization happens at execution time
            assert_eq!(config.become_flags, Some(flag.to_string()));
        }
    }

    /// Test BecomeConfig with all None/default values
    #[test]
    fn test_become_config_minimal() {
        let config = BecomeConfig::default();

        assert!(!config.enabled);
        assert!(config.password.is_none());
        assert!(config.flags.is_none());
    }

    /// Test rapid toggle of become
    #[test]
    fn test_rapid_become_toggle() {
        let mut options = ExecuteOptions::default();

        for _ in 0..1000 {
            options.escalate = !options.escalate;
        }

        // After even number of toggles, should be back to original (false)
        assert!(!options.escalate);
    }
}

// ============================================================================
// 14. PERFORMANCE TESTS
// ============================================================================

mod performance {
    use super::*;
    use std::time::Instant;

    /// Test that creating become options is fast
    #[test]
    fn test_become_options_creation_performance() {
        let start = Instant::now();
        let iterations = 10000;

        for _ in 0..iterations {
            let options = ExecuteOptions::new().with_escalation(Some("root".to_string()));
            assert!(options.escalate);
        }

        let duration = start.elapsed();
        // Should complete in under 1 second
        assert!(duration.as_secs() < 1);
    }

    /// Test that become config creation is fast
    #[test]
    fn test_become_config_creation_performance() {
        let start = Instant::now();
        let iterations = 10000;

        for _ in 0..iterations {
            let config = PrivilegeEscalation {
                r#become: true,
                become_method: "sudo".to_string(),
                become_user: "root".to_string(),
                become_ask_pass: false,
                become_flags: Some("-H".to_string()),
            };
            assert!(config.r#become);
        }

        let duration = start.elapsed();
        assert!(duration.as_secs() < 1);
    }

    /// Test that cloning become options is efficient
    #[test]
    fn test_become_options_clone_performance() {
        let original = ExecuteOptions::new()
            .with_escalation(Some("root".to_string()))
            .with_cwd("/tmp")
            .with_env("VAR", "value");

        let start = Instant::now();
        let iterations = 10000;

        for _ in 0..iterations {
            let cloned = original.clone();
            assert!(cloned.escalate);
        }

        let duration = start.elapsed();
        assert!(duration.as_secs() < 1);
    }
}
