#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use regex::Regex;
    use rustible::connection::{
        CommandResult, Connection, ConnectionError, ExecuteOptions, FileStat, TransferOptions,
    };
    use rustible::modules::cron::CronModule;
    use rustible::modules::{Module, ModuleContext, ModuleParams};
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    struct MockConnection {
        executed_commands: Arc<Mutex<Vec<String>>>,
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
            cmd: &str,
            _options: Option<ExecuteOptions>,
        ) -> Result<CommandResult, ConnectionError> {
            self.executed_commands.lock().unwrap().push(cmd.to_string());
            Ok(CommandResult {
                success: true,
                stdout: "".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            })
        }

        async fn upload(
            &self,
            _src: &Path,
            _dest: &Path,
            _options: Option<TransferOptions>,
        ) -> Result<(), ConnectionError> {
            Ok(())
        }
        async fn download(&self, _src: &Path, _dest: &Path) -> Result<(), ConnectionError> {
            Ok(())
        }
        async fn stat(&self, _path: &Path) -> Result<FileStat, ConnectionError> {
            unimplemented!()
        }
        async fn path_exists(&self, _path: &Path) -> Result<bool, ConnectionError> {
            Ok(false)
        }
        async fn close(&self) -> Result<(), ConnectionError> {
            Ok(())
        }
        async fn upload_content(
            &self,
            _content: &[u8],
            _dest: &Path,
            _options: Option<TransferOptions>,
        ) -> Result<(), ConnectionError> {
            Ok(())
        }
        async fn download_content(&self, _path: &Path) -> Result<Vec<u8>, ConnectionError> {
            Ok(Vec::new())
        }
        async fn is_directory(&self, _path: &Path) -> Result<bool, ConnectionError> {
            Ok(false)
        }
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
        params.insert(
            "job".to_string(),
            serde_json::json!("/bin/true\nRUSTIBLE_EOF\ntouch /tmp/pwned"),
        );
        params.insert("state".to_string(), serde_json::json!("present"));

        let context = ModuleContext::default().with_connection(connection);

        // Setup runtime context for execute() which calls block_on()
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        let result = module.execute(&params, &context);

        // The cron module should reject the newline-injected job parameter
        // before any commands are executed, preventing the injection entirely
        assert!(
            result.is_err(),
            "Cron module should reject job parameters containing newlines"
        );

        let commands = executed_commands.lock().unwrap();
        assert!(
            commands.is_empty(),
            "No commands should be executed when injection is detected"
        );
    }
}
