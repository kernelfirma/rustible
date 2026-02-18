//! Agent Mode Operational Pilot Tests
//!
//! This test suite validates the agent mode operational workflow:
//! 1. Agent build and configuration
//! 2. Agent runtime lifecycle
//! 3. Command execution via agent
//! 4. Playbook execution simulation
//! 5. Error handling and recovery
//!
//! These tests serve as an operational pilot to ensure agent mode
//! works correctly for standard playbook execution workflows.

use rustible::agent::{
    AgentBuilder, AgentClient, AgentConfig, AgentMethod, AgentRequest, AgentResponse,
    AgentRpcError, AgentRuntime, AgentStatus, ExecuteParams, ExecuteResult, HostInfo,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

// ============================================================================
// Agent Configuration Tests
// ============================================================================

mod config_tests {
    use super::*;

    #[test]
    fn test_agent_config_default_values() {
        let config = AgentConfig::default();

        assert_eq!(config.listen, "/var/run/rustible-agent.sock");
        assert!(!config.tls);
        assert!(config.tls_cert.is_none());
        assert!(config.tls_key.is_none());
        assert_eq!(config.idle_timeout, Duration::from_secs(3600));
        assert_eq!(config.max_concurrent, 10);
        assert_eq!(config.work_dir, PathBuf::from("/tmp/rustible"));
        assert_eq!(config.log_level, "info");
        assert!(config.auth_token.is_none());
    }

    #[test]
    fn test_agent_config_serialization() {
        let config = AgentConfig::default();
        let json = serde_json::to_string(&config).expect("Failed to serialize config");

        assert!(json.contains("rustible-agent.sock"));
        assert!(json.contains("\"tls\":false"));
        assert!(json.contains("\"max_concurrent\":10"));

        let deserialized: AgentConfig =
            serde_json::from_str(&json).expect("Failed to deserialize config");
        assert_eq!(deserialized.listen, config.listen);
        assert_eq!(deserialized.max_concurrent, config.max_concurrent);
    }

    #[test]
    fn test_agent_config_custom_values() {
        let config = AgentConfig {
            listen: "127.0.0.1:8080".to_string(),
            tls: true,
            tls_cert: Some(PathBuf::from("/etc/ssl/cert.pem")),
            tls_key: Some(PathBuf::from("/etc/ssl/key.pem")),
            idle_timeout: Duration::from_secs(7200),
            max_concurrent: 50,
            work_dir: PathBuf::from("/opt/rustible/work"),
            log_level: "debug".to_string(),
            auth_token: Some("secret-token-123".to_string()),
        };

        assert!(config.tls);
        assert_eq!(config.max_concurrent, 50);
        assert!(config.auth_token.is_some());
    }

    #[test]
    fn test_agent_config_tcp_listen() {
        let config = AgentConfig {
            listen: "0.0.0.0:9999".to_string(),
            ..Default::default()
        };

        assert!(config.listen.contains(':'));
        assert!(config.listen.starts_with("0.0.0.0"));
    }

    #[test]
    fn test_agent_config_unix_socket() {
        let config = AgentConfig {
            listen: "/var/run/custom-agent.sock".to_string(),
            ..Default::default()
        };

        assert!(config.listen.starts_with('/'));
        assert!(config.listen.ends_with(".sock"));
    }
}

// ============================================================================
// Agent Builder Tests
// ============================================================================

mod builder_tests {
    use super::*;

    #[test]
    fn test_agent_builder_default() {
        let builder = AgentBuilder::new();
        let config = builder.config();

        assert!(config.release);
        assert!(config.strip);
        assert!(!config.compress);
    }

    #[test]
    fn test_agent_builder_target_configuration() {
        let builder = AgentBuilder::new()
            .target("x86_64-unknown-linux-gnu")
            .release(true)
            .strip(true);

        let config = builder.config();
        assert_eq!(config.target, "x86_64-unknown-linux-gnu");
        assert!(config.release);
        assert!(config.strip);
    }

    #[test]
    fn test_agent_builder_aarch64_target() {
        let builder = AgentBuilder::new().target("aarch64-unknown-linux-gnu");

        let config = builder.config();
        assert_eq!(config.target, "aarch64-unknown-linux-gnu");
    }

    #[test]
    fn test_agent_builder_debug_build() {
        let builder = AgentBuilder::new().release(false).strip(false);

        let config = builder.config();
        assert!(!config.release);
        assert!(!config.strip);
    }

    #[test]
    fn test_agent_builder_output_dir() {
        let builder = AgentBuilder::new().output_dir(PathBuf::from("/custom/output"));

        let config = builder.config();
        assert_eq!(config.output_dir, PathBuf::from("/custom/output"));
    }

    #[test]
    fn test_agent_builder_fluent_api() {
        let builder = AgentBuilder::new()
            .target("x86_64-unknown-linux-musl")
            .release(true)
            .strip(true)
            .output_dir(PathBuf::from("/tmp/agent-build"));

        let config = builder.config();
        assert_eq!(config.target, "x86_64-unknown-linux-musl");
        assert!(config.release);
        assert!(config.strip);
        assert_eq!(config.output_dir, PathBuf::from("/tmp/agent-build"));
    }

    #[test]
    fn test_current_target() {
        let target = rustible::agent::current_target();

        assert!(!target.is_empty());
        assert!(target.contains('-'));

        // Should contain arch and os
        let parts: Vec<&str> = target.split('-').collect();
        assert!(parts.len() >= 2);
    }
}

// ============================================================================
// Agent Runtime Tests
// ============================================================================

mod runtime_tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_runtime_creation() {
        let config = AgentConfig::default();
        let runtime = AgentRuntime::new(config);

        let status = runtime.status();
        assert_eq!(status.tasks_executed, 0);
        assert_eq!(status.tasks_running, 0);
    }

    #[tokio::test]
    async fn test_agent_runtime_status() {
        let runtime = AgentRuntime::new(AgentConfig::default());
        let status = runtime.status();

        assert!(!status.version.is_empty());
        // uptime is u64, always >= 0
        assert_eq!(status.tasks_executed, 0);
        assert_eq!(status.tasks_running, 0);
    }

    #[tokio::test]
    async fn test_agent_runtime_host_info() {
        let runtime = AgentRuntime::new(AgentConfig::default());
        let status = runtime.status();

        // Host info should be populated
        assert!(!status.host_info.hostname.is_empty() || status.host_info.hostname == "unknown");
        assert!(!status.host_info.os.is_empty());
        assert!(!status.host_info.arch.is_empty());
        assert!(status.host_info.cpus > 0);
    }

    #[tokio::test]
    async fn test_agent_runtime_execute_echo() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        let params = ExecuteParams {
            command: "echo hello".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };

        let result = runtime.execute(params).await;

        // The test runs locally, so this should work
        assert!(result.is_ok(), "Execute should succeed");
        let result = result.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_agent_runtime_execute_with_env() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        let mut env = HashMap::new();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());

        let params = ExecuteParams {
            command: "echo $TEST_VAR".to_string(),
            cwd: Some("/tmp".to_string()),
            env,
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };

        let result = runtime.execute(params).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("test_value"));
    }

    #[tokio::test]
    async fn test_agent_runtime_execute_failure() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        let params = ExecuteParams {
            command: "false".to_string(), // Command that returns exit code 1
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };

        let result = runtime.execute(params).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_agent_runtime_task_counting() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        // Initial state
        let status = runtime.status();
        assert_eq!(status.tasks_executed, 0);

        // Execute a command
        let params = ExecuteParams {
            command: "echo test".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let _ = runtime.execute(params).await;

        // After execution
        let status = runtime.status();
        assert_eq!(status.tasks_executed, 1);
    }

    #[tokio::test]
    async fn test_agent_runtime_multiple_commands() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        for i in 0..5 {
            let params = ExecuteParams {
                command: format!("echo iteration_{}", i),
                cwd: Some("/tmp".to_string()),
                env: HashMap::new(),
                timeout: Some(30),
                user: None,
                group: None,
                shell: true,
            };

            let result = runtime.execute(params).await;
            assert!(result.is_ok());
            assert!(result.unwrap().stdout.contains(&format!("iteration_{}", i)));
        }

        let status = runtime.status();
        assert_eq!(status.tasks_executed, 5);
    }
}

// ============================================================================
// Execute Params Tests
// ============================================================================

mod execute_params_tests {
    use super::*;

    #[test]
    fn test_execute_params_basic() {
        let params = ExecuteParams {
            command: "ls -la".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(60),
            user: None,
            group: None,
            shell: false,
        };

        assert_eq!(params.command, "ls -la");
        assert_eq!(params.cwd, Some("/tmp".to_string()));
        assert_eq!(params.timeout, Some(60));
        assert!(!params.shell);
    }

    #[test]
    fn test_execute_params_serialization() {
        let mut env = HashMap::new();
        env.insert("KEY".to_string(), "VALUE".to_string());

        let params = ExecuteParams {
            command: "echo test".to_string(),
            cwd: Some("/home".to_string()),
            env,
            timeout: Some(30),
            user: Some("nobody".to_string()),
            group: Some("nogroup".to_string()),
            shell: true,
        };

        let json = serde_json::to_string(&params).expect("Failed to serialize");

        assert!(json.contains("echo test"));
        assert!(json.contains("/home"));
        assert!(json.contains("KEY"));
        assert!(json.contains("VALUE"));
        assert!(json.contains("nobody"));
        assert!(json.contains("nogroup"));
    }

    #[test]
    fn test_execute_params_deserialization() {
        let json = r#"{
            "command": "cat /etc/hostname",
            "cwd": "/tmp",
            "env": {"FOO": "bar"},
            "timeout": 120,
            "user": "root",
            "group": "root",
            "shell": true
        }"#;

        let params: ExecuteParams = serde_json::from_str(json).expect("Failed to deserialize");

        assert_eq!(params.command, "cat /etc/hostname");
        assert_eq!(params.cwd, Some("/tmp".to_string()));
        assert_eq!(params.timeout, Some(120));
        assert_eq!(params.user, Some("root".to_string()));
        assert!(params.shell);
        assert_eq!(params.env.get("FOO"), Some(&"bar".to_string()));
    }
}

// ============================================================================
// Execute Result Tests
// ============================================================================

mod execute_result_tests {
    use super::*;

    #[test]
    fn test_execute_result_success() {
        let result = ExecuteResult {
            exit_code: 0,
            stdout: "success output".to_string(),
            stderr: "".to_string(),
            duration_ms: 1234,
        };

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("success"));
        assert!(result.stderr.is_empty());
        assert_eq!(result.duration_ms, 1234);
    }

    #[test]
    fn test_execute_result_failure() {
        let result = ExecuteResult {
            exit_code: 1,
            stdout: "".to_string(),
            stderr: "error: command failed".to_string(),
            duration_ms: 500,
        };

        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
        assert!(result.stderr.contains("error"));
    }

    #[test]
    fn test_execute_result_serialization() {
        let result = ExecuteResult {
            exit_code: 0,
            stdout: "line1\nline2\n".to_string(),
            stderr: "warning: deprecated".to_string(),
            duration_ms: 100,
        };

        let json = serde_json::to_string(&result).expect("Failed to serialize");

        assert!(json.contains("\"exit_code\":0"));
        assert!(json.contains("line1"));
        assert!(json.contains("deprecated"));
        assert!(json.contains("\"duration_ms\":100"));
    }
}

// ============================================================================
// Agent Request/Response Tests
// ============================================================================

mod protocol_tests {
    use super::*;

    #[test]
    fn test_agent_method_serialization() {
        assert_eq!(
            serde_json::to_string(&AgentMethod::Execute).unwrap(),
            "\"execute\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMethod::Upload).unwrap(),
            "\"upload\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMethod::Download).unwrap(),
            "\"download\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMethod::Stat).unwrap(),
            "\"stat\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMethod::Mkdir).unwrap(),
            "\"mkdir\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMethod::Delete).unwrap(),
            "\"delete\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMethod::Facts).unwrap(),
            "\"facts\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMethod::Ping).unwrap(),
            "\"ping\""
        );
        assert_eq!(
            serde_json::to_string(&AgentMethod::Shutdown).unwrap(),
            "\"shutdown\""
        );
    }

    #[test]
    fn test_agent_request_serialization() {
        let request = AgentRequest {
            id: "test-123".to_string(),
            method: AgentMethod::Execute,
            params: Some(serde_json::json!({
                "command": "echo hello",
                "shell": true
            })),
        };

        let json = serde_json::to_string(&request).expect("Failed to serialize");

        assert!(json.contains("test-123"));
        assert!(json.contains("execute"));
        assert!(json.contains("echo hello"));
    }

    #[test]
    fn test_agent_response_success() {
        let response = AgentResponse {
            id: "test-123".to_string(),
            result: Some(serde_json::json!({
                "exit_code": 0,
                "stdout": "hello",
                "stderr": "",
                "duration_ms": 100
            })),
            error: None,
        };

        let json = serde_json::to_string(&response).expect("Failed to serialize");

        assert!(json.contains("test-123"));
        assert!(json.contains("exit_code"));
        // error field is present but null when None (serde default behavior)
        assert!(json.contains("\"error\":null"));
    }

    #[test]
    fn test_agent_response_error() {
        let response = AgentResponse {
            id: "test-123".to_string(),
            result: None,
            error: Some(AgentRpcError {
                code: -32600,
                message: "Invalid request".to_string(),
                data: None,
            }),
        };

        let json = serde_json::to_string(&response).expect("Failed to serialize");

        assert!(json.contains("-32600"));
        assert!(json.contains("Invalid request"));
    }

    #[test]
    fn test_agent_rpc_error() {
        let error = AgentRpcError {
            code: -32601,
            message: "Method not found".to_string(),
            data: Some(serde_json::json!({"method": "unknown"})),
        };

        let json = serde_json::to_string(&error).expect("Failed to serialize");

        assert!(json.contains("-32601"));
        assert!(json.contains("Method not found"));
        assert!(json.contains("unknown"));
    }
}

// ============================================================================
// Agent Client Tests
// ============================================================================

mod client_tests {
    use super::*;

    #[test]
    fn test_agent_client_creation() {
        let _client = AgentClient::new("192.168.1.100", "/var/run/rustible-agent.sock");

        // Client should be created without error
        let _ = &_client;
    }

    #[test]
    fn test_agent_client_with_auth() {
        let _client = AgentClient::new("192.168.1.100", "/var/run/rustible-agent.sock")
            .with_auth_token("secret-token".to_string());

        // Client should be created without error
        let _ = &_client;
    }

    #[test]
    fn test_agent_client_with_timeout() {
        let _client = AgentClient::new("192.168.1.100", "/var/run/rustible-agent.sock")
            .with_timeout(Duration::from_secs(60));

        // Client should be created without error
        let _ = &_client;
    }

    #[test]
    fn test_agent_client_fluent_api() {
        let _client = AgentClient::new("192.168.1.100", "127.0.0.1:8080")
            .with_auth_token("token123".to_string())
            .with_timeout(Duration::from_secs(120));

        // Client should be created without error
        let _ = &_client;
    }

    #[tokio::test]
    async fn test_agent_client_ping_not_running() {
        let client = AgentClient::new("nonexistent-host", "/var/run/rustible-agent.sock");

        let result = client.ping().await;

        // Should fail since agent is not running
        assert!(result.is_err());
    }
}

// ============================================================================
// Agent Status Tests
// ============================================================================

mod status_tests {
    use super::*;

    #[test]
    fn test_agent_status_creation() {
        let status = AgentStatus {
            version: "0.1.0".to_string(),
            uptime: 3600,
            tasks_executed: 100,
            tasks_running: 5,
            host_info: HostInfo {
                hostname: "test-host".to_string(),
                os: "linux".to_string(),
                arch: "x86_64".to_string(),
                cpus: 8,
                memory_total: 16_000_000_000,
                memory_available: 8_000_000_000,
            },
        };

        assert_eq!(status.version, "0.1.0");
        assert_eq!(status.uptime, 3600);
        assert_eq!(status.tasks_executed, 100);
        assert_eq!(status.tasks_running, 5);
        assert_eq!(status.host_info.hostname, "test-host");
    }

    #[test]
    fn test_agent_status_serialization() {
        let status = AgentStatus {
            version: "1.0.0".to_string(),
            uptime: 7200,
            tasks_executed: 500,
            tasks_running: 2,
            host_info: HostInfo {
                hostname: "server-01".to_string(),
                os: "linux".to_string(),
                arch: "aarch64".to_string(),
                cpus: 4,
                memory_total: 8_000_000_000,
                memory_available: 4_000_000_000,
            },
        };

        let json = serde_json::to_string(&status).expect("Failed to serialize");

        assert!(json.contains("1.0.0"));
        assert!(json.contains("7200"));
        assert!(json.contains("500"));
        assert!(json.contains("server-01"));
        assert!(json.contains("aarch64"));
    }

    #[test]
    fn test_host_info_deserialization() {
        let json = r#"{
            "hostname": "web-01",
            "os": "linux",
            "arch": "x86_64",
            "cpus": 16,
            "memory_total": 32000000000,
            "memory_available": 24000000000
        }"#;

        let info: HostInfo = serde_json::from_str(json).expect("Failed to deserialize");

        assert_eq!(info.hostname, "web-01");
        assert_eq!(info.cpus, 16);
        assert_eq!(info.memory_total, 32_000_000_000);
    }
}

// ============================================================================
// Pilot Workflow Tests (End-to-End Simulation)
// ============================================================================

mod pilot_workflow_tests {
    use super::*;

    /// Simulates a standard playbook workflow using agent mode
    #[tokio::test]
    async fn test_pilot_workflow_basic_tasks() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        // Simulate task 1: Check hostname (use cat /etc/hostname for portability)
        let params = ExecuteParams {
            command: "cat /etc/hostname 2>/dev/null || echo 'test-host'".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().exit_code, 0);

        // Simulate task 2: Create temp file
        let params = ExecuteParams {
            command: "touch /tmp/rustible-pilot-test".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().exit_code, 0);

        // Simulate task 3: Verify file exists
        let params = ExecuteParams {
            command: "test -f /tmp/rustible-pilot-test && echo exists".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("exists"));

        // Cleanup
        let params = ExecuteParams {
            command: "rm -f /tmp/rustible-pilot-test".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let _ = runtime.execute(params).await;

        // Verify task count
        let status = runtime.status();
        assert_eq!(status.tasks_executed, 4);
    }

    /// Simulates environment variable passing
    #[tokio::test]
    async fn test_pilot_workflow_environment() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        let mut env = HashMap::new();
        env.insert("PILOT_VAR".to_string(), "pilot_value".to_string());
        env.insert("PILOT_NUM".to_string(), "42".to_string());

        let params = ExecuteParams {
            command: "echo $PILOT_VAR $PILOT_NUM".to_string(),
            cwd: Some("/tmp".to_string()),
            env,
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };

        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.stdout.contains("pilot_value"));
        assert!(result.stdout.contains("42"));
    }

    /// Simulates working directory handling
    #[tokio::test]
    async fn test_pilot_workflow_working_directory() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        // Create temp directory
        let params = ExecuteParams {
            command: "mkdir -p /tmp/rustible-pilot-wd".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let _ = runtime.execute(params).await;

        // Execute in specific working directory
        let params = ExecuteParams {
            command: "pwd".to_string(),
            cwd: Some("/tmp/rustible-pilot-wd".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };

        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(
            result.stdout.contains("/tmp/rustible-pilot-wd")
                || result.stdout.contains("/private/tmp/rustible-pilot-wd")
        );

        // Cleanup
        let params = ExecuteParams {
            command: "rm -rf /tmp/rustible-pilot-wd".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let _ = runtime.execute(params).await;
    }

    /// Simulates error handling in playbook execution
    #[tokio::test]
    async fn test_pilot_workflow_error_handling() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        // Task that will fail
        let params = ExecuteParams {
            command: "exit 42".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };

        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.exit_code, 42);
    }

    /// Simulates multi-line script execution
    #[tokio::test]
    async fn test_pilot_workflow_script_execution() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        let params = ExecuteParams {
            command: r#"
                set -e
                echo "Step 1: Starting"
                RESULT="success"
                echo "Step 2: Result is $RESULT"
                echo "Step 3: Done"
            "#
            .to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(60),
            user: None,
            group: None,
            shell: true,
        };

        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Starting"));
        assert!(result.stdout.contains("success"));
        assert!(result.stdout.contains("Done"));
    }

    /// Simulates gathering facts
    #[tokio::test]
    async fn test_pilot_workflow_gather_facts() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        // Gather various system facts
        let commands = vec![
            "uname -a",
            "cat /etc/os-release 2>/dev/null || echo 'N/A'",
            "whoami",
            "id",
            "pwd",
        ];

        for cmd in commands {
            let params = ExecuteParams {
                command: cmd.to_string(),
                cwd: Some("/tmp".to_string()),
                env: HashMap::new(),
                timeout: Some(30),
                user: None,
                group: None,
                shell: true,
            };

            let result = runtime.execute(params).await;
            assert!(result.is_ok(), "Command '{}' should succeed", cmd);
        }

        let status = runtime.status();
        assert_eq!(status.tasks_executed, 5);
    }

    /// Simulates package-like operations (file manipulation)
    #[tokio::test]
    async fn test_pilot_workflow_file_operations() {
        let runtime = AgentRuntime::new(AgentConfig::default());

        // Create a test file
        let params = ExecuteParams {
            command: "echo 'test content' > /tmp/rustible-pilot-file.txt".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().exit_code, 0);

        // Read the file
        let params = ExecuteParams {
            command: "cat /tmp/rustible-pilot-file.txt".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let result = runtime.execute(params).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(result.stdout.contains("test content"));

        // Modify the file
        let params = ExecuteParams {
            command: "echo 'appended' >> /tmp/rustible-pilot-file.txt".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let result = runtime.execute(params).await;
        assert!(result.is_ok());

        // Check file permissions
        let params = ExecuteParams {
            command: "ls -la /tmp/rustible-pilot-file.txt".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let result = runtime.execute(params).await;
        assert!(result.is_ok());

        // Cleanup
        let params = ExecuteParams {
            command: "rm -f /tmp/rustible-pilot-file.txt".to_string(),
            cwd: Some("/tmp".to_string()),
            env: HashMap::new(),
            timeout: Some(30),
            user: None,
            group: None,
            shell: true,
        };
        let _ = runtime.execute(params).await;
    }
}

// ============================================================================
// Checksum and Integrity Tests
// ============================================================================

mod integrity_tests {
    #[allow(unused_imports)]
    use super::*;
    use std::io::Write;

    #[test]
    fn test_compute_checksum() {
        // Create a temp file with known content
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("rustible-checksum-test.txt");

        let mut file = std::fs::File::create(&test_file).expect("Failed to create test file");
        file.write_all(b"test content for checksum")
            .expect("Failed to write");
        drop(file);

        let checksum = rustible::agent::compute_checksum(&test_file);
        assert!(checksum.is_ok());
        let checksum = checksum.unwrap();
        assert!(!checksum.is_empty());
        assert_eq!(checksum.len(), 64); // SHA256 produces 64 hex characters

        // Cleanup
        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_verify_checksum_success() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("rustible-verify-test.txt");

        let mut file = std::fs::File::create(&test_file).expect("Failed to create test file");
        file.write_all(b"verify this content")
            .expect("Failed to write");
        drop(file);

        let expected = rustible::agent::compute_checksum(&test_file).unwrap();
        let result = rustible::agent::verify_checksum(&test_file, &expected);
        assert!(result.is_ok());

        // Cleanup
        std::fs::remove_file(&test_file).ok();
    }

    #[test]
    fn test_verify_checksum_failure() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("rustible-verify-fail-test.txt");

        let mut file = std::fs::File::create(&test_file).expect("Failed to create test file");
        file.write_all(b"some content").expect("Failed to write");
        drop(file);

        let result = rustible::agent::verify_checksum(&test_file, "invalid_checksum");
        assert!(result.is_err());

        // Cleanup
        std::fs::remove_file(&test_file).ok();
    }
}
