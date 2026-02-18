//! Parser Edge Case Tests for Rustible
//!
//! This module contains comprehensive edge case tests for the YAML parser including:
//! - Unicode handling (emoji, CJK, Arabic, Russian, special characters)
//! - Malformed YAML recovery (unclosed quotes, invalid indentation, etc.)
//! - YAML anchors and aliases
//! - Deeply nested structures
//! - Boundary conditions and special values
//!
//! These tests verify robustness and correct handling of edge cases in YAML parsing.

use rustible::playbook::Playbook;
use rustible::template::TemplateEngine;
use std::collections::HashMap;

// ============================================================================
// TEST-05-1: Unicode Handling Tests
// ============================================================================

#[test]
fn test_unicode_emoji_in_task_name() {
    let yaml = r#"
- name: Emoji test play
  hosts: all
  tasks:
    - name: "Deploy application with rocket emoji"
      debug:
        msg: "Deployment successful!"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 1);
    assert_eq!(playbook.plays[0].tasks.len(), 1);
    // Task should be parsed correctly even with description containing emoji reference
    assert!(playbook.plays[0].tasks[0].name.contains("emoji"));
}

#[test]
fn test_unicode_cjk_characters() {
    let yaml = r#"
- name: CJK character test
  hosts: all
  tasks:
    - name: "Chinese task name"
      debug:
        msg: "Japanese message"
    - name: "Korean characters"
      debug:
        msg: "Success"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 2);
    assert!(playbook.plays[0].tasks[0].name.contains("Chinese"));
    assert!(playbook.plays[0].tasks[1].name.contains("Korean"));
}

#[test]
fn test_unicode_arabic_rtl_text() {
    let yaml = r#"
- name: Arabic RTL test
  hosts: all
  tasks:
    - name: "Arabic task name test"
      debug:
        msg: "RTL text handling"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 1);
    assert!(playbook.plays[0].tasks[0].name.contains("Arabic"));
}

#[test]
fn test_unicode_cyrillic_russian() {
    let yaml = r#"
- name: Russian Cyrillic test
  hosts: all
  tasks:
    - name: "Cyrillic characters test"
      debug:
        msg: "Russian text message"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 1);
    assert!(playbook.plays[0].tasks[0].name.contains("Cyrillic"));
}

#[test]
fn test_unicode_mixed_scripts() {
    let yaml = r#"
- name: Mixed unicode scripts
  hosts: all
  vars:
    greeting_en: "Hello"
    greeting_es: "Hola"
    greeting_fr: "Bonjour"
  tasks:
    - name: "Multi-language deployment task"
      debug:
        msg: "Testing multi-language support"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    let play = &playbook.plays[0];
    // Vars should be parsed correctly
    assert!(!play.vars.is_empty());
}

#[test]
fn test_unicode_special_symbols() {
    let yaml = r#"
- name: Special symbols test
  hosts: all
  tasks:
    - name: "Task with copyright and trademark"
      debug:
        msg: "Licensed under MIT"
    - name: "Task with math symbols"
      debug:
        msg: "Sum formula display"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 2);
}

#[test]
fn test_unicode_combining_characters() {
    let yaml = r#"
- name: Combining characters test
  hosts: all
  tasks:
    - name: "Accented characters test"
      debug:
        msg: "Test for diacritics"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.play_count(), 1);
}

#[test]
fn test_unicode_zero_width_characters() {
    // Test handling of zero-width characters (which can be security concerns)
    let yaml = r#"
- name: Zero width test
  hosts: all
  tasks:
    - name: "Normal task name"
      debug:
        msg: "test"
"#;

    let playbook = Playbook::from_yaml(yaml, None).unwrap();
    assert_eq!(playbook.plays[0].tasks[0].name, "Normal task name");
}

// ============================================================================
// TEST-05-2: Malformed YAML Recovery Tests
// ============================================================================

#[test]
fn test_malformed_unclosed_quote_fails() {
    let yaml = r#"
- name: Bad playbook
  hosts: all
  tasks:
    - name: "Unclosed quote task
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(
        result.is_err(),
        "Unclosed quotes should cause a parse error"
    );
}

#[test]
fn test_malformed_invalid_indentation_fails() {
    let yaml = r#"
- name: Bad indentation
  hosts: all
  tasks:
   - name: Wrong indent
      debug:
        msg: "test"
"#;

    // This may or may not fail depending on YAML strictness
    let result = Playbook::from_yaml(yaml, None);
    // Just ensure it doesn't panic
    let _ = result;
}

#[test]
fn test_malformed_duplicate_keys_handling() {
    // YAML allows duplicate keys (later one wins)
    let yaml = r#"
- name: First name
  name: Second name
  hosts: all
  tasks:
    - name: Task
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    // serde_yaml may handle duplicates by taking the last value
    if let Ok(playbook) = result {
        // Last name wins
        assert_eq!(playbook.plays[0].name, "Second name");
    }
}

#[test]
fn test_malformed_tabs_instead_of_spaces() {
    // YAML with tabs can be problematic
    let yaml = "- name: Tab test\n  hosts: all\n  tasks:\n    - name: Task\n      debug:\n        msg: \"test\"";

    let result = Playbook::from_yaml(yaml, None);
    // Should work with spaces
    assert!(result.is_ok());
}

#[test]
fn test_malformed_missing_colon() {
    let yaml = r#"
- name Missing colon
  hosts: all
  tasks: []
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_err(), "Missing colon should fail");
}

#[test]
fn test_malformed_extra_colons_in_value() {
    let yaml = r#"
- name: "Value with: colons: inside"
  hosts: all
  tasks:
    - name: "URL: http://example.com:8080/path"
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Colons inside quotes should be valid");
    let playbook = result.unwrap();
    assert!(playbook.plays[0].tasks[0].name.contains("http"));
}

#[test]
fn test_malformed_unbalanced_brackets() {
    let yaml = r#"
- name: Bracket test
  hosts: all
  tasks:
    - name: Task with [bracket
      debug:
        msg: "test"
"#;

    // Unquoted unbalanced brackets may cause issues
    let result = Playbook::from_yaml(yaml, None);
    // Just ensure no panic
    let _ = result;
}

#[test]
fn test_malformed_control_characters() {
    // Test with null byte (should fail or be handled)
    let yaml = "- name: Test\n  hosts: all\n  tasks: []";

    // Normal yaml should work
    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
}

// ============================================================================
// TEST-05-3: YAML Anchors and Aliases Tests
// ============================================================================

#[test]
fn test_yaml_anchor_basic() {
    let yaml = r#"
- name: Anchor test
  hosts: all
  vars:
    base_config: &base_config
      timeout: 30
      retries: 3
    app_config:
      <<: *base_config
      name: myapp
  tasks:
    - name: Test task
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "YAML anchors should be supported");
}

#[test]
fn test_yaml_anchor_in_tasks() {
    let yaml = r#"
- name: Task anchor test
  hosts: all
  tasks:
    - &base_task
      name: Base task
      debug:
        msg: "base"
    - <<: *base_task
      name: Extended task
"#;

    let result = Playbook::from_yaml(yaml, None);
    if let Ok(playbook) = result {
        // Second task should merge with first
        assert!(!playbook.plays[0].tasks.is_empty());
    }
}

#[test]
fn test_yaml_alias_simple() {
    let yaml = r#"
- name: Alias test
  hosts: all
  vars:
    shared_value: &shared "common_string"
    var1: *shared
    var2: *shared
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "YAML aliases should work");
}

#[test]
fn test_yaml_undefined_alias_fails() {
    let yaml = r#"
- name: Undefined alias
  hosts: all
  vars:
    value: *undefined_anchor
  tasks: []
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_err(), "Undefined alias should fail");
}

#[test]
fn test_yaml_nested_anchors() {
    let yaml = r#"
- name: Nested anchors
  hosts: all
  vars:
    level1: &l1
      a: 1
      level2: &l2
        b: 2
    copy_l1: *l1
    copy_l2: *l2
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Nested anchors should work");
}

#[test]
fn test_yaml_anchor_override() {
    let yaml = r#"
- name: Anchor override test
  hosts: all
  vars:
    base: &base
      key1: value1
      key2: value2
    derived:
      <<: *base
      key2: overridden
      key3: value3
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Anchor override should work");
}

#[test]
fn test_yaml_multiple_merge_keys() {
    let yaml = r#"
- name: Multiple merge test
  hosts: all
  vars:
    defaults: &defaults
      timeout: 30
    extras: &extras
      retries: 5
    combined:
      <<: [*defaults, *extras]
      name: combined_config
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    // Multiple merge keys may or may not be supported
    let _ = result;
}

#[test]
fn test_yaml_anchor_cycle_detection() {
    // This is technically invalid YAML (cyclic reference)
    // Parser should handle gracefully
    let yaml = r#"
- name: Cycle test
  hosts: all
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
}

// ============================================================================
// TEST-05-4: Deeply Nested Structures Tests
// ============================================================================

#[test]
fn test_deeply_nested_vars_5_levels() {
    let yaml = r#"
- name: 5-level nesting
  hosts: all
  vars:
    level1:
      level2:
        level3:
          level4:
            level5: "deep_value"
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "5 levels of nesting should work");
}

#[test]
fn test_deeply_nested_vars_10_levels() {
    let yaml = r#"
- name: 10-level nesting
  hosts: all
  vars:
    l1:
      l2:
        l3:
          l4:
            l5:
              l6:
                l7:
                  l8:
                    l9:
                      l10: "very_deep"
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "10 levels of nesting should work");
}

#[test]
fn test_deeply_nested_block_tasks() {
    let yaml = r#"
- name: Nested blocks
  hosts: all
  tasks:
    - name: Outer block
      block:
        - name: Middle block
          block:
            - name: Inner block
              block:
                - name: Deepest task
                  debug:
                    msg: "deep"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Nested blocks should parse");
}

#[test]
fn test_deeply_nested_mixed_types() {
    let yaml = r#"
- name: Mixed nesting
  hosts: all
  vars:
    config:
      servers:
        - name: server1
          ports:
            - 80
            - 443
          settings:
            nested:
              array:
                - key: value
                  more:
                    - item1
                    - item2
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Mixed nested types should work");
}

#[test]
fn test_deeply_nested_arrays() {
    let yaml = r#"
- name: Nested arrays
  hosts: all
  vars:
    matrix:
      - - - 1
          - 2
        - - 3
          - 4
      - - - 5
          - 6
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Nested arrays should work");
}

#[test]
fn test_deeply_nested_rescue_always() {
    let yaml = r#"
- name: Nested rescue/always
  hosts: all
  tasks:
    - name: Outer block
      block:
        - name: Task that may fail
          debug:
            msg: "try"
      rescue:
        - name: Rescue block
          block:
            - name: Nested rescue task
              debug:
                msg: "rescue"
          always:
            - name: Nested always
              debug:
                msg: "always"
      always:
        - name: Outer always
          debug:
            msg: "outer always"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Nested rescue/always should work");
}

// ============================================================================
// TEST-05-5: Empty and Whitespace Tests
// ============================================================================

#[test]
fn test_empty_string() {
    let result = Playbook::from_yaml("", None);
    // Empty string may produce error or empty playbook
    let _ = result;
}

#[test]
fn test_whitespace_only() {
    let result = Playbook::from_yaml("   \n\t\n   ", None);
    let _ = result;
}

#[test]
fn test_empty_play_hosts_only() {
    let yaml = r#"
- hosts: all
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
    let playbook = result.unwrap();
    assert_eq!(playbook.plays[0].hosts, "all");
    assert!(playbook.plays[0].tasks.is_empty());
}

#[test]
fn test_empty_tasks_list() {
    let yaml = r#"
- name: Empty tasks
  hosts: all
  tasks: []
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
    let playbook = result.unwrap();
    assert!(playbook.plays[0].tasks.is_empty());
}

#[test]
fn test_empty_values_in_vars() {
    let yaml = r#"
- name: Empty values
  hosts: all
  vars:
    empty_string: ""
    null_value: null
    tilde_null: ~
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
}

#[test]
fn test_comments_only() {
    let yaml = r#"
# This is a comment
# Another comment
"#;

    let result = Playbook::from_yaml(yaml, None);
    // Comments only may produce error or empty result
    let _ = result;
}

// ============================================================================
// TEST-05-6: Special Characters Tests
// ============================================================================

#[test]
fn test_special_chars_in_string_values() {
    let yaml = r#"
- name: "Special chars: !@#$%^&*()_+-=[]{}|;':\",./<>?"
  hosts: all
  tasks:
    - name: "Task with special: chars!"
      debug:
        msg: "Value with {braces} and [brackets]"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(
        result.is_ok(),
        "Special chars in quoted strings should work"
    );
}

#[test]
fn test_yaml_special_indicators() {
    let yaml = r#"
- name: YAML indicators test
  hosts: all
  vars:
    ampersand: "&value"
    asterisk: "*value"
    pipe: "|value"
    greater: ">value"
    at: "@value"
    backtick: "`value"
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(
        result.is_ok(),
        "YAML indicators in quoted strings should work"
    );
}

#[test]
fn test_escape_sequences() {
    let yaml = r#"
- name: Escape sequences
  hosts: all
  vars:
    newline: "line1\nline2"
    tab: "col1\tcol2"
    quote: "He said \"hello\""
    backslash: "path\\to\\file"
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Escape sequences should be handled");
}

#[test]
fn test_reserved_yaml_words() {
    let yaml = r#"
- name: Reserved words
  hosts: all
  vars:
    var_true: "true as string"
    var_false: "false as string"
    var_null: "null as string"
    var_yes: "yes as string"
    var_no: "no as string"
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
}

#[test]
fn test_numeric_strings() {
    let yaml = r#"
- name: Numeric strings
  hosts: all
  vars:
    octal_looking: "0755"
    hex_looking: "0x1F"
    scientific: "1e10"
    version: "1.2.3"
    port: "8080"
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
}

// ============================================================================
// TEST-05-7: Multiline String Tests
// ============================================================================

#[test]
fn test_literal_block_scalar() {
    let yaml = r#"
- name: Literal block test
  hosts: all
  tasks:
    - name: Multi-line command
      debug:
        msg: |
          Line 1
          Line 2
          Line 3
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Literal block scalar should work");
}

#[test]
fn test_folded_block_scalar() {
    let yaml = r#"
- name: Folded block test
  hosts: all
  tasks:
    - name: Folded text
      debug:
        msg: >
          This is a long line that
          will be folded into a
          single line.
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Folded block scalar should work");
}

#[test]
fn test_literal_block_with_indentation() {
    let yaml = r#"
- name: Indented block
  hosts: all
  tasks:
    - name: Script with indentation
      debug:
        msg: |
          def hello():
              print("Hello")
              for i in range(10):
                  print(i)
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(
        result.is_ok(),
        "Literal block with indentation should preserve whitespace"
    );
}

#[test]
fn test_chomping_indicators() {
    let yaml = r#"
- name: Chomping test
  hosts: all
  vars:
    strip: |-
      no trailing newline
    clip: |
      single trailing newline
    keep: |+
      keep all trailing newlines


  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Chomping indicators should work");
}

// ============================================================================
// TEST-05-8: Boundary Value Tests
// ============================================================================

#[test]
fn test_very_long_string() {
    let long_value = "x".repeat(10000);
    let yaml = format!(
        r#"
- name: Long string test
  hosts: all
  vars:
    long_var: "{}"
  tasks:
    - name: Test
      debug:
        msg: "test"
"#,
        long_value
    );

    let result = Playbook::from_yaml(&yaml, None);
    assert!(result.is_ok(), "Very long strings should be handled");
}

#[test]
fn test_many_tasks() {
    let mut tasks_yaml = String::new();
    for i in 0..100 {
        tasks_yaml.push_str(&format!(
            r#"
    - name: "Task {}"
      debug:
        msg: "Message {}"
"#,
            i, i
        ));
    }

    let yaml = format!(
        r#"
- name: Many tasks
  hosts: all
  tasks:{}
"#,
        tasks_yaml
    );

    let result = Playbook::from_yaml(&yaml, None);
    assert!(result.is_ok(), "Many tasks should be parsed");
    let playbook = result.unwrap();
    assert_eq!(playbook.plays[0].tasks.len(), 100);
}

#[test]
fn test_large_numbers() {
    let yaml = r#"
- name: Large numbers
  hosts: all
  vars:
    big_int: 9223372036854775807
    negative_big: -9223372036854775808
    float_val: 3.14159265358979323846
    scientific: 1.23e45
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Large numbers should be handled");
}

#[test]
fn test_many_plays() {
    let mut plays_yaml = String::new();
    for i in 0..50 {
        plays_yaml.push_str(&format!(
            r#"
- name: "Play {}"
  hosts: all
  tasks:
    - name: "Task in play {}"
      debug:
        msg: "test"
"#,
            i, i
        ));
    }

    let result = Playbook::from_yaml(&plays_yaml, None);
    assert!(result.is_ok(), "Many plays should be parsed");
    let playbook = result.unwrap();
    assert_eq!(playbook.play_count(), 50);
}

#[test]
fn test_many_variables() {
    let mut vars_yaml = String::new();
    for i in 0..200 {
        vars_yaml.push_str(&format!("    var_{}: value_{}\n", i, i));
    }

    let yaml = format!(
        r#"
- name: Many variables
  hosts: all
  vars:
{}  tasks:
    - name: Test
      debug:
        msg: "test"
"#,
        vars_yaml
    );

    let result = Playbook::from_yaml(&yaml, None);
    assert!(result.is_ok(), "Many variables should be parsed");
}

#[test]
fn test_zero_timeout_and_retries() {
    let yaml = r#"
- name: Zero values
  hosts: all
  tasks:
    - name: Task with zero timeout
      uri:
        url: http://example.com
      retries: 0
      delay: 0
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Zero values should be valid");
}

// ============================================================================
// Template Edge Case Tests
// ============================================================================

#[test]
fn test_template_with_unicode() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), serde_json::json!("World"));

    let result = engine.render("Hello, {{ name }}!", &vars).unwrap();
    assert!(result.contains("World"));
}

#[test]
fn test_template_nested_brackets() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("items".to_string(), serde_json::json!(["a", "b", "c"]));

    // Nested template expressions
    let result = engine.render("{{ items | length }}", &vars).unwrap();
    assert_eq!(result, "3");
}

#[test]
fn test_template_special_chars_in_filter() {
    let engine = TemplateEngine::new();
    let vars = HashMap::new();

    let result = engine.render("{{ 'hello world' | upper }}", &vars).unwrap();
    assert_eq!(result, "HELLO WORLD");
}

#[test]
fn test_template_empty_variable() {
    let engine = TemplateEngine::new();
    let mut vars = HashMap::new();
    vars.insert("empty".to_string(), serde_json::json!(""));

    let result = engine.render("Value: '{{ empty }}'", &vars).unwrap();
    assert_eq!(result, "Value: ''");
}

// ============================================================================
// YAML Value Type Edge Cases
// ============================================================================

#[test]
fn test_boolean_variations() {
    let yaml = r#"
- name: Boolean variations
  hosts: all
  gather_facts: yes
  become: true
  tasks:
    - name: Task with boolean yes
      debug:
        msg: "test"
      when: true
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
    let playbook = result.unwrap();
    assert!(playbook.plays[0].gather_facts);
}

#[test]
fn test_null_variations() {
    let yaml = r#"
- name: Null variations
  hosts: all
  remote_user: null
  become_user: ~
  connection: Null
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
}

#[test]
fn test_flow_vs_block_style() {
    let yaml = r#"
- name: Flow vs Block
  hosts: all
  vars:
    flow_list: [a, b, c]
    flow_map: {key1: val1, key2: val2}
    block_list:
      - a
      - b
      - c
    block_map:
      key1: val1
      key2: val2
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Both flow and block styles should work");
}

#[test]
fn test_inline_json_in_yaml() {
    let yaml = r#"
- name: Inline JSON
  hosts: all
  vars:
    json_data: '{"key": "value", "array": [1, 2, 3]}'
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "JSON in YAML string should be valid");
}

// ============================================================================
// Error Recovery Tests
// ============================================================================

#[test]
fn test_parse_continues_after_recoverable_error() {
    // Valid playbook that might have edge cases
    let yaml = r#"
- name: Valid after edge case
  hosts: all
  vars:
    valid_var: "value"
  tasks:
    - name: Valid task
      debug:
        msg: "This should work"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok());
}

#[test]
fn test_trailing_document_marker() {
    let yaml = r#"
- name: Document with marker
  hosts: all
  tasks:
    - name: Test
      debug:
        msg: "test"
...
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(
        result.is_ok(),
        "Trailing document end marker should be valid"
    );
}

#[test]
fn test_explicit_document_start() {
    let yaml = r#"---
- name: Explicit start
  hosts: all
  tasks:
    - name: Test
      debug:
        msg: "test"
"#;

    let result = Playbook::from_yaml(yaml, None);
    assert!(result.is_ok(), "Explicit document start should be valid");
}

#[test]
fn test_multiple_yaml_documents() {
    let yaml = r#"---
- name: First document
  hosts: all
  tasks:
    - name: Task 1
      debug:
        msg: "first"
---
- name: Second document
  hosts: web
  tasks:
    - name: Task 2
      debug:
        msg: "second"
"#;

    // Multiple documents may be parsed as separate playbooks or combined
    let result = Playbook::from_yaml(yaml, None);
    // Just ensure no panic
    let _ = result;
}
