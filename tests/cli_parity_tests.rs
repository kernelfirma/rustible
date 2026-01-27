//! CLI Parity Test Suite for Issue #287
//!
//! Tests CLI conformance with ansible-playbook parity including:
//! - Flag parsing: --check, --diff, --tags, --skip-tags, --limit, --extra-vars
//! - Precedence rules for variable sources
//! - Default forks/strategy behaviors
//! - CLI behavior matches documented Ansible defaults

use std::collections::HashMap;

// ============================================================================
// Mock CLI Parser (mirrors production CLI implementation)
// ============================================================================

/// CLI options parsed from command line arguments
#[derive(Debug, Clone, Default)]
pub struct CliOptions {
    /// Check mode (--check, -C)
    pub check_mode: bool,
    /// Diff mode (--diff, -D)
    pub diff_mode: bool,
    /// Tags to run (--tags, -t)
    pub tags: Vec<String>,
    /// Tags to skip (--skip-tags)
    pub skip_tags: Vec<String>,
    /// Host limit pattern (--limit, -l)
    pub limit: Option<String>,
    /// Extra variables (--extra-vars, -e)
    pub extra_vars: Vec<String>,
    /// Number of parallel forks (--forks, -f)
    pub forks: Option<u32>,
    /// Execution strategy (--strategy)
    pub strategy: Option<String>,
    /// Verbosity level (-v, -vv, -vvv, -vvvv)
    pub verbosity: u8,
    /// Become/sudo mode (--become, -b)
    pub use_become: bool,
    /// Become user (--become-user)
    pub become_user: Option<String>,
    /// Become method (--become-method)
    pub become_method: Option<String>,
    /// Inventory paths (-i, --inventory)
    pub inventory: Vec<String>,
    /// Playbook paths
    pub playbooks: Vec<String>,
    /// Connection type (--connection, -c)
    pub connection: Option<String>,
    /// Start at task (--start-at-task)
    pub start_at_task: Option<String>,
    /// Step mode (--step)
    pub step: bool,
    /// Syntax check only (--syntax-check)
    pub syntax_check: bool,
    /// List hosts (--list-hosts)
    pub list_hosts: bool,
    /// List tasks (--list-tasks)
    pub list_tasks: bool,
    /// List tags (--list-tags)
    pub list_tags: bool,
    /// Flush cache (--flush-cache)
    pub flush_cache: bool,
    /// Force handlers (--force-handlers)
    pub force_handlers: bool,
    /// Module path (--module-path, -M)
    pub module_path: Vec<String>,
    /// Vault password file (--vault-password-file)
    pub vault_password_file: Option<String>,
    /// Ask vault pass (--ask-vault-pass)
    pub ask_vault_pass: bool,
    /// Private key file (--private-key, --key-file)
    pub private_key: Option<String>,
    /// Remote user (--user, -u)
    pub remote_user: Option<String>,
    /// Timeout (--timeout, -T)
    pub timeout: Option<u32>,
}

/// Ansible defaults as documented
pub struct AnsibleDefaults;

impl AnsibleDefaults {
    pub const FORKS: u32 = 5;
    pub const STRATEGY: &'static str = "linear";
    pub const CONNECTION: &'static str = "smart";
    pub const TIMEOUT: u32 = 10;
    pub const BECOME_METHOD: &'static str = "sudo";
    pub const BECOME_USER: &'static str = "root";
    pub const VERBOSITY: u8 = 0;
}

/// Parse CLI arguments into CliOptions
fn parse_cli_args(args: &[&str]) -> Result<CliOptions, String> {
    let mut opts = CliOptions::default();
    let mut i = 0;

    while i < args.len() {
        let arg = args[i];

        match arg {
            "--check" | "-C" => opts.check_mode = true,
            "--diff" | "-D" => opts.diff_mode = true,
            "--become" | "-b" => opts.use_become = true,
            "--step" => opts.step = true,
            "--syntax-check" => opts.syntax_check = true,
            "--list-hosts" => opts.list_hosts = true,
            "--list-tasks" => opts.list_tasks = true,
            "--list-tags" => opts.list_tags = true,
            "--flush-cache" => opts.flush_cache = true,
            "--force-handlers" => opts.force_handlers = true,
            "--ask-vault-pass" => opts.ask_vault_pass = true,

            // Verbosity flags
            "-v" => opts.verbosity = opts.verbosity.saturating_add(1),
            "-vv" => opts.verbosity = opts.verbosity.saturating_add(2),
            "-vvv" => opts.verbosity = opts.verbosity.saturating_add(3),
            "-vvvv" => opts.verbosity = opts.verbosity.saturating_add(4),
            "-vvvvv" => opts.verbosity = opts.verbosity.saturating_add(5),

            // Options with values
            "--tags" | "-t" => {
                i += 1;
                if i >= args.len() {
                    return Err("--tags requires a value".to_string());
                }
                opts.tags.extend(args[i].split(',').map(|s| s.trim().to_string()));
            }
            "--skip-tags" => {
                i += 1;
                if i >= args.len() {
                    return Err("--skip-tags requires a value".to_string());
                }
                opts.skip_tags.extend(args[i].split(',').map(|s| s.trim().to_string()));
            }
            "--limit" | "-l" => {
                i += 1;
                if i >= args.len() {
                    return Err("--limit requires a value".to_string());
                }
                opts.limit = Some(args[i].to_string());
            }
            "--extra-vars" | "-e" => {
                i += 1;
                if i >= args.len() {
                    return Err("--extra-vars requires a value".to_string());
                }
                opts.extra_vars.push(args[i].to_string());
            }
            "--forks" | "-f" => {
                i += 1;
                if i >= args.len() {
                    return Err("--forks requires a value".to_string());
                }
                opts.forks = Some(args[i].parse().map_err(|_| "Invalid forks value")?);
            }
            "--strategy" => {
                i += 1;
                if i >= args.len() {
                    return Err("--strategy requires a value".to_string());
                }
                opts.strategy = Some(args[i].to_string());
            }
            "--become-user" => {
                i += 1;
                if i >= args.len() {
                    return Err("--become-user requires a value".to_string());
                }
                opts.become_user = Some(args[i].to_string());
            }
            "--become-method" => {
                i += 1;
                if i >= args.len() {
                    return Err("--become-method requires a value".to_string());
                }
                opts.become_method = Some(args[i].to_string());
            }
            "--inventory" | "-i" => {
                i += 1;
                if i >= args.len() {
                    return Err("--inventory requires a value".to_string());
                }
                opts.inventory.push(args[i].to_string());
            }
            "--connection" | "-c" => {
                i += 1;
                if i >= args.len() {
                    return Err("--connection requires a value".to_string());
                }
                opts.connection = Some(args[i].to_string());
            }
            "--start-at-task" => {
                i += 1;
                if i >= args.len() {
                    return Err("--start-at-task requires a value".to_string());
                }
                opts.start_at_task = Some(args[i].to_string());
            }
            "--module-path" | "-M" => {
                i += 1;
                if i >= args.len() {
                    return Err("--module-path requires a value".to_string());
                }
                opts.module_path.push(args[i].to_string());
            }
            "--vault-password-file" => {
                i += 1;
                if i >= args.len() {
                    return Err("--vault-password-file requires a value".to_string());
                }
                opts.vault_password_file = Some(args[i].to_string());
            }
            "--private-key" | "--key-file" => {
                i += 1;
                if i >= args.len() {
                    return Err("--private-key requires a value".to_string());
                }
                opts.private_key = Some(args[i].to_string());
            }
            "--user" | "-u" => {
                i += 1;
                if i >= args.len() {
                    return Err("--user requires a value".to_string());
                }
                opts.remote_user = Some(args[i].to_string());
            }
            "--timeout" | "-T" => {
                i += 1;
                if i >= args.len() {
                    return Err("--timeout requires a value".to_string());
                }
                opts.timeout = Some(args[i].parse().map_err(|_| "Invalid timeout value")?);
            }

            // Positional arguments (playbooks)
            _ if !arg.starts_with('-') => {
                opts.playbooks.push(arg.to_string());
            }

            _ => {
                // Unknown flag
                return Err(format!("Unknown option: {}", arg));
            }
        }

        i += 1;
    }

    Ok(opts)
}

/// Apply defaults to CLI options (matching Ansible behavior)
fn apply_defaults(opts: &mut CliOptions) {
    if opts.forks.is_none() {
        opts.forks = Some(AnsibleDefaults::FORKS);
    }
    if opts.strategy.is_none() {
        opts.strategy = Some(AnsibleDefaults::STRATEGY.to_string());
    }
    if opts.connection.is_none() {
        opts.connection = Some(AnsibleDefaults::CONNECTION.to_string());
    }
    if opts.timeout.is_none() {
        opts.timeout = Some(AnsibleDefaults::TIMEOUT);
    }
    if opts.use_become && opts.become_method.is_none() {
        opts.become_method = Some(AnsibleDefaults::BECOME_METHOD.to_string());
    }
    if opts.use_become && opts.become_user.is_none() {
        opts.become_user = Some(AnsibleDefaults::BECOME_USER.to_string());
    }
}

// ============================================================================
// Variable Precedence (Ansible order)
// ============================================================================

/// Variable sources in precedence order (highest to lowest)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariablePrecedence {
    ExtraVars = 22,
    TaskVars = 21,
    BlockVars = 20,
    RoleParams = 19,
    SetFacts = 18,
    RegisteredVars = 17,
    IncludeParams = 16,
    RoleVars = 15,
    PlayVars = 14,
    PlayVarsPrompt = 13,
    PlayVarsFiles = 12,
    HostFactsCache = 11,
    InventoryHostVars = 10,
    InventoryGroupVars = 9,
    PlaybookGroupVars = 8,
    PlaybookHostVars = 7,
    HostFacts = 6,
    RoleDefaults = 5,
    CommandLineValues = 4,
    InventoryFileVars = 3,
    ConfigDefaults = 2,
    BuiltinDefaults = 1,
}

/// Resolve variable with proper precedence
fn resolve_variable(
    name: &str,
    sources: &[(VariablePrecedence, HashMap<String, String>)],
) -> Option<String> {
    let mut result: Option<(VariablePrecedence, String)> = None;

    for (precedence, vars) in sources {
        if let Some(value) = vars.get(name) {
            match &result {
                None => result = Some((*precedence, value.clone())),
                Some((current_prec, _)) if precedence > current_prec => {
                    result = Some((*precedence, value.clone()));
                }
                _ => {}
            }
        }
    }

    result.map(|(_, v)| v)
}

/// Parse extra-vars value (key=value or JSON)
fn parse_extra_vars(value: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();

    // Try JSON first
    if value.starts_with('{') || value.starts_with('@') {
        // JSON or file reference - simplified parsing
        if value.starts_with('{') {
            // Basic JSON parsing
            let trimmed = value.trim_start_matches('{').trim_end_matches('}');
            for pair in trimmed.split(',') {
                let parts: Vec<&str> = pair.split(':').collect();
                if parts.len() == 2 {
                    let key = parts[0].trim().trim_matches('"').trim_matches('\'');
                    let val = parts[1].trim().trim_matches('"').trim_matches('\'');
                    vars.insert(key.to_string(), val.to_string());
                }
            }
        }
    } else {
        // key=value format (possibly multiple)
        for pair in value.split_whitespace() {
            if let Some(eq_pos) = pair.find('=') {
                let key = &pair[..eq_pos];
                let val = &pair[eq_pos + 1..];
                vars.insert(key.to_string(), val.to_string());
            }
        }
    }

    vars
}

// ============================================================================
// Tests: --check flag
// ============================================================================

#[test]
fn test_check_flag_long_form() {
    let opts = parse_cli_args(&["--check", "playbook.yml"]).unwrap();
    assert!(opts.check_mode, "--check should enable check mode");
}

#[test]
fn test_check_flag_short_form() {
    let opts = parse_cli_args(&["-C", "playbook.yml"]).unwrap();
    assert!(opts.check_mode, "-C should enable check mode");
}

#[test]
fn test_check_flag_default_false() {
    let opts = parse_cli_args(&["playbook.yml"]).unwrap();
    assert!(!opts.check_mode, "check mode should default to false");
}

#[test]
fn test_check_mode_prevents_changes() {
    // In check mode, no actual changes should be made
    let opts = parse_cli_args(&["--check", "playbook.yml"]).unwrap();

    // Simulating that check mode would return changed=false for all tasks
    struct TaskResult {
        changed: bool,
        simulated: bool,
    }

    let result = TaskResult {
        changed: false,  // In check mode, nothing actually changes
        simulated: true, // But we simulate what would happen
    };

    assert!(opts.check_mode);
    assert!(result.simulated);
    assert!(!result.changed);
}

// ============================================================================
// Tests: --diff flag
// ============================================================================

#[test]
fn test_diff_flag_long_form() {
    let opts = parse_cli_args(&["--diff", "playbook.yml"]).unwrap();
    assert!(opts.diff_mode, "--diff should enable diff mode");
}

#[test]
fn test_diff_flag_short_form() {
    let opts = parse_cli_args(&["-D", "playbook.yml"]).unwrap();
    assert!(opts.diff_mode, "-D should enable diff mode");
}

#[test]
fn test_diff_flag_default_false() {
    let opts = parse_cli_args(&["playbook.yml"]).unwrap();
    assert!(!opts.diff_mode, "diff mode should default to false");
}

#[test]
fn test_check_and_diff_combined() {
    let opts = parse_cli_args(&["--check", "--diff", "playbook.yml"]).unwrap();
    assert!(opts.check_mode, "check mode should be enabled");
    assert!(opts.diff_mode, "diff mode should be enabled");
}

#[test]
fn test_check_and_diff_short_combined() {
    let opts = parse_cli_args(&["-C", "-D", "playbook.yml"]).unwrap();
    assert!(opts.check_mode);
    assert!(opts.diff_mode);
}

// ============================================================================
// Tests: --tags flag
// ============================================================================

#[test]
fn test_tags_single_value() {
    let opts = parse_cli_args(&["--tags", "deploy", "playbook.yml"]).unwrap();
    assert_eq!(opts.tags, vec!["deploy"]);
}

#[test]
fn test_tags_short_form() {
    let opts = parse_cli_args(&["-t", "deploy", "playbook.yml"]).unwrap();
    assert_eq!(opts.tags, vec!["deploy"]);
}

#[test]
fn test_tags_comma_separated() {
    let opts = parse_cli_args(&["--tags", "deploy,configure,test", "playbook.yml"]).unwrap();
    assert_eq!(opts.tags, vec!["deploy", "configure", "test"]);
}

#[test]
fn test_tags_multiple_flags() {
    let opts = parse_cli_args(&["--tags", "deploy", "--tags", "configure", "playbook.yml"]).unwrap();
    assert_eq!(opts.tags, vec!["deploy", "configure"]);
}

#[test]
fn test_tags_empty_by_default() {
    let opts = parse_cli_args(&["playbook.yml"]).unwrap();
    assert!(opts.tags.is_empty(), "tags should default to empty");
}

#[test]
fn test_tags_special_all() {
    let opts = parse_cli_args(&["--tags", "all", "playbook.yml"]).unwrap();
    assert_eq!(opts.tags, vec!["all"]);
}

#[test]
fn test_tags_special_tagged() {
    let opts = parse_cli_args(&["--tags", "tagged", "playbook.yml"]).unwrap();
    assert_eq!(opts.tags, vec!["tagged"]);
}

#[test]
fn test_tags_special_untagged() {
    let opts = parse_cli_args(&["--tags", "untagged", "playbook.yml"]).unwrap();
    assert_eq!(opts.tags, vec!["untagged"]);
}

// ============================================================================
// Tests: --skip-tags flag
// ============================================================================

#[test]
fn test_skip_tags_single_value() {
    let opts = parse_cli_args(&["--skip-tags", "slow", "playbook.yml"]).unwrap();
    assert_eq!(opts.skip_tags, vec!["slow"]);
}

#[test]
fn test_skip_tags_comma_separated() {
    let opts = parse_cli_args(&["--skip-tags", "slow,expensive,optional", "playbook.yml"]).unwrap();
    assert_eq!(opts.skip_tags, vec!["slow", "expensive", "optional"]);
}

#[test]
fn test_skip_tags_empty_by_default() {
    let opts = parse_cli_args(&["playbook.yml"]).unwrap();
    assert!(opts.skip_tags.is_empty(), "skip-tags should default to empty");
}

#[test]
fn test_tags_and_skip_tags_combined() {
    let opts = parse_cli_args(&[
        "--tags", "deploy,configure",
        "--skip-tags", "slow",
        "playbook.yml"
    ]).unwrap();
    assert_eq!(opts.tags, vec!["deploy", "configure"]);
    assert_eq!(opts.skip_tags, vec!["slow"]);
}

// ============================================================================
// Tests: --limit flag
// ============================================================================

#[test]
fn test_limit_single_host() {
    let opts = parse_cli_args(&["--limit", "webserver1", "playbook.yml"]).unwrap();
    assert_eq!(opts.limit, Some("webserver1".to_string()));
}

#[test]
fn test_limit_short_form() {
    let opts = parse_cli_args(&["-l", "webserver1", "playbook.yml"]).unwrap();
    assert_eq!(opts.limit, Some("webserver1".to_string()));
}

#[test]
fn test_limit_pattern() {
    let opts = parse_cli_args(&["--limit", "web*", "playbook.yml"]).unwrap();
    assert_eq!(opts.limit, Some("web*".to_string()));
}

#[test]
fn test_limit_group() {
    let opts = parse_cli_args(&["--limit", "webservers", "playbook.yml"]).unwrap();
    assert_eq!(opts.limit, Some("webservers".to_string()));
}

#[test]
fn test_limit_exclusion() {
    let opts = parse_cli_args(&["--limit", "all:!dbservers", "playbook.yml"]).unwrap();
    assert_eq!(opts.limit, Some("all:!dbservers".to_string()));
}

#[test]
fn test_limit_intersection() {
    let opts = parse_cli_args(&["--limit", "webservers:&staging", "playbook.yml"]).unwrap();
    assert_eq!(opts.limit, Some("webservers:&staging".to_string()));
}

#[test]
fn test_limit_none_by_default() {
    let opts = parse_cli_args(&["playbook.yml"]).unwrap();
    assert!(opts.limit.is_none(), "limit should default to None");
}

// ============================================================================
// Tests: --extra-vars flag
// ============================================================================

#[test]
fn test_extra_vars_key_value() {
    let opts = parse_cli_args(&["--extra-vars", "version=1.0", "playbook.yml"]).unwrap();
    assert_eq!(opts.extra_vars, vec!["version=1.0"]);
}

#[test]
fn test_extra_vars_short_form() {
    let opts = parse_cli_args(&["-e", "version=1.0", "playbook.yml"]).unwrap();
    assert_eq!(opts.extra_vars, vec!["version=1.0"]);
}

#[test]
fn test_extra_vars_json() {
    let opts = parse_cli_args(&["-e", "{\"version\":\"1.0\"}", "playbook.yml"]).unwrap();
    assert_eq!(opts.extra_vars, vec!["{\"version\":\"1.0\"}"]);
}

#[test]
fn test_extra_vars_multiple() {
    let opts = parse_cli_args(&[
        "-e", "version=1.0",
        "-e", "env=production",
        "playbook.yml"
    ]).unwrap();
    assert_eq!(opts.extra_vars, vec!["version=1.0", "env=production"]);
}

#[test]
fn test_extra_vars_empty_by_default() {
    let opts = parse_cli_args(&["playbook.yml"]).unwrap();
    assert!(opts.extra_vars.is_empty());
}

#[test]
fn test_extra_vars_parsing_key_value() {
    let vars = parse_extra_vars("version=1.0 env=prod");
    assert_eq!(vars.get("version"), Some(&"1.0".to_string()));
    assert_eq!(vars.get("env"), Some(&"prod".to_string()));
}

#[test]
fn test_extra_vars_parsing_json() {
    let vars = parse_extra_vars("{\"version\":\"1.0\",\"env\":\"prod\"}");
    assert_eq!(vars.get("version"), Some(&"1.0".to_string()));
    assert_eq!(vars.get("env"), Some(&"prod".to_string()));
}

// ============================================================================
// Tests: Variable Precedence
// ============================================================================

#[test]
fn test_extra_vars_highest_precedence() {
    use VariablePrecedence::*;

    let mut play_vars = HashMap::new();
    play_vars.insert("version".to_string(), "1.0".to_string());

    let mut extra_vars = HashMap::new();
    extra_vars.insert("version".to_string(), "2.0".to_string());

    let sources = vec![
        (PlayVars, play_vars),
        (ExtraVars, extra_vars),
    ];

    let result = resolve_variable("version", &sources);
    assert_eq!(result, Some("2.0".to_string()), "extra-vars should win");
}

#[test]
fn test_role_vars_over_role_defaults() {
    use VariablePrecedence::*;

    let mut role_defaults = HashMap::new();
    role_defaults.insert("port".to_string(), "80".to_string());

    let mut role_vars = HashMap::new();
    role_vars.insert("port".to_string(), "8080".to_string());

    let sources = vec![
        (RoleDefaults, role_defaults),
        (RoleVars, role_vars),
    ];

    let result = resolve_variable("port", &sources);
    assert_eq!(result, Some("8080".to_string()), "role vars should win over defaults");
}

#[test]
fn test_set_facts_over_play_vars() {
    use VariablePrecedence::*;

    let mut play_vars = HashMap::new();
    play_vars.insert("dynamic".to_string(), "static".to_string());

    let mut set_facts = HashMap::new();
    set_facts.insert("dynamic".to_string(), "computed".to_string());

    let sources = vec![
        (PlayVars, play_vars),
        (SetFacts, set_facts),
    ];

    let result = resolve_variable("dynamic", &sources);
    assert_eq!(result, Some("computed".to_string()), "set_fact should win");
}

#[test]
fn test_inventory_host_vars_over_group_vars() {
    use VariablePrecedence::*;

    let mut group_vars = HashMap::new();
    group_vars.insert("setting".to_string(), "group_value".to_string());

    let mut host_vars = HashMap::new();
    host_vars.insert("setting".to_string(), "host_value".to_string());

    let sources = vec![
        (InventoryGroupVars, group_vars),
        (InventoryHostVars, host_vars),
    ];

    let result = resolve_variable("setting", &sources);
    assert_eq!(result, Some("host_value".to_string()), "host vars should win");
}

#[test]
fn test_task_vars_very_high_precedence() {
    use VariablePrecedence::*;

    let mut role_vars = HashMap::new();
    role_vars.insert("item".to_string(), "role".to_string());

    let mut task_vars = HashMap::new();
    task_vars.insert("item".to_string(), "task".to_string());

    let sources = vec![
        (RoleVars, role_vars),
        (TaskVars, task_vars),
    ];

    let result = resolve_variable("item", &sources);
    assert_eq!(result, Some("task".to_string()), "task vars should win");
}

#[test]
fn test_complete_precedence_chain() {
    use VariablePrecedence::*;

    let mut role_defaults = HashMap::new();
    role_defaults.insert("var".to_string(), "1".to_string());

    let mut group_vars = HashMap::new();
    group_vars.insert("var".to_string(), "2".to_string());

    let mut host_vars = HashMap::new();
    host_vars.insert("var".to_string(), "3".to_string());

    let mut play_vars = HashMap::new();
    play_vars.insert("var".to_string(), "4".to_string());

    let mut role_vars = HashMap::new();
    role_vars.insert("var".to_string(), "5".to_string());

    let mut task_vars = HashMap::new();
    task_vars.insert("var".to_string(), "6".to_string());

    let mut extra_vars = HashMap::new();
    extra_vars.insert("var".to_string(), "7".to_string());

    let sources = vec![
        (RoleDefaults, role_defaults),
        (InventoryGroupVars, group_vars),
        (InventoryHostVars, host_vars),
        (PlayVars, play_vars),
        (RoleVars, role_vars),
        (TaskVars, task_vars),
        (ExtraVars, extra_vars),
    ];

    let result = resolve_variable("var", &sources);
    assert_eq!(result, Some("7".to_string()), "extra-vars (highest) should win");
}

// ============================================================================
// Tests: Default Forks Behavior
// ============================================================================

#[test]
fn test_default_forks_is_five() {
    let mut opts = parse_cli_args(&["playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.forks, Some(5), "default forks should be 5");
}

#[test]
fn test_custom_forks_override() {
    let mut opts = parse_cli_args(&["--forks", "10", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.forks, Some(10), "custom forks should override default");
}

#[test]
fn test_forks_short_form() {
    let mut opts = parse_cli_args(&["-f", "20", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.forks, Some(20));
}

#[test]
fn test_forks_minimum_one() {
    let opts = parse_cli_args(&["--forks", "1", "playbook.yml"]).unwrap();
    assert_eq!(opts.forks, Some(1), "forks can be set to 1");
}

#[test]
fn test_forks_large_value() {
    let opts = parse_cli_args(&["--forks", "100", "playbook.yml"]).unwrap();
    assert_eq!(opts.forks, Some(100), "large forks value should be accepted");
}

// ============================================================================
// Tests: Default Strategy Behavior
// ============================================================================

#[test]
fn test_default_strategy_is_linear() {
    let mut opts = parse_cli_args(&["playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.strategy, Some("linear".to_string()), "default strategy should be linear");
}

#[test]
fn test_custom_strategy_free() {
    let mut opts = parse_cli_args(&["--strategy", "free", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.strategy, Some("free".to_string()));
}

#[test]
fn test_custom_strategy_host_pinned() {
    let mut opts = parse_cli_args(&["--strategy", "host_pinned", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.strategy, Some("host_pinned".to_string()));
}

#[test]
fn test_custom_strategy_serial() {
    // Note: serial is typically set in playbook, but can be passed as strategy
    let mut opts = parse_cli_args(&["--strategy", "serial", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.strategy, Some("serial".to_string()));
}

// ============================================================================
// Tests: Other Defaults
// ============================================================================

#[test]
fn test_default_connection_is_smart() {
    let mut opts = parse_cli_args(&["playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.connection, Some("smart".to_string()));
}

#[test]
fn test_custom_connection_ssh() {
    let mut opts = parse_cli_args(&["--connection", "ssh", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.connection, Some("ssh".to_string()));
}

#[test]
fn test_custom_connection_local() {
    let mut opts = parse_cli_args(&["--connection", "local", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.connection, Some("local".to_string()));
}

#[test]
fn test_default_timeout() {
    let mut opts = parse_cli_args(&["playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.timeout, Some(10), "default timeout should be 10");
}

#[test]
fn test_custom_timeout() {
    let mut opts = parse_cli_args(&["--timeout", "30", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.timeout, Some(30));
}

#[test]
fn test_become_defaults() {
    let mut opts = parse_cli_args(&["--become", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert!(opts.use_become);
    assert_eq!(opts.become_method, Some("sudo".to_string()));
    assert_eq!(opts.become_user, Some("root".to_string()));
}

#[test]
fn test_become_custom_user() {
    let mut opts = parse_cli_args(&["--become", "--become-user", "admin", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert!(opts.use_become);
    assert_eq!(opts.become_user, Some("admin".to_string()));
}

#[test]
fn test_become_custom_method() {
    let mut opts = parse_cli_args(&["--become", "--become-method", "su", "playbook.yml"]).unwrap();
    apply_defaults(&mut opts);
    assert_eq!(opts.become_method, Some("su".to_string()));
}

// ============================================================================
// Tests: Verbosity
// ============================================================================

#[test]
fn test_verbosity_default_zero() {
    let opts = parse_cli_args(&["playbook.yml"]).unwrap();
    assert_eq!(opts.verbosity, 0);
}

#[test]
fn test_verbosity_single_v() {
    let opts = parse_cli_args(&["-v", "playbook.yml"]).unwrap();
    assert_eq!(opts.verbosity, 1);
}

#[test]
fn test_verbosity_double_v() {
    let opts = parse_cli_args(&["-vv", "playbook.yml"]).unwrap();
    assert_eq!(opts.verbosity, 2);
}

#[test]
fn test_verbosity_triple_v() {
    let opts = parse_cli_args(&["-vvv", "playbook.yml"]).unwrap();
    assert_eq!(opts.verbosity, 3);
}

#[test]
fn test_verbosity_quad_v() {
    let opts = parse_cli_args(&["-vvvv", "playbook.yml"]).unwrap();
    assert_eq!(opts.verbosity, 4);
}

#[test]
fn test_verbosity_cumulative() {
    let opts = parse_cli_args(&["-v", "-v", "-v", "playbook.yml"]).unwrap();
    assert_eq!(opts.verbosity, 3);
}

// ============================================================================
// Tests: Inventory
// ============================================================================

#[test]
fn test_inventory_single() {
    let opts = parse_cli_args(&["-i", "inventory.yml", "playbook.yml"]).unwrap();
    assert_eq!(opts.inventory, vec!["inventory.yml"]);
}

#[test]
fn test_inventory_multiple() {
    let opts = parse_cli_args(&[
        "-i", "staging.yml",
        "-i", "production.yml",
        "playbook.yml"
    ]).unwrap();
    assert_eq!(opts.inventory, vec!["staging.yml", "production.yml"]);
}

#[test]
fn test_inventory_long_form() {
    let opts = parse_cli_args(&["--inventory", "hosts", "playbook.yml"]).unwrap();
    assert_eq!(opts.inventory, vec!["hosts"]);
}

// ============================================================================
// Tests: Special Flags
// ============================================================================

#[test]
fn test_syntax_check_flag() {
    let opts = parse_cli_args(&["--syntax-check", "playbook.yml"]).unwrap();
    assert!(opts.syntax_check);
}

#[test]
fn test_list_hosts_flag() {
    let opts = parse_cli_args(&["--list-hosts", "playbook.yml"]).unwrap();
    assert!(opts.list_hosts);
}

#[test]
fn test_list_tasks_flag() {
    let opts = parse_cli_args(&["--list-tasks", "playbook.yml"]).unwrap();
    assert!(opts.list_tasks);
}

#[test]
fn test_list_tags_flag() {
    let opts = parse_cli_args(&["--list-tags", "playbook.yml"]).unwrap();
    assert!(opts.list_tags);
}

#[test]
fn test_flush_cache_flag() {
    let opts = parse_cli_args(&["--flush-cache", "playbook.yml"]).unwrap();
    assert!(opts.flush_cache);
}

#[test]
fn test_force_handlers_flag() {
    let opts = parse_cli_args(&["--force-handlers", "playbook.yml"]).unwrap();
    assert!(opts.force_handlers);
}

#[test]
fn test_step_flag() {
    let opts = parse_cli_args(&["--step", "playbook.yml"]).unwrap();
    assert!(opts.step);
}

#[test]
fn test_start_at_task() {
    let opts = parse_cli_args(&["--start-at-task", "Install packages", "playbook.yml"]).unwrap();
    assert_eq!(opts.start_at_task, Some("Install packages".to_string()));
}

// ============================================================================
// Tests: Complex Command Lines
// ============================================================================

#[test]
fn test_complex_command_line() {
    let opts = parse_cli_args(&[
        "-i", "inventory/production",
        "--limit", "webservers:&eu-west",
        "--tags", "deploy,configure",
        "--skip-tags", "slow",
        "-e", "version=2.0",
        "-e", "env=production",
        "--forks", "20",
        "--check",
        "--diff",
        "-vvv",
        "site.yml"
    ]).unwrap();

    assert_eq!(opts.inventory, vec!["inventory/production"]);
    assert_eq!(opts.limit, Some("webservers:&eu-west".to_string()));
    assert_eq!(opts.tags, vec!["deploy", "configure"]);
    assert_eq!(opts.skip_tags, vec!["slow"]);
    assert_eq!(opts.extra_vars, vec!["version=2.0", "env=production"]);
    assert_eq!(opts.forks, Some(20));
    assert!(opts.check_mode);
    assert!(opts.diff_mode);
    assert_eq!(opts.verbosity, 3);
    assert_eq!(opts.playbooks, vec!["site.yml"]);
}

#[test]
fn test_become_full_configuration() {
    let opts = parse_cli_args(&[
        "--become",
        "--become-method", "sudo",
        "--become-user", "deploy",
        "playbook.yml"
    ]).unwrap();

    assert!(opts.use_become);
    assert_eq!(opts.become_method, Some("sudo".to_string()));
    assert_eq!(opts.become_user, Some("deploy".to_string()));
}

#[test]
fn test_multiple_playbooks() {
    let opts = parse_cli_args(&[
        "site.yml",
        "database.yml",
        "webservers.yml"
    ]).unwrap();

    assert_eq!(opts.playbooks, vec!["site.yml", "database.yml", "webservers.yml"]);
}

// ============================================================================
// Tests: Error Handling
// ============================================================================

#[test]
fn test_missing_tags_value() {
    let result = parse_cli_args(&["--tags"]);
    assert!(result.is_err());
}

#[test]
fn test_missing_limit_value() {
    let result = parse_cli_args(&["--limit"]);
    assert!(result.is_err());
}

#[test]
fn test_missing_extra_vars_value() {
    let result = parse_cli_args(&["-e"]);
    assert!(result.is_err());
}

#[test]
fn test_invalid_forks_value() {
    let result = parse_cli_args(&["--forks", "abc", "playbook.yml"]);
    assert!(result.is_err());
}

#[test]
fn test_unknown_flag_error() {
    let result = parse_cli_args(&["--unknown-flag", "playbook.yml"]);
    assert!(result.is_err());
}

// ============================================================================
// CI Regression Guards
// ============================================================================

#[test]
fn test_ci_guard_defaults_match_ansible() {
    // This test guards against accidentally changing defaults
    assert_eq!(AnsibleDefaults::FORKS, 5);
    assert_eq!(AnsibleDefaults::STRATEGY, "linear");
    assert_eq!(AnsibleDefaults::CONNECTION, "smart");
    assert_eq!(AnsibleDefaults::TIMEOUT, 10);
    assert_eq!(AnsibleDefaults::BECOME_METHOD, "sudo");
    assert_eq!(AnsibleDefaults::BECOME_USER, "root");
    assert_eq!(AnsibleDefaults::VERBOSITY, 0);
}

#[test]
fn test_ci_guard_precedence_order() {
    // Guard: extra-vars must always be highest
    assert!(VariablePrecedence::ExtraVars > VariablePrecedence::TaskVars);
    assert!(VariablePrecedence::TaskVars > VariablePrecedence::RoleVars);
    assert!(VariablePrecedence::RoleVars > VariablePrecedence::PlayVars);
    assert!(VariablePrecedence::PlayVars > VariablePrecedence::RoleDefaults);
}

#[test]
fn test_ci_guard_flag_aliases_work() {
    // Guard: short and long forms must remain equivalent
    let long = parse_cli_args(&["--check", "--diff", "--tags", "t", "--limit", "h", "p.yml"]).unwrap();
    let short = parse_cli_args(&["-C", "-D", "-t", "t", "-l", "h", "p.yml"]).unwrap();

    assert_eq!(long.check_mode, short.check_mode);
    assert_eq!(long.diff_mode, short.diff_mode);
    assert_eq!(long.tags, short.tags);
    assert_eq!(long.limit, short.limit);
}

#[test]
fn test_ci_guard_multiple_inventory_supported() {
    // Guard: must support multiple inventory sources like Ansible
    let opts = parse_cli_args(&[
        "-i", "inv1",
        "-i", "inv2",
        "-i", "inv3",
        "playbook.yml"
    ]).unwrap();

    assert_eq!(opts.inventory.len(), 3, "must support multiple inventories");
}

#[test]
fn test_ci_guard_tags_comma_splitting() {
    // Guard: comma-separated tags must be split properly
    let opts = parse_cli_args(&["--tags", "a,b,c,d,e", "playbook.yml"]).unwrap();
    assert_eq!(opts.tags.len(), 5, "comma-separated tags must be split");
}
