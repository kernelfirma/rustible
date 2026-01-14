
#[cfg(test)]
mod tests {
    use rustible::modules::cron::CronModule;
    use rustible::modules::{Module, ModuleContext, ModuleParams};
    use rustible::connection::{Connection, CommandResult, ExecuteOptions, ConnectionError, TransferOptions, FileStat};
    use std::sync::{Arc, Mutex};
    use std::collections::HashMap;
    use async_trait::async_trait;
    use std::path::Path;
    use regex::Regex;

    struct MockConnection {
        executed_commands: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Connection for MockConnection {
        fn identifier(&self) -> &str { "mock" }
        async fn is_alive(&self) -> bool { true }

        async fn execute(&self, cmd: &str, _options: Option<ExecuteOptions>) -> Result<CommandResult, ConnectionError> {
            self.executed_commands.lock().unwrap().push(cmd.to_string());
            Ok(CommandResult {
                success: true,
                stdout: "".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            })
        }

        async fn upload(&self, _src: &Path, _dest: &Path, _options: Option<TransferOptions>) -> Result<(), ConnectionError> { Ok(()) }
        async fn download(&self, _src: &Path, _dest: &Path) -> Result<(), ConnectionError> { Ok(()) }
        async fn stat(&self, _path: &Path) -> Result<FileStat, ConnectionError> { unimplemented!() }
        async fn path_exists(&self, _path: &Path) -> Result<bool, ConnectionError> { Ok(false) }
        async fn close(&self) -> Result<(), ConnectionError> { Ok(()) }
        async fn upload_content(&self, _content: &[u8], _dest: &Path, _options: Option<TransferOptions>) -> Result<(), ConnectionError> { Ok(()) }
        async fn download_content(&self, _path: &Path) -> Result<Vec<u8>, ConnectionError> { Ok(Vec::new()) }
        async fn is_directory(&self, _path: &Path) -> Result<bool, ConnectionError> { Ok(false) }
    }

    #[test]
    fn test_cron_heredoc_injection() {
        let executed_commands = Arc::new(Mutex::new(Vec::new()));
        let connection = Arc::new(MockConnection {
            executed_commands: executed_commands.clone(),
        });

        let module = CronModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("name".to_string(), serde_json::json!("test_job"));
        // Inject RUSTIBLE_EOF into the job command
        params.insert("job".to_string(), serde_json::json!("/bin/true\nRUSTIBLE_EOF\ntouch /tmp/pwned"));
        params.insert("state".to_string(), serde_json::json!("present"));

        let context = ModuleContext::default().with_connection(connection);

        // Setup runtime context for execute() which calls block_on()
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let _ = module.execute(&params, &context);

        let commands = executed_commands.lock().unwrap();
        // The last command should be the set_crontab command
        if let Some(cmd) = commands.last() {
            println!("Executed command: {}", cmd);

            // Extract delimiter from "cat << 'DELIMITER'"
            let re = Regex::new(r"cat << '([^']+)'").unwrap();
            if let Some(caps) = re.captures(cmd) {
                let delimiter = caps.get(1).unwrap().as_str();
                println!("Using delimiter: {}", delimiter);

                // Verify delimiter is randomized (not just RUSTIBLE_EOF)
                assert!(delimiter.starts_with("RUSTIBLE_EOF_"));
                assert_ne!(delimiter, "RUSTIBLE_EOF");

                // Verify the payload is present but harmlessly wrapped
                // It should appear as text within the heredoc
                assert!(cmd.contains("RUSTIBLE_EOF\ntouch /tmp/pwned"));

                // Verify the content is followed by the delimiter (closing the heredoc)
                assert!(cmd.trim().ends_with(delimiter), "Command should end with delimiter: {}", delimiter);
            } else {
                panic!("Command does not match expected heredoc pattern: {}", cmd);
            }
        } else {
            panic!("No commands executed");
        }
    }
}
