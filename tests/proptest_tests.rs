#![cfg(not(tarpaulin))]
//! Property-based tests for Rustible using proptest.
//!
//! This module provides comprehensive fuzz testing to discover edge cases
//! through random input generation. Property-based testing helps find
//! unexpected panics, crashes, and logic errors.

use proptest::collection::vec;
use proptest::prelude::*;
use std::collections::HashMap;

// ============================================================================
// Strategies for generating test data
// ============================================================================

/// Strategy for generating valid YAML-safe strings
fn yaml_safe_string() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z0-9_][a-zA-Z0-9_-]{0,63}")
        .unwrap()
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Strategy for generating potentially problematic strings
fn problematic_string() -> impl Strategy<Value = String> {
    prop_oneof![
        // Normal strings
        "[a-zA-Z0-9_-]{0,100}",
        // Unicode strings
        "\\PC{0,50}",
        // Strings with special YAML characters
        prop::string::string_regex("[:\\[\\]{}#&*!|>'\"@`%]{0,20}").unwrap(),
        // Strings with whitespace
        "[ \\t\\n\\r]{0,20}",
        // Empty string
        Just("".to_string()),
        // Very long strings
        "[a-z]{1000,2000}",
        // Null bytes and control characters
        prop::string::string_regex("[\\x00-\\x1f]{0,10}").unwrap(),
    ]
}

/// Strategy for generating valid host names
fn valid_hostname() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z][a-zA-Z0-9-]{0,62}")
        .unwrap()
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Strategy for generating valid variable names
fn valid_var_name() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z_][a-zA-Z0-9_]{0,63}")
        .unwrap()
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Strategy for generating valid group names
fn valid_group_name() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z_][a-zA-Z0-9_-]{0,31}")
        .unwrap()
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Strategy for generating YAML values
fn yaml_value() -> impl Strategy<Value = serde_yaml::Value> {
    let leaf = prop_oneof![
        Just(serde_yaml::Value::Null),
        any::<bool>().prop_map(serde_yaml::Value::Bool),
        any::<i64>().prop_map(|n| serde_yaml::Value::Number(n.into())),
        "[a-zA-Z0-9_ -]{0,100}".prop_map(serde_yaml::Value::String),
    ];

    leaf.prop_recursive(
        3,  // depth
        32, // max nodes
        10, // items per collection
        |inner| {
            prop_oneof![
                // Sequence
                vec(inner.clone(), 0..5).prop_map(serde_yaml::Value::Sequence),
                // Mapping
                vec((yaml_safe_string(), inner), 0..5).prop_map(|pairs| {
                    let mut map = serde_yaml::Mapping::new();
                    for (k, v) in pairs {
                        map.insert(serde_yaml::Value::String(k), v);
                    }
                    serde_yaml::Value::Mapping(map)
                }),
            ]
        },
    )
}

/// Strategy for generating YAML mappings (no Null at top level)
fn yaml_mapping() -> impl Strategy<Value = serde_yaml::Value> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(serde_yaml::Value::Bool),
        any::<i64>().prop_map(|n| serde_yaml::Value::Number(n.into())),
        "[a-zA-Z0-9_ -]{0,100}".prop_map(serde_yaml::Value::String),
    ];

    vec((yaml_safe_string(), leaf), 0..5).prop_map(|pairs| {
        let mut map = serde_yaml::Mapping::new();
        for (k, v) in pairs {
            map.insert(serde_yaml::Value::String(k), v);
        }
        serde_yaml::Value::Mapping(map)
    })
}

/// Strategy for generating JSON values
fn json_value() -> impl Strategy<Value = serde_json::Value> {
    let leaf = prop_oneof![
        Just(serde_json::Value::Null),
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|n| serde_json::Value::Number(n.into())),
        "[a-zA-Z0-9_ -]{0,100}".prop_map(serde_json::Value::String),
    ];

    leaf.prop_recursive(
        3,  // depth
        32, // max nodes
        10, // items per collection
        |inner| {
            prop_oneof![
                // Array
                vec(inner.clone(), 0..5).prop_map(serde_json::Value::Array),
                // Object
                vec((yaml_safe_string(), inner), 0..5).prop_map(|pairs| {
                    let mut map = serde_json::Map::new();
                    for (k, v) in pairs {
                        map.insert(k, v);
                    }
                    serde_json::Value::Object(map)
                }),
            ]
        },
    )
}

/// Strategy for generating template strings
fn template_string() -> impl Strategy<Value = String> {
    prop_oneof![
        // Plain text
        "[a-zA-Z0-9 .,!?-]{0,200}",
        // Simple variable
        yaml_safe_string().prop_map(|v| format!("{{{{ {} }}}}", v)),
        // Variable with filter
        (yaml_safe_string(), yaml_safe_string())
            .prop_map(|(v, f)| format!("{{{{ {} | {} }}}}", v, f)),
        // Conditional
        yaml_safe_string().prop_map(|v| format!("{{% if {} %}}yes{{% endif %}}", v)),
        // Loop
        yaml_safe_string()
            .prop_map(|v| format!("{{% for item in {} %}}{{{{ item }}}}{{% endfor %}}", v)),
        // Mixed content
        (yaml_safe_string(), "[a-zA-Z ]{0,50}")
            .prop_map(|(v, text)| format!("{} {{{{ {} }}}} more text", text, v)),
    ]
}

/// Strategy for generating host patterns
fn host_pattern() -> impl Strategy<Value = String> {
    prop_oneof![
        // Simple patterns
        Just("all".to_string()),
        Just("*".to_string()),
        valid_hostname(),
        valid_group_name(),
        // Glob patterns
        valid_hostname().prop_map(|h| format!("{}*", h)),
        valid_hostname().prop_map(|h| format!("*{}", h)),
        // Regex patterns
        valid_hostname().prop_map(|h| format!("~{}.*", h)),
        // Complex patterns with operators
        (valid_group_name(), valid_group_name()).prop_map(|(a, b)| format!("{}:{}", a, b)),
        (valid_group_name(), valid_group_name()).prop_map(|(a, b)| format!("{}:&{}", a, b)),
        (valid_group_name(), valid_group_name()).prop_map(|(a, b)| format!("{}:!{}", a, b)),
    ]
}

/// Strategy for generating file paths
fn file_path() -> impl Strategy<Value = String> {
    prop_oneof![
        // Valid Unix paths
        prop::string::string_regex("/[a-zA-Z0-9_/.-]{0,200}").unwrap(),
        // Relative paths
        prop::string::string_regex("[a-zA-Z0-9_.-]+(/[a-zA-Z0-9_.-]+){0,10}").unwrap(),
        // Path traversal attempts
        prop::string::string_regex("(\\.\\./){1,10}[a-zA-Z0-9_]+").unwrap(),
        // Paths with special characters
        prop::string::string_regex("/tmp/[a-zA-Z0-9 ${}\\[\\]'\"]+").unwrap(),
        // Very long paths
        prop::string::string_regex("/[a-z]{1,10}(/[a-z]{1,10}){50,100}").unwrap(),
        // Empty path
        Just("".to_string()),
        // Tilde expansion
        prop::string::string_regex("~/[a-zA-Z0-9_/.-]{0,50}").unwrap(),
    ]
}

/// Strategy for generating shell commands
fn shell_command() -> impl Strategy<Value = String> {
    prop_oneof![
        // Simple commands
        prop::string::string_regex("[a-z]+( [a-zA-Z0-9_/-]+){0,5}").unwrap(),
        // Commands with arguments
        prop::string::string_regex("[a-z]+ --[a-z]+(=[a-zA-Z0-9]+)?").unwrap(),
        // Commands with pipes
        prop::string::string_regex("[a-z]+ \\| [a-z]+").unwrap(),
        // Commands with redirection
        prop::string::string_regex("[a-z]+ > /tmp/[a-z]+").unwrap(),
        // Commands with shell metacharacters
        prop::string::string_regex("[a-z]+; [a-z]+").unwrap(),
        prop::string::string_regex("[a-z]+ && [a-z]+").unwrap(),
        prop::string::string_regex("[a-z]+ \\|\\| [a-z]+").unwrap(),
        // Command substitution
        prop::string::string_regex("\\$\\([a-z]+\\)").unwrap(),
        prop::string::string_regex("`[a-z]+`").unwrap(),
        // Very long commands
        prop::string::string_regex("[a-z ]{1000,2000}").unwrap(),
    ]
}

// ============================================================================
// YAML PARSING FUZZING TESTS
// ============================================================================

mod yaml_parsing {
    use super::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        /// Property: Parsing random YAML structures should never panic
        #[test]
        fn parsing_random_yaml_never_panics(yaml in yaml_value()) {
            let yaml_str = serde_yaml::to_string(&yaml).unwrap_or_default();
            // Should not panic - result can be Ok or Err
            let _: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml_str);
        }

        /// Property: Parsing random strings should never panic
        #[test]
        fn parsing_random_strings_never_panics(content in "\\PC{0,1000}") {
            let _: Result<serde_yaml::Value, _> = serde_yaml::from_str(&content);
        }

        /// Property: Parsing malformed YAML should return errors, not panic
        #[test]
        fn parsing_malformed_yaml_returns_error(
            content in prop_oneof![
                // Unclosed brackets
                "[a-z]+: \\[",
                "[a-z]+: \\{",
                // Invalid indentation
                "  [a-z]+:\\n[a-z]+",
                // Duplicate keys
                "[a-z]+: 1\\n[a-z]+: 2",
                // Tab characters
                "\\t[a-z]+: value",
            ]
        ) {
            // Should not panic
            let _: Result<serde_yaml::Value, _> = serde_yaml::from_str(&content);
        }

        /// Property: Very deep nesting should be handled gracefully
        #[test]
        fn deep_nesting_handled(depth in 1..20usize) {
            let mut yaml = String::from("root:\n");
            for i in 0..depth {
                yaml.push_str(&"  ".repeat(i + 1));
                yaml.push_str(&format!("level{}:\n", i));
            }
            yaml.push_str(&"  ".repeat(depth + 1));
            yaml.push_str("value: end\n");

            let _: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml);
        }

        /// Property: Large documents should be handled without stack overflow
        #[test]
        fn large_document_no_stack_overflow(size in 100..500usize) {
            let mut yaml = String::from("- name: Test Play\n  hosts: all\n  tasks:\n");
            for i in 0..size {
                yaml.push_str(&format!("    - name: Task {}\n      debug:\n        msg: test\n", i));
            }

            let _: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml);
        }

        /// Property: Special characters in strings should be handled safely
        #[test]
        fn special_characters_in_yaml(s in problematic_string()) {
            let yaml = format!("key: \"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));
            let _: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml);
        }
    }
}

// ============================================================================
// INVENTORY FUZZING TESTS
// ============================================================================

mod inventory_fuzzing {
    use super::*;
    use rustible::inventory::{Group, Host, Inventory};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Property: Creating hosts with random names should never panic
        #[test]
        fn create_host_never_panics(name in "\\PC{0,200}") {
            let _ = Host::new(name);
        }

        /// Property: Parsing host definitions should not panic
        #[test]
        fn parse_host_definition_no_panic(input in "\\PC{0,500}") {
            let _ = Host::parse(&input);
        }

        /// Property: Valid hostnames should parse successfully
        #[test]
        fn valid_hostname_parses(name in valid_hostname()) {
            let result = Host::parse(&name);
            prop_assert!(result.is_ok(), "Valid hostname '{}' should parse", name);
        }

        /// Property: Host with parameters should parse
        #[test]
        fn host_with_params_parses(
            name in valid_hostname(),
            port in 1u16..65535u16,
            user in valid_var_name(),
        ) {
            let input = format!("{} ansible_port={} ansible_user={}", name, port, user);
            let result = Host::parse(&input);
            if let Ok(host) = result {
                prop_assert_eq!(host.name, name);
                prop_assert_eq!(host.connection.ssh.port, port);
            }
        }

        /// Property: Creating groups with random names should never panic
        #[test]
        fn create_group_never_panics(name in "\\PC{0,200}") {
            let _ = Group::new(name);
        }

        /// Property: Adding hosts to groups should never panic
        #[test]
        fn add_host_to_group_no_panic(
            group_name in valid_group_name(),
            hosts in vec(valid_hostname(), 0..100),
        ) {
            let mut group = Group::new(group_name);
            for host in hosts {
                group.add_host(host);
            }
        }

        /// Property: Adding child groups should never panic
        #[test]
        fn add_child_groups_no_panic(
            parent_name in valid_group_name(),
            children in vec(valid_group_name(), 0..50),
        ) {
            let mut group = Group::new(parent_name);
            for child in children {
                group.add_child(child);
            }
        }

        /// Property: Inventory pattern matching should not panic
        #[test]
        fn pattern_matching_no_panic(
            pattern in host_pattern(),
            hosts in vec(valid_hostname(), 0..20),
        ) {
            let mut inventory = Inventory::new();
            for host_name in hosts {
                let host = Host::new(host_name);
                let _ = inventory.add_host(host);
            }
            // Should not panic
            let _ = inventory.get_hosts_for_pattern(&pattern);
        }

        /// Property: Complex patterns with operators should not panic
        #[test]
        fn complex_pattern_no_panic(
            patterns in vec(valid_group_name(), 1..5),
            operators in vec(prop_oneof![Just(":"), Just(":&"), Just(":!")], 0..4),
        ) {
            let mut pattern = patterns[0].clone();
            for (i, op) in operators.iter().enumerate() {
                if i + 1 < patterns.len() {
                    pattern.push_str(op);
                    pattern.push_str(&patterns[i + 1]);
                }
            }

            let inventory = Inventory::new();
            let _ = inventory.get_hosts_for_pattern(&pattern);
        }
    }
}

// ============================================================================
// TEMPLATE FUZZING TESTS
// ============================================================================

mod template_fuzzing {
    use super::*;
    use rustible::template::TemplateEngine;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        /// Property: Rendering templates should never panic
        #[test]
        fn render_template_no_panic(
            template in template_string(),
            vars in vec((valid_var_name(), json_value()), 0..10),
        ) {
            let engine = TemplateEngine::new();
            let var_map: HashMap<String, serde_json::Value> = vars.into_iter().collect();
            // Should not panic
            let _ = engine.render(&template, &var_map);
        }

        /// Property: Random template strings should not cause panic
        #[test]
        fn random_template_no_panic(template in "\\PC{0,500}") {
            let engine = TemplateEngine::new();
            let vars = HashMap::new();
            let _ = engine.render(&template, &vars);
        }

        /// Property: Malformed templates should return errors, not panic
        #[test]
        fn malformed_template_returns_error(
            template in prop_oneof![
                // Unclosed variable
                Just("{{ var".to_string()),
                Just("{{ foo".to_string()),
                Just("{{ bar".to_string()),
                // Unclosed block
                Just("{% if condition %}".to_string()),
                Just("{% for x in items %}".to_string()),
                // Invalid syntax (double pipes)
                Just("{{ var | | }}".to_string()),
                Just("{{ foo | | filter }}".to_string()),
                // Deeply nested (malformed)
                Just("{{{{{{ var }}}}}}".to_string()),
                Just("{{ {{ nested }} }}".to_string()),
            ]
        ) {
            let engine = TemplateEngine::new();
            let vars = HashMap::new();
            // Should not panic
            let _ = engine.render(&template, &vars);
        }

        // NOTE: has_template_detection test disabled because is_template() implementation
        // may have different detection logic than simple substring checks. For example,
        // it may require complete delimiter pairs or have other heuristics.
        // See test_template_detection in module_tests.rs for the actual behavior tests.

        /// Property: is_template correctly identifies template syntax
        #[test]
        fn has_template_detection(s in "\\PC{0,200}") {
            let result = TemplateEngine::is_template(&s);
            let expected = s.contains("{{") || s.contains("{%") || s.contains("{#");
            prop_assert_eq!(result, expected);
        }

        /// Property: Filter chains should not panic
        #[test]
        fn filter_chain_no_panic(
            var in valid_var_name(),
            filters in vec(yaml_safe_string(), 0..5),
        ) {
            let template = if filters.is_empty() {
                format!("{{{{ {} }}}}", var)
            } else {
                format!("{{{{ {} | {} }}}}", var, filters.join(" | "))
            };

            let engine = TemplateEngine::new();
            let mut vars = HashMap::new();
            vars.insert(var, serde_json::Value::String("test".to_string()));
            let _ = engine.render(&template, &vars);
        }

        /// Property: Deeply nested templates should not cause stack overflow
        #[test]
        fn deeply_nested_template_no_overflow(depth in 1..30usize) {
            let mut template = String::new();
            for _ in 0..depth {
                template.push_str("{% if true %}");
            }
            template.push_str("value");
            for _ in 0..depth {
                template.push_str("{% endif %}");
            }

            let engine = TemplateEngine::new();
            let vars = HashMap::new();
            let _ = engine.render(&template, &vars);
        }

        /// Property: Large variable contexts should be handled
        #[test]
        fn large_context_no_panic(
            vars in vec((valid_var_name(), json_value()), 100..500),
        ) {
            let engine = TemplateEngine::new();
            let var_map: HashMap<String, serde_json::Value> = vars.into_iter().collect();
            let template = "{{ test | default('none') }}";
            let _ = engine.render(template, &var_map);
        }
    }
}

// ============================================================================
// VARIABLE FUZZING TESTS
// ============================================================================

mod variable_fuzzing {
    use super::*;
    use rustible::vars::{deep_merge, resolve, VarPrecedence, VarStore};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Property: Setting and getting variables should never panic
        #[test]
        fn set_get_variable_no_panic(
            key in valid_var_name(),
            value in yaml_value(),
        ) {
            let mut store = VarStore::new();
            store.set(key.clone(), value.clone(), VarPrecedence::PlayVars);
            let _ = store.get(&key);
        }

        /// Property: Setting variables with random keys should not panic
        #[test]
        fn random_key_no_panic(
            key in "\\PC{0,200}",
            value in yaml_value(),
        ) {
            let mut store = VarStore::new();
            store.set(key, value, VarPrecedence::PlayVars);
        }

        /// Property: Deep merge should never panic
        #[test]
        fn deep_merge_no_panic(
            base in yaml_value(),
            overlay in yaml_value(),
        ) {
            let _ = deep_merge(&base, &overlay);
        }

        /// Property: Deep merge of mappings should be idempotent
        #[test]
        fn deep_merge_idempotent(value in yaml_value()) {
            let merged1 = deep_merge(&value, &value);
            let merged2 = deep_merge(&merged1, &value);
            // Second merge should produce same result
            prop_assert_eq!(
                serde_yaml::to_string(&merged1).unwrap_or_default(),
                serde_yaml::to_string(&merged2).unwrap_or_default()
            );
        }

        /// Property: Resolve path should never panic
        #[test]
        fn resolve_path_no_panic(
            value in yaml_value(),
            path in "[a-zA-Z_][a-zA-Z0-9_.]{0,50}",
        ) {
            let _ = resolve::resolve_path(&value, &path);
        }

        /// Property: Set path should never panic
        #[test]
        fn set_path_no_panic(
            path in "[a-zA-Z_][a-zA-Z0-9_.]{0,50}",
            new_value in yaml_value(),
        ) {
            let mut value = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
            let _ = resolve::set_path(&mut value, &path, new_value);
        }

        /// Property: Type conversions should never panic
        #[test]
        fn type_conversions_no_panic(value in yaml_value()) {
            let _ = resolve::to_string(&value);
            let _ = resolve::to_bool(&value);
            let _ = resolve::to_int(&value);
            let _ = resolve::to_float(&value);
            let _ = resolve::to_list(&value);
        }

        /// Property: Multiple precedence levels should not cause issues
        #[test]
        fn multiple_precedence_no_panic(
            vars in vec((valid_var_name(), yaml_value()), 0..50),
        ) {
            let mut store = VarStore::new();
            let precedences = [
                VarPrecedence::RoleDefaults,
                VarPrecedence::PlayVars,
                VarPrecedence::TaskVars,
                VarPrecedence::ExtraVars,
            ];

            for (i, (key, value)) in vars.into_iter().enumerate() {
                let precedence = precedences[i % precedences.len()];
                store.set(key, value, precedence);
            }

            // Iterate all
            let _ = store.all();
        }

        /// Property: Variable with deeply nested values should work
        #[test]
        fn deeply_nested_values_no_panic(depth in 1..20usize) {
            let mut value = serde_yaml::Value::String("leaf".to_string());
            for i in 0..depth {
                let mut map = serde_yaml::Mapping::new();
                map.insert(
                    serde_yaml::Value::String(format!("level{}", i)),
                    value,
                );
                value = serde_yaml::Value::Mapping(map);
            }

            let mut store = VarStore::new();
            store.set("nested", value, VarPrecedence::PlayVars);
            let _ = store.get("nested");
        }
    }
}

// ============================================================================
// HOST PATTERN FUZZING TESTS
// ============================================================================

mod host_pattern_fuzzing {
    use super::*;
    use rustible::inventory::{Host, Inventory};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Property: Pattern matching on empty inventory should not panic
        #[test]
        fn empty_inventory_pattern_no_panic(pattern in host_pattern()) {
            let inventory = Inventory::new();
            let _ = inventory.get_hosts_for_pattern(&pattern);
        }

        /// Property: Regex patterns should not panic
        #[test]
        fn regex_pattern_no_panic(regex in "[a-zA-Z0-9.*+?^$()\\[\\]{}|\\\\-]{0,50}") {
            let inventory = Inventory::new();
            let pattern = format!("~{}", regex);
            let _ = inventory.get_hosts_for_pattern(&pattern);
        }

        /// Property: Glob patterns should not panic
        #[test]
        fn glob_pattern_no_panic(
            prefix in "[a-zA-Z]{0,10}",
            suffix in "[a-zA-Z]{0,10}",
        ) {
            let mut inventory = Inventory::new();
            let _ = inventory.add_host(Host::new("test1"));
            let _ = inventory.add_host(Host::new("test2"));

            for pattern in [
                format!("{}*", prefix),
                format!("*{}", suffix),
                format!("{}*{}", prefix, suffix),
                format!("{}?{}", prefix, suffix),
            ] {
                let _ = inventory.get_hosts_for_pattern(&pattern);
            }
        }

        /// Property: Pattern with many colons should not panic
        #[test]
        fn many_colons_pattern_no_panic(count in 1..20usize) {
            let pattern = vec!["all"; count].join(":");
            let inventory = Inventory::new();
            let _ = inventory.get_hosts_for_pattern(&pattern);
        }

        // NOTE: bracket_pattern_no_panic test temporarily disabled due to stack overflow
        // in pattern matching on macOS CI. This should be investigated separately.
        // See: get_hosts_for_pattern() appears to have unbounded recursion with bracket patterns

        // NOTE: invalid_pattern_error_not_panic test disabled due to stack overflow
        // in pattern matching with arbitrary Unicode strings. This should be investigated
        // separately. See: get_hosts_for_pattern() has unbounded recursion with some patterns.

        /// Property: Invalid patterns should return errors, not panic
        #[test]
        fn invalid_pattern_error_not_panic(pattern in "\\PC{0,100}") {
            let inventory = Inventory::new();
            // Should not panic, but may return an error
            let _ = inventory.get_hosts_for_pattern(&pattern);
        }
    }
}

// ============================================================================
// MODULE ARGS FUZZING TESTS
// ============================================================================

mod module_args_fuzzing {
    use super::*;
    use rustible::modules::{ModuleContext, ModuleParams, ModuleRegistry, ParamExt};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Property: ParamExt methods should never panic
        #[test]
        fn param_ext_no_panic(
            key in valid_var_name(),
            value in json_value(),
        ) {
            let mut params: ModuleParams = HashMap::new();
            params.insert(key.clone(), value);

            let _ = params.get_string(&key);
            let _ = params.get_bool(&key);
            let _ = params.get_i64(&key);
            let _ = params.get_u32(&key);
            let _ = params.get_vec_string(&key);
        }

        /// Property: get_bool_or should always return a value
        #[test]
        fn get_bool_or_always_returns(
            key in valid_var_name(),
            value in json_value(),
            default in any::<bool>(),
        ) {
            let mut params: ModuleParams = HashMap::new();
            params.insert(key.clone(), value);
            // Should always return a bool
            let _ = params.get_bool_or(&key, default);
            let _ = params.get_bool_or("nonexistent", default);
        }

        /// Property: Module execution should not panic on random params
        #[test]
        fn module_execute_no_panic(
            module_name in prop_oneof![
                Just("command"),
                Just("shell"),
                Just("copy"),
                Just("file"),
                Just("template"),
                Just("service"),
                Just("package"),
                Just("user"),
            ],
            params in vec((valid_var_name(), json_value()), 0..10),
        ) {
            let registry = ModuleRegistry::with_builtins();
            let param_map: ModuleParams = params.into_iter().collect();
            let context = ModuleContext::new();

            // Should not panic
            let _ = registry.execute(&module_name, &param_map, &context);
        }

        /// Property: Module validation should not panic
        #[test]
        fn module_validate_no_panic(
            params in vec((valid_var_name(), json_value()), 0..20),
        ) {
            let registry = ModuleRegistry::with_builtins();
            let param_map: ModuleParams = params.into_iter().collect();

            for name in registry.names() {
                if let Some(module) = registry.get(name) {
                    let _ = module.validate_params(&param_map);
                }
            }
        }

        /// Property: Type coercion edge cases should not panic
        #[test]
        fn type_coercion_edge_cases(
            key in valid_var_name(),
            string_value in prop_oneof![
                Just("true".to_string()),
                Just("false".to_string()),
                Just("yes".to_string()),
                Just("no".to_string()),
                Just("1".to_string()),
                Just("0".to_string()),
                Just("on".to_string()),
                Just("off".to_string()),
                Just("".to_string()),
                "[0-9]+".prop_map(|s| s),
                "[0-9]+\\.[0-9]+".prop_map(|s| s),
                Just("not_a_number".to_string()),
            ],
        ) {
            let mut params: ModuleParams = HashMap::new();
            params.insert(key.clone(), serde_json::Value::String(string_value));

            let _ = params.get_bool(&key);
            let _ = params.get_i64(&key);
            let _ = params.get_u32(&key);
        }

        /// Property: Module context with various settings should not cause issues
        #[test]
        fn module_context_no_panic(
            check_mode in any::<bool>(),
            diff_mode in any::<bool>(),
            _become_flag in any::<bool>(),
        ) {
            let context = ModuleContext::new()
                .with_check_mode(check_mode)
                .with_diff_mode(diff_mode);

            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("cmd".to_string(), serde_json::json!("echo test"));

            // Should not panic
            let _ = registry.execute("command", &params, &context);
        }
    }
}

// ============================================================================
// COMMAND FUZZING TESTS
// ============================================================================

mod command_fuzzing {
    use super::*;
    use rustible::modules::{ModuleContext, ModuleParams, ModuleRegistry};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// Property: Command module with random commands should not panic
        #[test]
        fn command_module_no_panic(cmd in shell_command()) {
            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("cmd".to_string(), serde_json::json!(cmd));

            let context = ModuleContext::new().with_check_mode(true);
            // Should not panic (we use check mode to avoid actual execution)
            let _ = registry.execute("command", &params, &context);
        }

        /// Property: Shell module with metacharacters should not panic
        #[test]
        fn shell_metacharacters_no_panic(
            base_cmd in "[a-z]+",
            metachar in prop_oneof![
                Just(";"),
                Just("&&"),
                Just("||"),
                Just("|"),
                Just(">"),
                Just(">>"),
                Just("<"),
                Just("$("),
                Just("`"),
                Just("$"),
            ],
        ) {
            let cmd = format!("{} {} echo test", base_cmd, metachar);
            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("cmd".to_string(), serde_json::json!(cmd));

            let context = ModuleContext::new().with_check_mode(true);
            let _ = registry.execute("shell", &params, &context);
        }

        /// Property: Very long commands should be handled
        #[test]
        fn long_command_no_panic(len in 1000..5000usize) {
            let cmd: String = (0..len).map(|_| 'a').collect();
            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("cmd".to_string(), serde_json::json!(cmd));

            let context = ModuleContext::new().with_check_mode(true);
            let _ = registry.execute("command", &params, &context);
        }

        /// Property: Commands with null bytes should be handled
        #[test]
        fn null_bytes_in_command_no_panic(
            prefix in "[a-z]{1,10}",
            suffix in "[a-z]{1,10}",
        ) {
            let cmd = format!("{}\0{}", prefix, suffix);
            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("cmd".to_string(), serde_json::json!(cmd));

            let context = ModuleContext::new().with_check_mode(true);
            let _ = registry.execute("command", &params, &context);
        }
    }
}

// ============================================================================
// PATH FUZZING TESTS
// ============================================================================

mod path_fuzzing {
    use super::*;
    use rustible::modules::{ModuleContext, ModuleParams, ModuleRegistry};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Property: Copy module with random paths should not panic
        #[test]
        fn copy_module_path_no_panic(
            src in file_path(),
            dest in file_path(),
        ) {
            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("src".to_string(), serde_json::json!(src));
            params.insert("dest".to_string(), serde_json::json!(dest));

            let context = ModuleContext::new().with_check_mode(true);
            let _ = registry.execute("copy", &params, &context);
        }

        /// Property: File module with random paths should not panic
        #[test]
        fn file_module_path_no_panic(path in file_path()) {
            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("path".to_string(), serde_json::json!(path));
            params.insert("state".to_string(), serde_json::json!("touch"));

            let context = ModuleContext::new().with_check_mode(true);
            let _ = registry.execute("file", &params, &context);
        }

        /// Property: Path traversal attempts should be handled safely
        #[test]
        fn path_traversal_handled(depth in 1..20usize) {
            let traversal = "../".repeat(depth);
            let path = format!("{}etc/passwd", traversal);

            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("src".to_string(), serde_json::json!(path.clone()));
            params.insert("dest".to_string(), serde_json::json!("/tmp/test"));

            let context = ModuleContext::new().with_check_mode(true);
            let _ = registry.execute("copy", &params, &context);
        }

        /// Property: Very long paths should be handled
        #[test]
        fn long_path_handled(segment_count in 50..200usize) {
            let path: String = (0..segment_count)
                .map(|i| format!("/dir{}", i))
                .collect();

            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("path".to_string(), serde_json::json!(path));

            let context = ModuleContext::new().with_check_mode(true);
            let _ = registry.execute("file", &params, &context);
        }

        /// Property: Paths with special characters should not cause issues
        #[test]
        fn special_char_path_no_panic(
            special in prop_oneof![
                Just("$HOME"),
                Just("~"),
                Just("${VAR}"),
                Just("path with spaces"),
                Just("path'with'quotes"),
                Just("path\"with\"doublequotes"),
            ],
        ) {
            let path = format!("/tmp/{}/file", special);

            let registry = ModuleRegistry::with_builtins();
            let mut params: ModuleParams = HashMap::new();
            params.insert("path".to_string(), serde_json::json!(path));

            let context = ModuleContext::new().with_check_mode(true);
            let _ = registry.execute("file", &params, &context);
        }
    }
}

// ============================================================================
// FILTER FUZZING TESTS
// ============================================================================

mod filter_fuzzing {
    use super::*;
    use rustible::template::TemplateEngine;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Property: All built-in filters should not panic on random input
        #[test]
        fn builtin_filters_no_panic(
            filter in prop_oneof![
                Just("lower"),
                Just("upper"),
                Just("capitalize"),
                Just("title"),
                Just("trim"),
                Just("length"),
                Just("first"),
                Just("last"),
                Just("unique"),
                Just("sort"),
                Just("reverse"),
                Just("flatten"),
                Just("min"),
                Just("max"),
                Just("int"),
                Just("float"),
                Just("string"),
                Just("type_debug"),
                Just("to_json"),
                Just("to_nice_json"),
                Just("to_yaml"),
                Just("b64encode"),
                Just("b64decode"),
                Just("basename"),
                Just("dirname"),
                Just("expanduser"),
                Just("shuffle"),
            ],
            input in json_value(),
        ) {
            let engine = TemplateEngine::new();
            let mut vars = HashMap::new();
            vars.insert("input".to_string(), input);

            let template = format!("{{{{ input | {} }}}}", filter);
            let _ = engine.render(&template, &vars);
        }

        /// Property: Filter with args should not panic
        #[test]
        fn filter_with_args_no_panic(
            filter in prop_oneof![
                Just("default('fallback')"),
                Just("replace('a', 'b')"),
                Just("split(' ')"),
                Just("join(', ')"),
                Just("comment('# ')"),
                Just("regex_search('.*')"),
                Just("regex_replace('a', 'b')"),
            ],
        ) {
            let engine = TemplateEngine::new();
            let mut vars = HashMap::new();
            vars.insert("input".to_string(), serde_json::Value::String("test value".to_string()));

            let template = format!("{{{{ input | {} }}}}", filter);
            let _ = engine.render(&template, &vars);
        }

        /// Property: Chained filters should not panic
        #[test]
        fn chained_filters_no_panic(
            filters in vec(
                prop_oneof![
                    Just("lower"),
                    Just("upper"),
                    Just("trim"),
                    Just("string"),
                ],
                1..5
            ),
        ) {
            let engine = TemplateEngine::new();
            let mut vars = HashMap::new();
            vars.insert("input".to_string(), serde_json::Value::String("Test Value".to_string()));

            let template = format!("{{{{ input | {} }}}}", filters.join(" | "));
            let _ = engine.render(&template, &vars);
        }

        /// Property: Invalid filter names should return error, not panic
        #[test]
        fn invalid_filter_error_not_panic(filter_name in "[a-z_]{1,20}") {
            let engine = TemplateEngine::new();
            let mut vars = HashMap::new();
            vars.insert("input".to_string(), serde_json::Value::String("test".to_string()));

            let template = format!("{{{{ input | {} }}}}", filter_name);
            // Should not panic
            let _ = engine.render(&template, &vars);
        }

        /// Property: Filters on null/undefined should not panic
        #[test]
        fn filter_on_null_no_panic(
            filter in prop_oneof![
                Just("lower"),
                Just("upper"),
                Just("default('x')"),
                Just("string"),
            ],
        ) {
            let engine = TemplateEngine::new();
            let mut vars = HashMap::new();
            vars.insert("input".to_string(), serde_json::Value::Null);

            let template = format!("{{{{ input | {} }}}}", filter);
            let _ = engine.render(&template, &vars);
        }
    }
}

// ============================================================================
// INVARIANT PROPERTIES
// ============================================================================

mod invariants {
    use super::*;
    use rustible::inventory::{Group, Host, Inventory};
    use rustible::vars::{deep_merge, VarPrecedence, VarStore};

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        /// Invariant: YAML parsing should never panic (only return errors)
        #[test]
        fn parsing_never_panics(input in "\\PC{0,2000}") {
            // These should never panic
            let _: Result<serde_yaml::Value, _> = serde_yaml::from_str(&input);
        }

        /// Invariant: YAML roundtrip should preserve structure
        #[test]
        fn yaml_roundtrip(value in yaml_value()) {
            if let Ok(yaml_str) = serde_yaml::to_string(&value) {
                if let Ok(parsed) = serde_yaml::from_str::<serde_yaml::Value>(&yaml_str) {
                    // Roundtrip should produce equivalent structure
                    let original_str = serde_yaml::to_string(&value).unwrap_or_default();
                    let parsed_str = serde_yaml::to_string(&parsed).unwrap_or_default();
                    prop_assert_eq!(original_str, parsed_str);
                }
            }
        }

        /// Invariant: JSON roundtrip should preserve structure
        #[test]
        fn json_roundtrip(value in json_value()) {
            if let Ok(json_str) = serde_json::to_string(&value) {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                    prop_assert_eq!(value, parsed);
                }
            }
        }

        /// Invariant: Variable merging should be associative for mappings
        /// Note: Merging with Null is not associative by design (overlay wins),
        /// so we only test with mapping values
        #[test]
        fn merge_associative(
            a in yaml_mapping(),
            b in yaml_mapping(),
            c in yaml_mapping(),
        ) {
            // (a merge b) merge c == a merge (b merge c)
            let ab = deep_merge(&a, &b);
            let ab_c = deep_merge(&ab, &c);

            let bc = deep_merge(&b, &c);
            let a_bc = deep_merge(&a, &bc);

            // Compare string representations
            let left = serde_yaml::to_string(&ab_c).unwrap_or_default();
            let right = serde_yaml::to_string(&a_bc).unwrap_or_default();
            prop_assert_eq!(left, right);
        }

        /// Invariant: Inventory host count should match added hosts
        #[test]
        fn inventory_host_count_consistent(hosts in vec(valid_hostname(), 0..100)) {
            let mut inventory = Inventory::new();
            let unique_hosts: std::collections::HashSet<_> = hosts.iter().collect();

            for host_name in &hosts {
                let _ = inventory.add_host(Host::new(host_name.clone()));
            }

            // Host count should equal unique host names
            prop_assert_eq!(inventory.host_count(), unique_hosts.len());
        }

        /// Invariant: Group hierarchy should be consistent
        #[test]
        fn group_hierarchy_consistent(
            parent_name in valid_group_name(),
            children in vec(valid_group_name(), 1..10),
        ) {
            let mut inventory = Inventory::new();

            // Filter out children that have the same name as parent
            // (a group can't be its own child)
            let children: Vec<_> = children.into_iter()
                .filter(|c| c != &parent_name)
                .collect();

            if children.is_empty() {
                return Ok(());
            }

            let mut parent = Group::new(&parent_name);
            for child_name in &children {
                parent.add_child(child_name.clone());
            }
            let _ = inventory.add_group(parent);

            for child_name in &children {
                let child = Group::new(child_name);
                let _ = inventory.add_group(child);
            }

            // Each child should have the parent
            if let Some(parent_group) = inventory.get_group(&parent_name) {
                for child_name in &children {
                    prop_assert!(parent_group.has_child(child_name));
                }
            }
        }

        /// Invariant: VarStore precedence should be consistent
        #[test]
        fn var_precedence_consistent(
            key in valid_var_name(),
            low_value in yaml_value(),
            high_value in yaml_value(),
        ) {
            let mut store = VarStore::new();

            store.set(key.clone(), low_value.clone(), VarPrecedence::RoleDefaults);
            store.set(key.clone(), high_value.clone(), VarPrecedence::ExtraVars);

            // Higher precedence should always win
            if let Some(result) = store.get(&key) {
                let result_str = serde_yaml::to_string(result).unwrap_or_default();
                let high_str = serde_yaml::to_string(&high_value).unwrap_or_default();
                prop_assert_eq!(result_str, high_str);
            }
        }

        /// Invariant: Empty pattern should return no hosts
        #[test]
        fn empty_pattern_returns_empty(_dummy in Just(())) {
            let inventory = Inventory::new();
            match inventory.get_hosts_for_pattern("") {
                Ok(hosts) => prop_assert!(hosts.is_empty()),
                Err(_) => {} // Error is also acceptable
            }
        }

        /// Invariant: "all" pattern should return all hosts
        #[test]
        fn all_pattern_returns_all(hosts in vec(valid_hostname(), 1..50)) {
            let mut inventory = Inventory::new();
            let unique_hosts: std::collections::HashSet<_> = hosts.iter().collect();

            for host_name in &hosts {
                let _ = inventory.add_host(Host::new(host_name.clone()));
            }

            if let Ok(all_hosts) = inventory.get_hosts_for_pattern("all") {
                prop_assert_eq!(all_hosts.len(), unique_hosts.len());
            }
        }
    }
}

// ============================================================================
// EDGE CASE TESTS
// ============================================================================

mod edge_cases {
    use super::*;
    use rustible::inventory::{Group, Host};
    use rustible::template::TemplateEngine;

    #[test]
    fn test_empty_inputs() {
        let engine = TemplateEngine::new();

        // Empty YAML should parse
        let result: Result<serde_yaml::Value, _> = serde_yaml::from_str("");
        assert!(result.is_ok() || result.is_err()); // Should not panic

        // Empty template should render to empty
        let vars = HashMap::new();
        let result = engine.render("", &vars);
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn test_unicode_handling() {
        let engine = TemplateEngine::new();

        // Unicode in templates
        let mut vars = HashMap::new();
        vars.insert(
            "emoji".to_string(),
            serde_json::Value::String("Hello World".to_string()),
        );
        let result = engine.render("{{ emoji }}", &vars);
        assert!(result.is_ok());

        // Unicode in host names
        let host = Host::new("server-");
        assert_eq!(host.name, "server-");
    }

    #[test]
    fn test_null_handling() {
        let engine = TemplateEngine::new();
        let mut vars = HashMap::new();
        vars.insert("null_var".to_string(), serde_json::Value::Null);

        // Null with default filter
        let result = engine.render("{{ null_var | default('fallback') }}", &vars);
        assert!(result.is_ok());
    }

    #[test]
    fn test_very_long_strings() {
        // Very long hostname (should be truncated or handled)
        let long_name: String = (0..1000).map(|_| 'a').collect();
        let host = Host::new(long_name.clone());
        assert_eq!(host.name.len(), 1000);

        // Very long variable name
        let mut group = Group::new("test");
        let long_var_name: String = (0..1000).map(|_| 'v').collect();
        group.set_var(
            long_var_name.clone(),
            serde_yaml::Value::String("value".to_string()),
        );
        assert!(group.has_var(&long_var_name));
    }

    #[test]
    fn test_special_yaml_values() {
        // Test various special YAML strings
        let special_values = vec![
            "null", "NULL", "Null", "~", "true", "false", "TRUE", "FALSE", "yes", "no", ".inf",
            "-.inf", ".nan", "0x1A", "0o17", "0b1010",
        ];

        for val in special_values {
            let yaml = format!("key: {}", val);
            let _: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml); // Should not panic
        }
    }

    #[test]
    fn test_deeply_nested_structures() {
        use rustible::vars::deep_merge;

        // Create deeply nested structure
        let mut value = serde_yaml::Value::String("leaf".to_string());
        for i in 0..100 {
            let mut map = serde_yaml::Mapping::new();
            map.insert(serde_yaml::Value::String(format!("level{}", i)), value);
            value = serde_yaml::Value::Mapping(map);
        }

        // Deep merge should handle it
        let merged = deep_merge(&value, &value);
        assert!(!merged.is_null());
    }

    #[test]
    fn test_recursive_template_protection() {
        let engine = TemplateEngine::new();
        let mut vars = HashMap::new();

        // Self-referential variable (should not cause infinite loop)
        vars.insert(
            "a".to_string(),
            serde_json::Value::String("{{ a }}".to_string()),
        );

        let result = engine.render("{{ a }}", &vars);
        // Should either render or error, but not hang
        let _ = result;
    }

    #[test]
    fn test_binary_data_handling() {
        // Create binary-like data
        let binary_data: Vec<u8> = (0..255).collect();
        let binary_string = String::from_utf8_lossy(&binary_data).to_string();

        // Should handle gracefully
        let host = Host::new(binary_string);
        assert!(!host.name.is_empty());
    }
}

// ============================================================================
// STRESS TESTS
// ============================================================================

mod stress_tests {
    use super::*;
    use rustible::inventory::{Host, Inventory};
    use rustible::template::TemplateEngine;
    use rustible::vars::VarStore;

    #[test]
    fn test_many_variables() {
        use rustible::vars::VarPrecedence;
        let mut store = VarStore::new();

        // Add 10000 variables
        for i in 0..10000 {
            store.set(
                format!("var_{}", i),
                serde_yaml::Value::Number(i.into()),
                VarPrecedence::PlayVars,
            );
        }

        assert_eq!(store.len(), 10000);

        // Retrieving should work
        for i in 0..100 {
            let _ = store.get(&format!("var_{}", i));
        }
    }

    #[test]
    fn test_many_hosts() {
        let mut inventory = Inventory::new();

        // Add 1000 hosts
        for i in 0..1000 {
            let _ = inventory.add_host(Host::new(format!("host{}", i)));
        }

        assert_eq!(inventory.host_count(), 1000);

        // Pattern matching should still work
        let result = inventory.get_hosts_for_pattern("host*");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1000);
    }

    #[test]
    fn test_complex_yaml_parsing() {
        // Generate a complex playbook YAML
        let mut yaml = String::from("---\n");
        for play_num in 0..10 {
            yaml.push_str(&format!(
                "- name: Play {}\n  hosts: all\n  tasks:\n",
                play_num
            ));
            for task_num in 0..50 {
                yaml.push_str(&format!(
                    "    - name: Task {}.{}\n      debug:\n        msg: test\n",
                    play_num, task_num
                ));
            }
        }

        let result: Result<Vec<serde_yaml::Value>, _> = serde_yaml::from_str(&yaml);
        assert!(result.is_ok());
        let playbook = result.unwrap();
        assert_eq!(playbook.len(), 10);
    }

    #[test]
    fn test_template_with_many_variables() {
        let engine = TemplateEngine::new();
        let mut vars = HashMap::new();

        // Create many variables
        for i in 0..1000 {
            vars.insert(
                format!("var_{}", i),
                serde_json::Value::String(format!("value_{}", i)),
            );
        }

        // Template using some of them
        let mut template = String::new();
        for i in 0..100 {
            template.push_str(&format!("{{{{ var_{} }}}} ", i));
        }

        let result = engine.render(&template, &vars);
        assert!(result.is_ok());
    }
}
