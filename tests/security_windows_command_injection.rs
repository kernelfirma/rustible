use rustible::modules::{validate_command_args, command::CommandModule, Module, ModuleParams, ModuleContext};
use rustible::utils::cmd_arg_escape;
use std::collections::HashMap;

#[test]
fn test_validate_command_args_allows_percent_globally() {
    assert!(validate_command_args("echo %USERNAME%").is_ok());
}

#[test]
fn test_cmd_arg_escape_preserves_percent() {
    let input = "%USERNAME%";
    let escaped = cmd_arg_escape(input);
    assert_eq!(escaped, "\"%USERNAME%\"");
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

    println!("Message: {}", msg);
    assert!(msg.contains("\"%USERNAME%\""));
}
