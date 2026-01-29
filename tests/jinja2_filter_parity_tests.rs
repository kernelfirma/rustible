//! Jinja2 Filter Parity Test Suite for Issue #286
//!
//! These tests exercise the production template engine filters to ensure
//! expected Jinja2/Ansible-compatible behavior.

use rustible::template::TemplateEngine;
use serde_json::{json, Value as JsonValue};

fn render_expr(expr: &str, context: JsonValue) -> String {
    let engine = TemplateEngine::new();
    let template = format!("{{{{ {} }}}}", expr);
    engine
        .render_with_json(&template, &context)
        .expect("template rendering should succeed")
}

fn render_expr_json(expr: &str, context: JsonValue) -> JsonValue {
    let rendered = render_expr(&format!("{} | to_json", expr), context);
    serde_json::from_str(&rendered).expect("rendered output should be valid JSON")
}

// ============================================================================
// Tests: String Filters
// ============================================================================

#[test]
fn test_string_filters() {
    let cases = vec![
        (r#""HELLO"|lower"#, json!("hello")),
        (r#""hello"|upper"#, json!("HELLO")),
        (r#""hello world"|capitalize"#, json!("Hello world")),
        (r#""hello world"|title"#, json!("Hello World")),
        (r#""  hello  "|trim"#, json!("hello")),
        (r#""hello world"|replace("world", "rust")"#, json!("hello rust")),
    ];

    for (expr, expected) in cases {
        assert_eq!(render_expr_json(expr, json!({})), expected);
    }
}

#[test]
fn test_regex_filters() {
    assert_eq!(
        render_expr_json(r#""abc123"|regex_search("\\d+")"#, json!({})),
        json!("123")
    );
    assert_eq!(
        render_expr_json(r#""abc"|regex_search("\\d+")"#, json!({})),
        json!("")
    );
    assert_eq!(
        render_expr_json(r#""abc123"|regex_replace("\\d+", "x")"#, json!({})),
        json!("abcx")
    );
}

#[test]
fn test_split_join_filters() {
    assert_eq!(
        render_expr_json(r#""a,b,c"|split(",")"#, json!({})),
        json!(["a", "b", "c"])
    );
    assert_eq!(
        render_expr_json(r#"['a', 'b', 'c']|join(", ")"#, json!({})),
        json!("a, b, c")
    );
}

// ============================================================================
// Tests: Type Conversion Filters
// ============================================================================

#[test]
fn test_type_conversion_filters() {
    let cases = vec![
        (r#""42"|int"#, json!(42)),
        (r#""3.5"|float"#, json!(3.5)),
        (r#"1|string"#, json!("1")),
        (r#"''|bool"#, json!(false)),
        (r#""hello"|bool"#, json!(true)),
        (r#""ab"|list"#, json!(["a", "b"])),
    ];

    for (expr, expected) in cases {
        assert_eq!(render_expr_json(expr, json!({})), expected);
    }
}

// ============================================================================
// Tests: Collection Filters
// ============================================================================

#[test]
fn test_collection_filters() {
    let cases = vec![
        (r#""abc"|first"#, json!("a")),
        (r#""abc"|last"#, json!("c")),
        (r#""hello"|length"#, json!(5)),
        (r#""hello"|count"#, json!(5)),
        (r#"[1,2,1,3]|unique"#, json!([1, 2, 3])),
        (r#"[3,1,2]|sort"#, json!([1, 2, 3])),
        (r#"[1,2,3]|reverse"#, json!([3, 2, 1])),
        (r#"[[1,2],[3],4]|flatten"#, json!([1, 2, 3, 4])),
    ];

    for (expr, expected) in cases {
        assert_eq!(render_expr_json(expr, json!({})), expected);
    }
}

// ============================================================================
// Tests: Path Filters
// ============================================================================

#[test]
fn test_path_filters() {
    let cases = vec![
        (r#""/path/to/file.txt"|basename"#, json!("file.txt")),
        (r#""/path/to/file.txt"|dirname"#, json!("/path/to")),
        (r#""/tmp/file"|expanduser"#, json!("/tmp/file")),
        (r#""/does/not/exist"|realpath"#, json!("/does/not/exist")),
    ];

    for (expr, expected) in cases {
        assert_eq!(render_expr_json(expr, json!({})), expected);
    }
}

// ============================================================================
// Tests: Encoding & Serialization Filters
// ============================================================================

#[test]
fn test_encoding_filters() {
    assert_eq!(
        render_expr_json(r#""hello"|b64encode"#, json!({})),
        json!("aGVsbG8=")
    );
    assert_eq!(
        render_expr_json(r#""aGVsbG8="|b64decode"#, json!({})),
        json!("hello")
    );
}

#[test]
fn test_json_yaml_filters() {
    let json_rendered = render_expr(r#"{'a': 1, 'b': 2}|to_json"#, json!({}));
    let json_value: JsonValue =
        serde_json::from_str(&json_rendered).expect("to_json output should parse");
    assert_eq!(json_value, json!({"a": 1, "b": 2}));

    let context = json!({"payload": "{\"a\": 1}"});
    assert_eq!(
        render_expr_json("payload | from_json", context),
        json!({"a": 1})
    );

    assert_eq!(
        render_expr_json(r#"{'a': 1}|to_yaml|from_yaml"#, json!({})),
        json!({"a": 1})
    );

    let docs = json!({"docs": "---\na: 1\n---\na: 2\n"});
    assert_eq!(
        render_expr_json("docs | from_yaml_all", docs),
        json!([{"a": 1}, {"a": 2}])
    );

    assert_eq!(
        render_expr_json(r#"{'a': 1}|to_nice_yaml|from_yaml"#, json!({})),
        json!({"a": 1})
    );

    let pretty = render_expr(r#"{'a': 1}|to_nice_json"#, json!({}));
    let pretty_value: JsonValue =
        serde_json::from_str(&pretty).expect("to_nice_json output should parse");
    assert_eq!(pretty_value, json!({"a": 1}));
}

// ============================================================================
// Tests: Ansible-Specific Filters
// ============================================================================

#[test]
fn test_default_filter_and_alias() {
    assert_eq!(
        render_expr_json("missing | default('fallback')", json!({})),
        json!("fallback")
    );
    assert_eq!(
        render_expr_json("missing | d('fallback')", json!({})),
        json!("fallback")
    );
}

#[test]
fn test_mandatory_filter() {
    assert_eq!(
        render_expr_json("present | mandatory", json!({"present": "value"})),
        json!("value")
    );

    let engine = TemplateEngine::new();
    let template = "{{ missing | mandatory }}";
    assert!(engine.render_with_json(template, &json!({})).is_err());
}

#[test]
fn test_ternary_filter() {
    assert_eq!(
        render_expr_json(r#"true | ternary("yes", "no")"#, json!({})),
        json!("yes")
    );
    assert_eq!(
        render_expr_json(r#"false | ternary("yes", "no")"#, json!({})),
        json!("no")
    );
}

#[test]
fn test_combine_dict_filters() {
    assert_eq!(
        render_expr_json(r#"{'a': 1}|combine({'b': 2})"#, json!({})),
        json!({"a": 1, "b": 2})
    );

    assert_eq!(
        render_expr_json(r#"{'a': 1}|dict2items"#, json!({})),
        json!([{"key": "a", "value": 1}])
    );

    assert_eq!(
        render_expr_json(
            r#"[{'key': 'a', 'value': 1}, {'key': 'b', 'value': 2}]|items2dict"#,
            json!({})
        ),
        json!({"a": 1, "b": 2})
    );
}

#[test]
fn test_select_reject_map_filters() {
    let context = json!({
        "items": [
            {"name": "a", "enabled": true},
            {"name": "b", "enabled": false}
        ]
    });

    assert_eq!(
        render_expr_json("items | selectattr('enabled')", context.clone()),
        json!([{"name": "a", "enabled": true}])
    );
    assert_eq!(
        render_expr_json("items | rejectattr('enabled')", context.clone()),
        json!([{"name": "b", "enabled": false}])
    );
    assert_eq!(
        render_expr_json("items | map('name')", context),
        json!(["a", "b"])
    );
}
