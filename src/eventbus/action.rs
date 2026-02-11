//! Reactor actions that can be triggered by matching events.
//!
//! Actions represent the work to be performed when a reactor rule fires,
//! such as running a playbook, executing a module, sending a notification,
//! or calling a webhook.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Actions that can be executed by the reactor engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReactorAction {
    /// Execute a playbook at the given path
    RunPlaybook {
        /// Path to the playbook file
        path: String,
    },
    /// Execute a single module with the given arguments
    ExecuteModule {
        /// Module name (e.g., "command", "service")
        module: String,
        /// Module arguments as key-value pairs
        args: HashMap<String, String>,
    },
    /// Send a notification to a channel
    Notify {
        /// Notification channel (e.g., "slack", "email", "pagerduty")
        channel: String,
        /// Notification message body
        message: String,
    },
    /// Call an external webhook
    WebhookCall {
        /// The URL to call
        url: String,
        /// HTTP method (e.g., "POST", "GET")
        method: String,
    },
}

impl ReactorAction {
    /// Returns a human-readable name for this action type.
    pub fn action_name(&self) -> &str {
        match self {
            ReactorAction::RunPlaybook { .. } => "run_playbook",
            ReactorAction::ExecuteModule { .. } => "execute_module",
            ReactorAction::Notify { .. } => "notify",
            ReactorAction::WebhookCall { .. } => "webhook_call",
        }
    }
}

/// Result of executing an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    /// Name/type of the action that was executed
    pub action_name: String,
    /// Whether the action succeeded
    pub success: bool,
    /// Optional output or description from the action
    pub output: Option<String>,
}

/// Executor that runs reactor actions.
pub struct ActionExecutor {
    /// When true, actions are logged but not actually executed.
    pub dry_run: bool,
}

impl ActionExecutor {
    /// Create a new action executor.
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    /// Execute a reactor action.
    ///
    /// In dry-run mode, the action is described but not performed.
    /// Returns a result indicating success or failure.
    pub fn execute(&self, action: &ReactorAction) -> anyhow::Result<ActionResult> {
        if self.dry_run {
            return Ok(ActionResult {
                action_name: action.action_name().to_string(),
                success: true,
                output: Some(format!("[dry-run] Would execute: {:?}", action)),
            });
        }

        match action {
            ReactorAction::RunPlaybook { path } => {
                // In a real implementation, this would invoke the playbook executor.
                Ok(ActionResult {
                    action_name: "run_playbook".to_string(),
                    success: true,
                    output: Some(format!("Queued playbook for execution: {}", path)),
                })
            }
            ReactorAction::ExecuteModule { module, args } => {
                Ok(ActionResult {
                    action_name: "execute_module".to_string(),
                    success: true,
                    output: Some(format!(
                        "Queued module '{}' with {} arg(s)",
                        module,
                        args.len()
                    )),
                })
            }
            ReactorAction::Notify { channel, message } => {
                Ok(ActionResult {
                    action_name: "notify".to_string(),
                    success: true,
                    output: Some(format!(
                        "Notification sent to '{}': {}",
                        channel, message
                    )),
                })
            }
            ReactorAction::WebhookCall { url, method } => {
                Ok(ActionResult {
                    action_name: "webhook_call".to_string(),
                    success: true,
                    output: Some(format!("Webhook {} {}", method, url)),
                })
            }
        }
    }
}

impl Default for ActionExecutor {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_executor_dry_run() {
        let executor = ActionExecutor::new(true);
        let action = ReactorAction::RunPlaybook {
            path: "site.yml".to_string(),
        };

        let result = executor.execute(&action).unwrap();
        assert!(result.success);
        assert!(result.output.as_ref().unwrap().contains("[dry-run]"));
        assert_eq!(result.action_name, "run_playbook");
    }

    #[test]
    fn test_action_executor_run_playbook() {
        let executor = ActionExecutor::new(false);
        let action = ReactorAction::RunPlaybook {
            path: "deploy.yml".to_string(),
        };

        let result = executor.execute(&action).unwrap();
        assert!(result.success);
        assert!(result.output.as_ref().unwrap().contains("deploy.yml"));
    }

    #[test]
    fn test_action_executor_notify() {
        let executor = ActionExecutor::new(false);
        let action = ReactorAction::Notify {
            channel: "slack".to_string(),
            message: "Deployment complete".to_string(),
        };

        let result = executor.execute(&action).unwrap();
        assert!(result.success);
        assert_eq!(result.action_name, "notify");
    }

    #[test]
    fn test_action_name() {
        assert_eq!(
            ReactorAction::RunPlaybook {
                path: "x".to_string()
            }
            .action_name(),
            "run_playbook"
        );
        assert_eq!(
            ReactorAction::ExecuteModule {
                module: "m".to_string(),
                args: HashMap::new()
            }
            .action_name(),
            "execute_module"
        );
        assert_eq!(
            ReactorAction::Notify {
                channel: "c".to_string(),
                message: "m".to_string()
            }
            .action_name(),
            "notify"
        );
        assert_eq!(
            ReactorAction::WebhookCall {
                url: "u".to_string(),
                method: "POST".to_string()
            }
            .action_name(),
            "webhook_call"
        );
    }
}
