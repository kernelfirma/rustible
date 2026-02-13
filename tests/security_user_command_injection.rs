
use async_trait::async_trait;
use rustible::connection::{
    CommandResult, Connection, ConnectionResult, ExecuteOptions, FileStat, TransferOptions,
};
use rustible::modules::{Module, ModuleContext, ModuleParams, user::UserModule};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

// Mock connection that records commands
#[derive(Clone)]
struct MockConnection {
    commands: Arc<Mutex<Vec<String>>>,
}

impl MockConnection {
    fn new() -> Self {
        Self {
            commands: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn get_commands(&self) -> Vec<String> {
        self.commands.lock().unwrap().clone()
    }
}

#[async_trait]
impl Connection for MockConnection {
    fn identifier(&self) -> &str {
        "mock"
    }

    async fn is_alive(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        command: &str,
        _options: Option<ExecuteOptions>,
    ) -> ConnectionResult<CommandResult> {
        self.commands.lock().unwrap().push(command.to_string());

        // Return simulated success for user module checks
        // We need to simulate "id user" returning false (user doesn't exist)
        // so create_user path is taken.
        // Or if it's "useradd", return success.

        if command.starts_with("id ") {
             return Ok(CommandResult::failure(1, "".to_string(), "no such user".to_string()));
        }

        Ok(CommandResult::success("".to_string(), "".to_string()))
    }

    async fn upload(
        &self,
        _local_path: &Path,
        _remote_path: &Path,
        _options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        Ok(())
    }

    async fn upload_content(
        &self,
        _content: &[u8],
        _remote_path: &Path,
        _options: Option<TransferOptions>,
    ) -> ConnectionResult<()> {
        Ok(())
    }

    async fn download(&self, _remote_path: &Path, _local_path: &Path) -> ConnectionResult<()> {
        Ok(())
    }

    async fn download_content(&self, _remote_path: &Path) -> ConnectionResult<Vec<u8>> {
        Ok(Vec::new())
    }

    async fn path_exists(&self, _path: &Path) -> ConnectionResult<bool> {
        Ok(false)
    }

    async fn is_directory(&self, _path: &Path) -> ConnectionResult<bool> {
        Ok(false)
    }

    async fn stat(&self, _path: &Path) -> ConnectionResult<FileStat> {
        Err(rustible::connection::ConnectionError::ConnectionFailed("Not implemented".to_string()))
    }

    async fn close(&self) -> ConnectionResult<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_user_module_command_injection_in_groups() {
    let connection = Arc::new(MockConnection::new());
    let module = UserModule;

    let mut params: ModuleParams = HashMap::new();
    params.insert("name".to_string(), serde_json::json!("testuser"));
    // Malicious group name containing command injection
    params.insert("groups".to_string(), serde_json::json!(["safe_group", "wheel; touch /tmp/pwned"]));

    let context = ModuleContext::default()
        .with_connection(connection.clone());

    // Execute module
    let _ = module.execute(&params, &context);

    // Check executed commands
    let commands = connection.get_commands();

    // We expect commands:
    // 1. "id 'testuser'" (to check existence)
    // 2. "useradd ... -G safe_group,wheel; touch /tmp/pwned ... 'testuser'"

    let useradd_cmd = commands.iter().find(|cmd| cmd.contains("useradd")).expect("No useradd command found");
    println!("Executed command: {}", useradd_cmd);

    // Verify INJECTION logic:
    // If injection is successful (vulnerable), the command will contain unescaped semicolon
    // If properly escaped, it should be something like "safe_group,'wheel; touch /tmp/pwned'" or similar,
    // but definitely not a raw semicolon that terminates the command.

    // In this specific case, if we see "; touch /tmp/pwned" without quotes around it, it's vulnerable.
    // However, it's easier to check if we effectively passed a list where the semicolon is NOT escaped.

    // If vulnerable: ... -G safe_group,wheel; touch /tmp/pwned ...
    // If fixed: ... -G safe_group,'wheel; touch /tmp/pwned' ... (or similar escaping)

    // The current vulnerability allows the semicolon to be interpreted by the shell.
    // "groups.join(",")" -> "safe_group,wheel; touch /tmp/pwned"

    // Assert that the command contains the ESCAPED string (quoted)
    // The shell_escape function wraps unsafe strings in single quotes
    // Expected: ... -G 'safe_group,wheel; touch /tmp/pwned' ...

    let expected_escaped_arg = "'safe_group,wheel; touch /tmp/pwned'";
    assert!(useradd_cmd.contains(expected_escaped_arg), "Command should contain the escaped groups list: '{}', found: '{}'", expected_escaped_arg, useradd_cmd);

    // Ensure it does NOT contain the unescaped semicolon part exposed
    // This double checks that we didn't just append the escaped version while leaving the unescaped one
    // (unlikely given the code change, but good for verification)

    // We search for ; surrounded by anything that isn't a quote
    // But simplest is to ensure the specific dangerous sequence is NOT present WITHOUT quotes.
    // This is hard to regex without regex crate, but the positive assertion above is strong enough.

    println!("Verified safe command: {}", useradd_cmd);
}
