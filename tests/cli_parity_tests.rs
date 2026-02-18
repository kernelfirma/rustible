//! CLI Parity Test Suite for Issue #287
//!
//! These tests exercise the production CLI parser (clap) to validate
//! Rustible CLI defaults and flag parsing behavior.

use clap::Parser;
use std::path::PathBuf;

// Re-export config so the CLI module can resolve crate::config in this test crate.
mod config {
    pub use rustible::config::*;
}

// Pull in the real CLI module used by the binary.
#[path = "../src/cli/mod.rs"]
#[allow(dead_code, unused_imports)]
mod cli;

#[test]
fn test_global_flags_and_run_args_parsing() {
    let cli = cli::Cli::try_parse_from([
        "rustible",
        "--check",
        "--diff",
        "-e",
        "a=1",
        "-e",
        "b=2",
        "-v",
        "-v",
        "--limit",
        "web",
        "--forks",
        "10",
        "--timeout",
        "20",
        "run",
        "playbook.yml",
        "--tags",
        "one",
        "--tags",
        "two",
        "--skip-tags",
        "skip",
        "--start-at-task",
        "task name",
        "--step",
        "--become",
        "--become-user",
        "admin",
        "--become-method",
        "su",
        "-u",
        "remote",
        "--private-key",
        "key.pem",
    ])
    .unwrap();

    assert!(cli.check_mode);
    assert!(cli.diff_mode);
    assert_eq!(cli.extra_vars, vec!["a=1".to_string(), "b=2".to_string()]);
    assert_eq!(cli.verbose, 2);
    assert_eq!(cli.verbosity(), 2);
    assert_eq!(cli.limit.as_deref(), Some("web"));
    assert_eq!(cli.forks, 10);
    assert_eq!(cli.timeout, 20);

    match cli.command {
        cli::Commands::Run(args) => {
            assert_eq!(args.playbook, PathBuf::from("playbook.yml"));
            assert_eq!(args.tags, vec!["one".to_string(), "two".to_string()]);
            assert_eq!(args.skip_tags, vec!["skip".to_string()]);
            assert_eq!(args.start_at_task.as_deref(), Some("task name"));
            assert!(args.step);
            assert!(args.r#become);
            assert_eq!(args.become_user, "admin");
            assert_eq!(args.become_method, "su");
            assert_eq!(args.user.as_deref(), Some("remote"));
            assert_eq!(args.private_key, Some(PathBuf::from("key.pem")));
        }
        _ => panic!("expected run subcommand"),
    }
}

#[test]
fn test_run_defaults() {
    let cli = cli::Cli::try_parse_from(["rustible", "run", "playbook.yml"]).unwrap();

    assert!(!cli.check_mode);
    assert!(!cli.diff_mode);
    assert_eq!(cli.verbose, 0);
    assert_eq!(cli.forks, 5);
    assert_eq!(cli.timeout, 30);
    assert_eq!(cli.limit, None);
    assert!(cli.extra_vars.is_empty());

    match cli.command {
        cli::Commands::Run(args) => {
            assert!(args.tags.is_empty());
            assert!(args.skip_tags.is_empty());
            assert_eq!(args.become_method, "sudo");
            assert_eq!(args.become_user, "root");
            assert!(!args.ask_vault_pass);
            assert!(!args.r#become);
        }
        _ => panic!("expected run subcommand"),
    }
}

#[test]
fn test_tags_are_appended_without_splitting() {
    let cli = cli::Cli::try_parse_from([
        "rustible",
        "run",
        "playbook.yml",
        "--tags",
        "one,two",
        "--tags",
        "three",
    ])
    .unwrap();

    match cli.command {
        cli::Commands::Run(args) => {
            assert_eq!(args.tags, vec!["one,two".to_string(), "three".to_string()]);
        }
        _ => panic!("expected run subcommand"),
    }
}

#[test]
fn test_verbose_clamps_at_four() {
    let cli = cli::Cli::try_parse_from([
        "rustible",
        "-v",
        "-v",
        "-v",
        "-v",
        "-v",
        "run",
        "playbook.yml",
    ])
    .unwrap();

    assert_eq!(cli.verbose, 5);
    assert_eq!(cli.verbosity(), 4);
}

#[test]
fn test_output_format_json() {
    let cli =
        cli::Cli::try_parse_from(["rustible", "--output", "json", "run", "playbook.yml"]).unwrap();
    assert!(cli.is_json());
}

#[test]
fn test_check_subcommand_parsing() {
    let cli = cli::Cli::try_parse_from(["rustible", "check", "playbook.yml"]).unwrap();
    match cli.command {
        cli::Commands::Check(args) => {
            assert_eq!(args.playbook, PathBuf::from("playbook.yml"));
            assert_eq!(args.become_method, "sudo");
            assert_eq!(args.become_user, "root");
        }
        _ => panic!("expected check subcommand"),
    }
}

#[test]
fn test_invalid_forks_value_errors() {
    let err =
        cli::Cli::try_parse_from(["rustible", "--forks", "not-a-number", "run", "playbook.yml"]);
    assert!(err.is_err());
}
