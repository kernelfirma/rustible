//! Rustible agent binary.
//!
//! This provides a minimal one-shot JSON request interface for executing commands
//! on the target. It is intentionally lightweight to support agent build/deploy
//! workflows and local testing.

use clap::Parser;
use rustible::agent::{
    AgentConfig, AgentMethod, AgentRequest, AgentResponse, AgentRpcError, AgentRuntime,
    ExecuteParams,
};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "rustible-agent", version, about = "Rustible agent runtime")]
struct Args {
    /// Execute a command (one-shot)
    #[arg(long)]
    command: Option<String>,

    /// Working directory for --command
    #[arg(long)]
    cwd: Option<String>,

    /// Environment variables for --command (KEY=VALUE)
    #[arg(long, action = clap::ArgAction::Append)]
    env: Vec<String>,

    /// Timeout in seconds for --command
    #[arg(long)]
    timeout: Option<u64>,

    /// Disable shell wrapping for --command
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    shell: bool,

    /// JSON request payload (AgentRequest)
    #[arg(long)]
    request: Option<String>,

    /// Read JSON request payload from file
    #[arg(long)]
    request_file: Option<PathBuf>,

    /// Emit agent status as JSON
    #[arg(long)]
    status: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let runtime = AgentRuntime::new(AgentConfig::default());

    if args.status {
        let status = runtime.status();
        println!("{}", serde_json::to_string(&status)?);
        return Ok(());
    }

    if let Some(request) = load_request(&args)? {
        let response = handle_request(&runtime, request).await;
        println!("{}", serde_json::to_string(&response)?);
        return Ok(());
    }

    if let Some(command) = args.command {
        let params = ExecuteParams {
            command,
            cwd: args.cwd,
            env: parse_env(args.env),
            timeout: args.timeout,
            user: None,
            group: None,
            shell: args.shell,
        };

        match runtime.execute(params).await {
            Ok(result) => {
                println!("{}", serde_json::to_string(&result)?);
                Ok(())
            }
            Err(err) => {
                let response = AgentResponse {
                    id: "local".to_string(),
                    result: None,
                    error: Some(AgentRpcError {
                        code: -1,
                        message: err.to_string(),
                        data: None,
                    }),
                };
                println!("{}", serde_json::to_string(&response)?);
                Ok(())
            }
        }
    } else {
        eprintln!("No request provided. Use --command or --request.");
        std::process::exit(1);
    }
}

fn load_request(args: &Args) -> anyhow::Result<Option<AgentRequest>> {
    if let Some(request) = &args.request {
        let parsed = serde_json::from_str::<AgentRequest>(request)?;
        return Ok(Some(parsed));
    }

    if let Some(path) = &args.request_file {
        let content = fs::read_to_string(path)?;
        let parsed = serde_json::from_str::<AgentRequest>(&content)?;
        return Ok(Some(parsed));
    }

    Ok(None)
}

async fn handle_request(runtime: &AgentRuntime, request: AgentRequest) -> AgentResponse {
    match request.method {
        AgentMethod::Execute => {
            let params = match request
                .params
                .as_ref()
                .and_then(|value| serde_json::from_value::<ExecuteParams>(value.clone()).ok())
            {
                Some(params) => params,
                None => {
                    return AgentResponse {
                        id: request.id,
                        result: None,
                        error: Some(rpc_error(-32602, "Invalid params for execute")),
                    }
                }
            };

            match runtime.execute(params).await {
                Ok(result) => AgentResponse {
                    id: request.id,
                    result: Some(json!(result)),
                    error: None,
                },
                Err(err) => AgentResponse {
                    id: request.id,
                    result: None,
                    error: Some(rpc_error(-32000, err.to_string())),
                },
            }
        }
        AgentMethod::Ping => AgentResponse {
            id: request.id,
            result: Some(json!({"ok": true})),
            error: None,
        },
        AgentMethod::Status => AgentResponse {
            id: request.id,
            result: Some(json!(runtime.status())),
            error: None,
        },
        AgentMethod::Shutdown => AgentResponse {
            id: request.id,
            result: Some(json!({"ok": true})),
            error: None,
        },
        _ => AgentResponse {
            id: request.id,
            result: None,
            error: Some(rpc_error(-32601, "Method not supported")),
        },
    }
}

fn parse_env(items: Vec<String>) -> HashMap<String, String> {
    let mut env = HashMap::new();
    for item in items {
        if let Some((key, value)) = item.split_once('=') {
            env.insert(key.to_string(), value.to_string());
        }
    }
    env
}

fn rpc_error(code: i32, message: impl Into<String>) -> AgentRpcError {
    AgentRpcError {
        code,
        message: message.into(),
        data: None,
    }
}
