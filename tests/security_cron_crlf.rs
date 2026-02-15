#[cfg(test)]
mod tests {
    use rustible::modules::cron::CronModule;
    use rustible::modules::{Module, ModuleContext, ModuleParams};
    use rustible::connection::{Connection, ExecuteOptions};
    use std::sync::Arc;
    use async_trait::async_trait;

    // Mock connection to capture the crontab content being set
    struct MockConnection {
        captured_crontab: std::sync::Mutex<String>,
    }

    #[async_trait]
    impl Connection for MockConnection {
        fn identifier(&self) -> &str { "mock" }
        async fn is_alive(&self) -> bool { true }

        async fn execute(
            &self,
            cmd: &str,
            _options: Option<ExecuteOptions>,
        ) -> Result<rustible::connection::CommandResult, rustible::connection::ConnectionError> {
            // Check if this is the command setting the crontab
            // Command format: cat << 'DELIM' | crontab ...\nCONTENT\nDELIM
            if cmd.contains("| crontab") {
                // Extract the content between the first newline and the last line
                let lines: Vec<&str> = cmd.lines().collect();
                if lines.len() > 2 {
                    // Content is everything between the first line (cat ...) and the last line (DELIM)
                    let content = lines[1..lines.len()-1].join("\n");
                    *self.captured_crontab.lock().unwrap() = content;
                }
            }

            Ok(rustible::connection::CommandResult {
                success: true,
                stdout: "".to_string(), // Empty current crontab
                stderr: "".to_string(),
                exit_code: 0,
            })
        }

        // Unused methods
        async fn upload(&self, _: &std::path::Path, _: &std::path::Path, _: Option<rustible::connection::TransferOptions>) -> Result<(), rustible::connection::ConnectionError> { Ok(()) }
        async fn download(&self, _: &std::path::Path, _: &std::path::Path) -> Result<(), rustible::connection::ConnectionError> { Ok(()) }
        async fn stat(&self, _: &std::path::Path) -> Result<rustible::connection::FileStat, rustible::connection::ConnectionError> { unimplemented!() }
        async fn path_exists(&self, _: &std::path::Path) -> Result<bool, rustible::connection::ConnectionError> { Ok(false) }
        async fn close(&self) -> Result<(), rustible::connection::ConnectionError> { Ok(()) }
        async fn upload_content(&self, _: &[u8], _: &std::path::Path, _: Option<rustible::connection::TransferOptions>) -> Result<(), rustible::connection::ConnectionError> { Ok(()) }
        async fn download_content(&self, _: &std::path::Path) -> Result<Vec<u8>, rustible::connection::ConnectionError> { Ok(Vec::new()) }
        async fn is_directory(&self, _: &std::path::Path) -> Result<bool, rustible::connection::ConnectionError> { Ok(false) }
    }

    #[test]
    fn test_cron_crlf_injection_prevention() {
        let connection = Arc::new(MockConnection {
            captured_crontab: std::sync::Mutex::new(String::new()),
        });

        // Setup module and context
        let module = CronModule;

        // Test case 1: Injection in 'name'
        let mut params = ModuleParams::default();
        params.insert("name".to_string(), "test_job\n* * * * * touch /tmp/pwned #".into());
        params.insert("job".to_string(), "ls -la".into());
        params.insert("state".to_string(), "present".into());

        let context = ModuleContext {
            connection: Some(connection.clone()),
            ..Default::default()
        };

        // Create a runtime and enter its context
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();

        // Run the module - expected to fail
        let result = module.execute(&params, &context);

        assert!(result.is_err(), "Module should return error for newline in name");
        if let Err(e) = result {
            let msg = format!("{}", e);
            assert!(msg.contains("newlines are not allowed"), "Error message should mention newlines, got: {}", msg);
        }

        // Test case 2: Injection in 'job'
        let mut params2 = ModuleParams::default();
        params2.insert("name".to_string(), "safe_name".into());
        params2.insert("job".to_string(), "ls -la\n* * * * * pwn".into());
        params2.insert("state".to_string(), "present".into());

        let result2 = module.execute(&params2, &context);
        assert!(result2.is_err(), "Module should return error for newline in job");
        if let Err(e) = result2 {
            let msg = format!("{}", e);
            assert!(msg.contains("newlines are not allowed"), "Error message should mention newlines, got: {}", msg);
        }

        // Verify that no crontab was set (captured_crontab should be empty or unchanged)
        let content = connection.captured_crontab.lock().unwrap().clone();
        assert!(content.is_empty(), "No crontab should have been written");
    }
}
