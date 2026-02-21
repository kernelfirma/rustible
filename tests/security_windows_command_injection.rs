use rustible::modules::{validate_command_args, command::CommandModule, Module, ModuleParams, ModuleContext};
use rustible::utils::cmd_arg_escape;
use std::collections::HashMap;

#[test]
fn test_validate_command_args_blocks_percent() {
    // This should now fail (Err) because we added % to dangerous_patterns
    assert!(validate_command_args("echo %USERNAME%").is_err());
}

#[test]
fn test_cmd_arg_escape_escapes_percent() {
    let input = "%USERNAME%";
    let escaped = cmd_arg_escape(input);
    // Should now return "%""USERNAME%" inside quotes (outer quotes + inner escaped quotes)
    // cmd_arg_escape wraps in "...", so result is "%""USERNAME%"
    // Wait, escaped string content: "%""USERNAME%"
    // Representation in Rust string literal: "\"%\"\"USERNAME%\"\"\""
    assert_eq!(escaped, "\"%\"\"USERNAME%\"\"\"");
}

#[test]
fn test_command_module_argv_escapes_percent() {
    let module = CommandModule;
    let mut params: ModuleParams = HashMap::new();
    params.insert(
        "argv".to_string(),
        serde_json::json!(["echo", "%USERNAME%"]),
    );
    params.insert("shell_type".to_string(), serde_json::json!("cmd"));

    let context = ModuleContext::default().with_check_mode(true);

    let result = module.execute(&params, &context).unwrap();
    let msg = result.msg;

    // msg format: "Would execute: <cmd>"
    // cmd should be: "echo" "%""USERNAME%"
    // Expected substring in msg: "\"%\"\"USERNAME%\"\"\""

    println!("Message: {}", msg);
    assert!(msg.contains("\"%\"\"USERNAME%\"\"\""));
}
