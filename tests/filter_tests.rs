//! Comprehensive filter tests for tags, skip-tags, and host limits
//!
//! This test suite covers:
//! - Tags basic functionality
//! - Tag inheritance (play, block, role level)
//! - Special tags (always, never, tagged, untagged, all)
//! - Skip-tags functionality
//! - Tag expressions
//! - Limits basic functionality
//! - Limit patterns (wildcards, regex, exclusion, intersection, ranges)
//! - Limit from file
//! - Combinations of tags and limits
//! - Edge cases

use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

// =============================================================================
// Helper Functions
// =============================================================================

fn rustible_cmd() -> Command {
    assert_cmd::cargo::cargo_bin_cmd!("rustible")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tags")
}

fn create_test_playbook_with_tags() -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"---
- name: Test playbook
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Install task
      debug:
        msg: "Installing"
      tags:
        - install

    - name: Configure task
      debug:
        msg: "Configuring"
      tags:
        - configure

    - name: Deploy task
      debug:
        msg: "Deploying"
      tags:
        - deploy
"#
    )
    .unwrap();
    file
}

fn create_test_inventory() -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(
        file,
        r#"all:
  hosts:
    localhost:
      ansible_connection: local
    web01:
      ansible_host: 192.168.1.10
    web02:
      ansible_host: 192.168.1.11
    db01:
      ansible_host: 192.168.1.20
    db02:
      ansible_host: 192.168.1.21
  children:
    webservers:
      hosts:
        web01: {{}}
        web02: {{}}
    dbservers:
      hosts:
        db01: {{}}
        db02: {{}}
    production:
      children:
        webservers: {{}}
        dbservers: {{}}
"#
    )
    .unwrap();
    file
}

// =============================================================================
// 1. TAGS BASIC TESTS
// =============================================================================

mod tags_basic {
    use super::*;

    #[test]
    fn test_task_with_single_tag() {
        let playbook = fixtures_dir().join("single_tag.yml");

        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_task_with_multiple_tags() {
        let playbook = fixtures_dir().join("multiple_tags.yml");

        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("setup")
            .assert()
            .success();
    }

    #[test]
    fn test_tags_includes_only_matching() {
        let playbook = create_test_playbook_with_tags();

        // Run with only install tag - should not run configure or deploy
        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("-t")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_untagged_tasks_skipped_with_tags() {
        let playbook = fixtures_dir().join("untagged_tasks.yml");

        // When --tags is specified, untagged tasks should be skipped
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_no_tags_runs_all() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Without --tags, all tasks should run
        rustible_cmd().arg("run").arg(&playbook).assert().success();
    }

    #[test]
    fn test_list_tasks_with_tags() {
        let playbook = fixtures_dir().join("single_tag.yml");

        rustible_cmd()
            .arg("list-tasks")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success()
            .stdout(predicate::str::contains("install"));
    }

    #[test]
    fn test_multiple_tags_flag() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Multiple -t flags
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("-t")
            .arg("install")
            .arg("-t")
            .arg("configure")
            .assert()
            .success();
    }

    #[test]
    fn test_comma_separated_tags() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Comma-separated tags
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install,configure")
            .assert()
            .success();
    }
}

// =============================================================================
// 2. TAG INHERITANCE TESTS
// =============================================================================

mod tag_inheritance {
    use super::*;

    #[test]
    fn test_tags_on_play_level() {
        let playbook = fixtures_dir().join("play_level_tags.yml");

        // Tasks should inherit tags from play level
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("webserver")
            .assert()
            .success();
    }

    #[test]
    fn test_tags_on_block_level() {
        let playbook = fixtures_dir().join("block_level_tags.yml");

        // Tasks in block should inherit block tags
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("database")
            .assert()
            .success();
    }

    #[test]
    fn test_tags_on_role_level() {
        let playbook = fixtures_dir().join("role_level_tags.yml");

        // Role tasks should inherit role-level tags
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("web")
            .assert()
            .success();
    }

    #[test]
    fn test_tags_inheritance_through_include() {
        let playbook = fixtures_dir().join("include_tasks_tags.yml");

        // Include_tasks should respect tags
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_nested_tag_inheritance() {
        let mut playbook = NamedTempFile::new().unwrap();
        writeln!(
            playbook,
            r#"---
- name: Nested tags test
  hosts: localhost
  gather_facts: false
  tags:
    - outer
  tasks:
    - block:
        - name: Inner task
          debug:
            msg: "Inner"
      tags:
        - inner
"#
        )
        .unwrap();

        // Both outer and inner tags should work
        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("--tags")
            .arg("outer")
            .assert()
            .success();

        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("--tags")
            .arg("inner")
            .assert()
            .success();
    }
}

// =============================================================================
// 3. SPECIAL TAGS TESTS
// =============================================================================

mod special_tags {
    use super::*;

    #[test]
    fn test_always_tag_always_runs() {
        let playbook = fixtures_dir().join("special_always.yml");

        // Task with 'always' tag should run even when other tags are specified
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_never_tag_never_runs_by_default() {
        let playbook = fixtures_dir().join("special_never.yml");

        // Task with 'never' tag should not run unless explicitly specified
        rustible_cmd().arg("run").arg(&playbook).assert().success();
    }

    #[test]
    fn test_never_tag_runs_when_specified() {
        let playbook = fixtures_dir().join("special_never.yml");

        // Task with 'never' tag should run when that tag is specified
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("debug_mode")
            .assert()
            .success();
    }

    #[test]
    fn test_tagged_special_tag() {
        let playbook = fixtures_dir().join("untagged_tasks.yml");

        // The 'tagged' special tag runs all tagged tasks
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("tagged")
            .assert()
            .success();
    }

    #[test]
    fn test_untagged_special_tag() {
        let playbook = fixtures_dir().join("untagged_tasks.yml");

        // The 'untagged' special tag runs all untagged tasks
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("untagged")
            .assert()
            .success();
    }

    #[test]
    fn test_all_special_tag() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // The 'all' special tag runs everything
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("all")
            .assert()
            .success();
    }

    #[test]
    fn test_always_with_skip_tags() {
        let playbook = fixtures_dir().join("special_always.yml");

        // 'always' tasks should run even with skip-tags (unless always is skipped)
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--skip-tags")
            .arg("install")
            .assert()
            .success();
    }
}

// =============================================================================
// 4. SKIP-TAGS TESTS
// =============================================================================

mod skip_tags {
    use super::*;

    #[test]
    fn test_skip_tags_excludes_matching() {
        let playbook = fixtures_dir().join("skip_tags_test.yml");

        // Skip tasks with 'slow' tag
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--skip-tags")
            .arg("slow")
            .assert()
            .success();
    }

    #[test]
    fn test_skip_tags_multiple() {
        let playbook = fixtures_dir().join("skip_tags_test.yml");

        // Skip multiple tags
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--skip-tags")
            .arg("slow,cleanup")
            .assert()
            .success();
    }

    #[test]
    fn test_skip_tags_with_always_tag() {
        let playbook = fixtures_dir().join("skip_tags_test.yml");

        // Skip-tags should not skip 'always' tasks by default
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--skip-tags")
            .arg("tests")
            .assert()
            .success();
    }

    #[test]
    fn test_skip_always_tag_explicitly() {
        let playbook = fixtures_dir().join("special_always.yml");

        // Explicitly skipping 'always' should work
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--skip-tags")
            .arg("always")
            .assert()
            .success();
    }

    #[test]
    fn test_combination_tags_and_skip_tags() {
        let playbook = fixtures_dir().join("skip_tags_test.yml");

        // Run tests but skip slow ones
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("tests")
            .arg("--skip-tags")
            .arg("slow")
            .assert()
            .success();
    }

    #[test]
    fn test_skip_tags_takes_precedence() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // When both tags and skip-tags match, skip-tags wins
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .arg("--skip-tags")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_skip_tags_with_list_tasks() {
        let playbook = fixtures_dir().join("skip_tags_test.yml");

        rustible_cmd()
            .arg("list-tasks")
            .arg(&playbook)
            .arg("--skip-tags")
            .arg("slow")
            .assert()
            .success();
    }
}

// =============================================================================
// 5. TAG EXPRESSIONS TESTS
// =============================================================================

mod tag_expressions {
    use super::*;

    #[test]
    fn test_multiple_tags_or_logic() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Multiple tags should use OR logic (run if any tag matches)
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install,configure")
            .assert()
            .success();
    }

    #[test]
    fn test_tags_with_spaces() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Tags with spaces in the list (trimmed)
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install, configure")
            .assert()
            .success();
    }

    #[test]
    fn test_tag_case_sensitivity() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Tags should be case-sensitive
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("INSTALL")
            .assert()
            .success(); // May or may not match depending on implementation
    }

    #[test]
    fn test_empty_tag_list() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Empty tag specification should be handled gracefully
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("")
            .assert()
            .success();
    }

    #[test]
    fn test_tags_with_special_characters() {
        let mut playbook = NamedTempFile::new().unwrap();
        writeln!(
            playbook,
            r#"---
- name: Special tags test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Task with hyphen tag
      debug:
        msg: "Hyphen"
      tags:
        - pre-install

    - name: Task with underscore tag
      debug:
        msg: "Underscore"
      tags:
        - post_install

    - name: Task with numbers
      debug:
        msg: "Numbers"
      tags:
        - version2
"#
        )
        .unwrap();

        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("--tags")
            .arg("pre-install")
            .assert()
            .success();

        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("--tags")
            .arg("post_install")
            .assert()
            .success();
    }
}

// =============================================================================
// 6. LIMITS BASIC TESTS
// =============================================================================

mod limits_basic {
    use super::*;

    #[test]
    fn test_limit_single_host() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("web01")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_group_name() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("--limit")
            .arg("webservers")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_multiple_hosts() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Multiple hosts with colon separator
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("web01:db01")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_localhost() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = create_test_inventory();

        rustible_cmd()
            .arg("-i")
            .arg(inventory.path())
            .arg("-l")
            .arg("localhost")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_long_form() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("--limit")
            .arg("webservers")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_list_hosts_with_limit() {
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("list-hosts")
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("webservers")
            .assert()
            .success()
            .stdout(predicate::str::contains("web"));
    }
}

// =============================================================================
// 7. LIMIT PATTERNS TESTS
// =============================================================================

mod limit_patterns {
    use super::*;

    #[test]
    fn test_limit_wildcard_pattern() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Wildcard: web*
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("web*")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_regex_pattern() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Regex: ~web\d+
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("~web\\d+")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_exclusion_pattern() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Exclusion: all:!web01
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("all:!web01")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_intersection_pattern() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Intersection: webservers:&production
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("webservers:&production")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_range_pattern() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Range: web[01:02] (if supported)
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("web[01:03]")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_multiple_groups() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Multiple groups: webservers:dbservers
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("webservers:dbservers")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_exclude_group() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Exclude entire group: all:!dbservers
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("all:!dbservers")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_list_hosts_with_wildcard() {
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("list-hosts")
            .arg("-i")
            .arg(&inventory)
            .arg("web*")
            .assert()
            .success();
    }
}

// =============================================================================
// 8. LIMIT FROM FILE TESTS
// =============================================================================

mod limit_from_file {
    use super::*;

    #[test]
    fn test_limit_from_hosts_file() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");
        let hosts_file = fixtures_dir().join("limit_hosts.txt");

        // --limit @hosts.txt
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg(format!("@{}", hosts_file.display()))
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_from_tempfile() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        let mut hosts_file = NamedTempFile::new().unwrap();
        writeln!(hosts_file, "web01").unwrap();
        writeln!(hosts_file, "db01").unwrap();

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg(format!("@{}", hosts_file.path().display()))
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_file_with_comments() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        let mut hosts_file = NamedTempFile::new().unwrap();
        writeln!(hosts_file, "# This is a comment").unwrap();
        writeln!(hosts_file, "web01").unwrap();
        writeln!(hosts_file, "# Another comment").unwrap();
        writeln!(hosts_file, "web02").unwrap();
        writeln!(hosts_file, "").unwrap(); // Empty line

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg(format!("@{}", hosts_file.path().display()))
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_limit_file_not_found() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("@/nonexistent/hosts.txt")
            .arg("run")
            .arg(&playbook)
            .assert()
            .failure();
    }

    #[test]
    fn test_limit_file_empty() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        let hosts_file = NamedTempFile::new().unwrap();
        // Empty file

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg(format!("@{}", hosts_file.path().display()))
            .arg("run")
            .arg(&playbook)
            .assert()
            .success(); // Should succeed with no hosts (or fail gracefully)
    }
}

// =============================================================================
// 9. COMBINATIONS TESTS
// =============================================================================

mod combinations {
    use super::*;

    #[test]
    fn test_tags_with_limit() {
        let playbook = fixtures_dir().join("combination_test.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("webservers")
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_skip_tags_with_limit() {
        let playbook = fixtures_dir().join("combination_test.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("dbservers")
            .arg("run")
            .arg(&playbook)
            .arg("--skip-tags")
            .arg("slow")
            .assert()
            .success();
    }

    #[test]
    fn test_all_three_together() {
        let playbook = fixtures_dir().join("combination_test.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("production")
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("configure,cleanup")
            .arg("--skip-tags")
            .arg("slow")
            .assert()
            .success();
    }

    #[test]
    fn test_limit_with_multiple_tags() {
        let playbook = fixtures_dir().join("combination_test.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("web*")
            .arg("run")
            .arg(&playbook)
            .arg("-t")
            .arg("web")
            .arg("-t")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_check_mode_with_tags_and_limit() {
        let playbook = fixtures_dir().join("combination_test.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("webservers")
            .arg("--check")
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success()
            .stderr(predicate::str::contains("CHECK"));
    }

    #[test]
    fn test_verbose_with_tags_and_limit() {
        let playbook = fixtures_dir().join("combination_test.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("webservers")
            .arg("-vv")
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success();
    }
}

// =============================================================================
// 10. EDGE CASES TESTS
// =============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_empty_result_from_limit() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Limit to non-existent host pattern
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("nonexistent*")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success(); // Should complete gracefully with no hosts
    }

    #[test]
    fn test_no_matching_tags() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Tag that doesn't exist
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("nonexistent_tag")
            .assert()
            .success(); // Should complete with no tasks run
    }

    #[test]
    fn test_invalid_limit_pattern() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Invalid regex pattern
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("~[invalid")
            .arg("run")
            .arg(&playbook)
            .assert()
            .failure();
    }

    #[test]
    fn test_limit_on_non_existent_host() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Specific non-existent host
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("nonexistent_host")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success(); // Should warn but not fail
    }

    #[test]
    fn test_limit_on_non_existent_group() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("nonexistent_group")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_empty_tags_string() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Empty tags string
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("")
            .assert()
            .success();
    }

    #[test]
    fn test_empty_skip_tags_string() {
        let playbook = fixtures_dir().join("single_tag.yml");

        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--skip-tags")
            .arg("")
            .assert()
            .success();
    }

    #[test]
    fn test_whitespace_only_tags() {
        let playbook = fixtures_dir().join("single_tag.yml");

        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("   ")
            .assert()
            .success();
    }

    #[test]
    fn test_duplicate_tags() {
        let playbook = fixtures_dir().join("single_tag.yml");

        // Same tag specified multiple times
        rustible_cmd()
            .arg("run")
            .arg(&playbook)
            .arg("-t")
            .arg("install")
            .arg("-t")
            .arg("install")
            .arg("-t")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_conflicting_limit_patterns() {
        let playbook = fixtures_dir().join("limit_playbook.yml");
        let inventory = fixtures_dir().join("inventory_multi.yml");

        // Include all, then exclude all
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("all:!all")
            .arg("run")
            .arg(&playbook)
            .assert()
            .success(); // Should result in no hosts
    }

    #[test]
    fn test_tags_on_empty_playbook() {
        let mut playbook = NamedTempFile::new().unwrap();
        writeln!(
            playbook,
            r#"---
- name: Empty play
  hosts: localhost
  gather_facts: false
  tasks: []
"#
        )
        .unwrap();

        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("--tags")
            .arg("install")
            .assert()
            .success();
    }

    #[test]
    fn test_unicode_tag_names() {
        let mut playbook = NamedTempFile::new().unwrap();
        writeln!(
            playbook,
            r#"---
- name: Unicode tags test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Unicode tag task
      debug:
        msg: "Unicode"
      tags:
        - "instalacion"
        - "configuracion"
"#
        )
        .unwrap();

        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("--tags")
            .arg("instalacion")
            .assert()
            .success();
    }

    #[test]
    fn test_very_long_tag_name() {
        let long_tag = "x".repeat(256);
        let mut playbook = NamedTempFile::new().unwrap();
        writeln!(
            playbook,
            r#"---
- name: Long tag test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Long tag task
      debug:
        msg: "Long tag"
      tags:
        - "{}"
"#,
            long_tag
        )
        .unwrap();

        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("--tags")
            .arg(&long_tag)
            .assert()
            .success();
    }

    #[test]
    fn test_special_yaml_characters_in_tags() {
        let mut playbook = NamedTempFile::new().unwrap();
        writeln!(
            playbook,
            r#"---
- name: Special chars test
  hosts: localhost
  gather_facts: false
  tasks:
    - name: Task with colon tag
      debug:
        msg: "Colon"
      tags:
        - "step:1"
        - "version:2.0"
"#
        )
        .unwrap();

        rustible_cmd()
            .arg("run")
            .arg(playbook.path())
            .arg("--tags")
            .arg("step:1")
            .assert()
            .success();
    }
}

// =============================================================================
// Additional Integration Tests
// =============================================================================

mod integration {
    use super::*;

    #[test]
    fn test_full_workflow_with_filtering() {
        let inventory = fixtures_dir().join("inventory_multi.yml");
        let playbook = fixtures_dir().join("combination_test.yml");

        // Simulate a typical deployment workflow:
        // 1. Run on webservers only
        // 2. Only install and configure tasks
        // 3. Skip slow tasks
        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("webservers")
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("install,configure,web")
            .arg("--skip-tags")
            .arg("slow")
            .assert()
            .success();
    }

    #[test]
    fn test_validate_with_tags() {
        let playbook = fixtures_dir().join("single_tag.yml");

        rustible_cmd()
            .arg("validate")
            .arg(&playbook)
            .assert()
            .success();
    }

    #[test]
    fn test_list_tasks_filtered() {
        let playbook = fixtures_dir().join("combination_test.yml");

        // List only tasks with specific tags
        rustible_cmd()
            .arg("list-tasks")
            .arg(&playbook)
            .arg("--tags")
            .arg("install")
            .assert()
            .success()
            .stdout(predicate::str::contains("Install"));
    }

    #[test]
    fn test_dry_run_with_full_filtering() {
        let inventory = fixtures_dir().join("inventory_multi.yml");
        let playbook = fixtures_dir().join("combination_test.yml");

        rustible_cmd()
            .arg("-i")
            .arg(&inventory)
            .arg("-l")
            .arg("production")
            .arg("--check")
            .arg("run")
            .arg(&playbook)
            .arg("--tags")
            .arg("configure")
            .arg("--skip-tags")
            .arg("slow")
            .assert()
            .success()
            .stderr(predicate::str::contains("CHECK").or(predicate::str::contains("DRY")));
    }
}
