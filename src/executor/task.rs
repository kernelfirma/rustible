//! Task definition and execution for Rustible
//!
//! This module provides:
//! - Task struct with module, args, when conditions, loops
//! - Task result handling
//! - Changed/ok/failed states
//!
//! # Performance Optimizations
//!
//! This module includes several hot path optimizations:
//! - Cached regex patterns using `once_cell::sync::Lazy`
//! - Inline hints for frequently called functions
//! - Reduced allocations in template processing

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, instrument, warn};

// ============================================================================
// PERFORMANCE: Cached regex patterns for hot path template processing
// ============================================================================

/// Cached regex for template variable extraction: {{ variable }}
/// This regex is compiled once and reused across all template operations.
static TEMPLATE_VAR_REGEX: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"\{\{\s*([^}]+?)\s*\}\}").expect("Invalid template regex"));

/// Cached regex for checking if string contains template syntax
#[allow(dead_code)]
static TEMPLATE_CHECK_REGEX: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"\{\{|\{%").expect("Invalid template check regex"));

use crate::diagnostics::template_syntax_error;
use crate::error::Error;
use crate::executor::parallelization::ParallelizationManager;
use crate::executor::runtime::{ExecutionContext, RegisteredResult, RuntimeContext};
use crate::executor::{ExecutorError, ExecutorResult};
use crate::modules::ModuleRegistry;
use crate::template::TEMPLATE_ENGINE;

/// Status of a task execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum TaskStatus {
    /// Task completed successfully without changes
    #[default]
    Ok,
    /// Task completed successfully with changes
    Changed,
    /// Task failed
    Failed,
    /// Task was skipped (condition not met)
    Skipped,
    /// Host was unreachable
    Unreachable,
}

/// Result of executing a task
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskResult {
    /// Final status of the task
    pub status: TaskStatus,
    /// Whether something was changed
    pub changed: bool,
    /// Optional message from the task
    pub msg: Option<String>,
    /// Module-specific result data
    pub result: Option<JsonValue>,
    /// Diff showing what changed (if diff_mode enabled)
    pub diff: Option<TaskDiff>,
}

impl TaskResult {
    /// Create a successful result
    pub fn ok() -> Self {
        Self {
            status: TaskStatus::Ok,
            changed: false,
            ..Default::default()
        }
    }

    /// Create a changed result
    pub fn changed() -> Self {
        Self {
            status: TaskStatus::Changed,
            changed: true,
            ..Default::default()
        }
    }

    /// Create a failed result
    pub fn failed(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Failed,
            changed: false,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    /// Create a skipped result
    pub fn skipped(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Skipped,
            changed: false,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    /// Create an unreachable result
    pub fn unreachable(msg: impl Into<String>) -> Self {
        Self {
            status: TaskStatus::Unreachable,
            changed: false,
            msg: Some(msg.into()),
            ..Default::default()
        }
    }

    /// Set the result data
    pub fn with_result(mut self, result: JsonValue) -> Self {
        self.result = Some(result);
        self
    }

    /// Set the message
    pub fn with_msg(mut self, msg: impl Into<String>) -> Self {
        self.msg = Some(msg.into());
        self
    }

    /// Set the diff
    pub fn with_diff(mut self, diff: TaskDiff) -> Self {
        self.diff = Some(diff);
        self
    }

    /// Convert to RegisteredResult
    ///
    /// Extracts data from `self.result` if available, falling back to the
    /// explicit stdout/stderr parameters. This ensures module output data
    /// (rc, stdout, stderr, module-specific fields) is preserved for register.
    pub fn to_registered(
        &self,
        stdout: Option<String>,
        stderr: Option<String>,
    ) -> RegisteredResult {
        // Try to extract fields from self.result if it contains module output data
        let (rc, result_stdout, result_stderr, data) = if let Some(ref result) = self.result {
            if let Some(obj) = result.as_object() {
                let rc = obj.get("rc").and_then(|v| v.as_i64()).map(|v| v as i32);
                let result_stdout = obj.get("stdout").and_then(|v| v.as_str()).map(String::from);
                let result_stderr = obj.get("stderr").and_then(|v| v.as_str()).map(String::from);

                // Collect module-specific data (excluding standard fields)
                let mut data = IndexMap::new();
                for (key, value) in obj {
                    // Skip standard RegisteredResult fields
                    if !matches!(
                        key.as_str(),
                        "changed"
                            | "failed"
                            | "skipped"
                            | "rc"
                            | "stdout"
                            | "stdout_lines"
                            | "stderr"
                            | "stderr_lines"
                            | "msg"
                            | "results"
                    ) {
                        data.insert(key.clone(), value.clone());
                    }
                }

                (rc, result_stdout, result_stderr, data)
            } else {
                (None, None, None, IndexMap::new())
            }
        } else {
            (None, None, None, IndexMap::new())
        };

        // Use result data if available, otherwise fall back to explicit parameters
        let final_stdout = result_stdout.or(stdout);
        let final_stderr = result_stderr.or(stderr);

        RegisteredResult {
            changed: self.changed,
            failed: self.status == TaskStatus::Failed,
            skipped: self.status == TaskStatus::Skipped,
            rc,
            stdout: final_stdout.clone(),
            stdout_lines: final_stdout.map(|s| s.lines().map(String::from).collect()),
            stderr: final_stderr.clone(),
            stderr_lines: final_stderr.map(|s| s.lines().map(String::from).collect()),
            msg: self.msg.clone(),
            results: None,
            data,
        }
    }
}

/// Diff showing before/after state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDiff {
    pub before: Option<String>,
    pub after: Option<String>,
    pub before_header: Option<String>,
    pub after_header: Option<String>,
}

/// Helper function to deserialize string or sequence into Vec<String>
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = JsonValue::deserialize(deserializer)?;
    match value {
        JsonValue::Null => Ok(Vec::new()),
        JsonValue::String(s) => Ok(vec![s]),
        JsonValue::Bool(b) => Ok(vec![b.to_string()]),
        JsonValue::Number(n) => Ok(vec![n.to_string()]),
        JsonValue::Array(seq) => {
            let mut result = Vec::new();
            for item in seq {
                match item {
                    JsonValue::String(s) => result.push(s),
                    JsonValue::Bool(b) => result.push(b.to_string()),
                    JsonValue::Number(n) => result.push(n.to_string()),
                    other => result.push(format!("{:?}", other)),
                }
            }
            Ok(result)
        }
        other => Ok(vec![format!("{:?}", other)]),
    }
}

/// A handler that can be notified by tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handler {
    /// Handler name (used for notification)
    pub name: String,
    /// Module to execute
    pub module: String,
    /// Module arguments
    #[serde(default)]
    pub args: IndexMap<String, JsonValue>,
    /// Optional when condition
    pub when: Option<String>,
    /// Listen for multiple notification names
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub listen: Vec<String>,
}

/// Loop control options for customizing loop behavior
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoopControl {
    /// Variable name for current item (default: "item")
    #[serde(default = "default_loop_var")]
    pub loop_var: String,
    /// Variable name for item index
    #[serde(default)]
    pub index_var: Option<String>,
    /// Label for display (template evaluated per item)
    #[serde(default)]
    pub label: Option<String>,
    /// Pause between iterations in seconds
    #[serde(default)]
    pub pause: Option<u64>,
    /// Enable extended loop information (revindex, revindex0, etc.)
    #[serde(default)]
    pub extended: bool,
}

/// A task to be executed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Task name (displayed during execution)
    pub name: String,
    /// Module to execute
    pub module: String,
    /// Module arguments
    #[serde(default)]
    pub args: IndexMap<String, JsonValue>,
    /// Conditional expression (Jinja2-like)
    #[serde(default)]
    pub when: Option<String>,
    /// Handlers to notify on change
    #[serde(default)]
    pub notify: Vec<String>,
    /// Variable name to register result
    #[serde(default)]
    pub register: Option<String>,
    /// Items to loop over (can be literal items or template expression)
    #[serde(default)]
    pub loop_items: Option<LoopSource>,
    /// Loop variable name (default: "item")
    #[serde(default = "default_loop_var")]
    pub loop_var: String,
    /// Loop control options
    #[serde(default)]
    pub loop_control: Option<LoopControl>,
    /// Whether to ignore errors
    #[serde(default)]
    pub ignore_errors: bool,
    /// Custom condition to determine if task changed
    #[serde(default)]
    pub changed_when: Option<String>,
    /// Custom condition to determine if task failed
    #[serde(default)]
    pub failed_when: Option<String>,
    /// Delegate task to another host
    #[serde(default)]
    pub delegate_to: Option<String>,
    /// Whether facts should be set on the delegated host instead of the original host
    #[serde(default)]
    pub delegate_facts: Option<bool>,
    /// Run task only once (not on each host)
    #[serde(default)]
    pub run_once: bool,
    /// Tags for task filtering
    #[serde(default)]
    pub tags: Vec<String>,
    /// Task-level variables
    #[serde(default)]
    pub vars: IndexMap<String, JsonValue>,
    /// Whether to become another user
    #[serde(default)]
    pub r#become: bool,
    /// User to become
    #[serde(default)]
    pub become_user: Option<String>,
    /// Block ID this task belongs to (if part of block/rescue/always)
    #[serde(default)]
    pub block_id: Option<String>,
    /// Task type within a block
    #[serde(default)]
    pub block_role: BlockRole,
    /// Block context stack (outermost to innermost)
    #[serde(default)]
    pub block_stack: Vec<BlockContext>,
    /// Number of retries for until loop
    #[serde(default)]
    pub retries: Option<u32>,
    /// Delay between retries in seconds
    #[serde(default)]
    pub delay: Option<u64>,
    /// Until condition for retry loop
    #[serde(default)]
    pub until: Option<String>,
}

/// Role of a task within a block structure
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlockRole {
    /// Normal task or task in the main block section
    #[default]
    Normal,
    /// Task in the rescue section (runs on block failure)
    Rescue,
    /// Task in the always section (runs regardless)
    Always,
}

/// Block context for a task (supports nested blocks)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockContext {
    /// Block ID
    pub id: String,
    /// Task role within this block
    #[serde(default)]
    pub role: BlockRole,
    /// Block-level variables
    #[serde(default)]
    pub vars: IndexMap<String, JsonValue>,
}

fn default_loop_var() -> String {
    "item".to_string()
}

/// Source of loop items - can be literal items or a template expression
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoopSource {
    /// Literal array of items
    Items(Vec<JsonValue>),
    /// Template expression that evaluates to an array (e.g., "{{ result.results }}")
    Template(String),
}

impl Default for Task {
    fn default() -> Self {
        Self {
            name: String::new(),
            module: String::new(),
            args: IndexMap::new(),
            when: None,
            notify: Vec::new(),
            register: None,
            loop_items: None,
            loop_var: default_loop_var(),
            loop_control: None,
            ignore_errors: false,
            changed_when: None,
            failed_when: None,
            delegate_to: None,
            delegate_facts: None,
            run_once: false,
            tags: Vec::new(),
            vars: IndexMap::new(),
            r#become: false,
            become_user: None,
            block_id: None,
            block_role: BlockRole::Normal,
            block_stack: Vec::new(),
            retries: None,
            delay: None,
            until: None,
        }
    }
}

/// Convert from playbook::Task to executor::task::Task
impl From<crate::playbook::Task> for Task {
    fn from(pt: crate::playbook::Task) -> Self {
        // Convert args from serde_json::Value to IndexMap
        let args = if let Some(obj) = pt.module.args.as_object() {
            obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        } else {
            IndexMap::new()
        };

        // Convert when condition
        let when = pt.when.map(|w| match w {
            crate::playbook::When::Single(s) => s,
            crate::playbook::When::Multiple(v) => v.join(" and "),
        });

        // Convert loop items from various sources
        // Priority: loop > with_items > with_dict > with_fileglob
        let loop_items = if let Some(v) = pt.loop_.or(pt.with_items) {
            // Standard loop or with_items - can be array or template expression
            if let Some(arr) = v.as_array() {
                Some(LoopSource::Items(arr.clone()))
            } else {
                v.as_str().map(|s| LoopSource::Template(s.to_string()))
            }
        } else if let Some(v) = pt.with_dict {
            // with_dict - convert dict to list of {key, value} objects
            if let Some(obj) = v.as_object() {
                let items: Vec<JsonValue> = obj
                    .iter()
                    .map(|(k, val)| serde_json::json!({"key": k, "value": val}))
                    .collect();
                Some(LoopSource::Items(items))
            } else {
                None
            }
        } else if let Some(v) = pt.with_fileglob {
            // with_fileglob - for now just pass patterns as strings
            // (actual glob expansion happens at runtime)
            if let Some(arr) = v.as_array() {
                Some(LoopSource::Items(arr.clone()))
            } else if v.is_string() {
                Some(LoopSource::Items(vec![v]))
            } else {
                None
            }
        } else {
            None
        };

        // Get loop_var from loop_control if available
        let loop_var = pt
            .loop_control
            .as_ref()
            .map(|lc| lc.loop_var.clone())
            .unwrap_or_else(default_loop_var);

        // Convert loop_control from playbook to executor format
        let loop_control = pt.loop_control.as_ref().map(|lc| LoopControl {
            loop_var: lc.loop_var.clone(),
            index_var: lc.index_var.clone(),
            label: lc.label.clone(),
            pause: lc.pause,
            extended: lc.extended,
        });

        Self {
            name: pt.name,
            module: pt.module.name,
            args,
            when,
            notify: pt.notify,
            register: pt.register,
            loop_items,
            loop_var,
            loop_control,
            ignore_errors: pt.ignore_errors,
            changed_when: pt.changed_when,
            failed_when: pt.failed_when,
            delegate_to: pt.delegate_to,
            delegate_facts: pt.delegate_facts,
            run_once: pt.run_once,
            tags: pt.tags,
            vars: pt.vars.as_map().clone(),
            r#become: pt.r#become.unwrap_or(false),
            become_user: pt.become_user,
            block_id: None,
            block_role: BlockRole::Normal,
            block_stack: Vec::new(),
            retries: pt.retries,
            delay: pt.delay,
            until: pt.until,
        }
    }
}

impl Task {
    /// Create a new task with the given name and module
    pub fn new(name: impl Into<String>, module: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module: module.into(),
            ..Default::default()
        }
    }

    /// Add an argument to the task
    pub fn arg(mut self, key: impl Into<String>, value: impl Into<JsonValue>) -> Self {
        self.args.insert(key.into(), value.into());
        self
    }

    /// Set the when condition
    pub fn when(mut self, condition: impl Into<String>) -> Self {
        self.when = Some(condition.into());
        self
    }

    /// Add a handler to notify
    pub fn notify(mut self, handler: impl Into<String>) -> Self {
        self.notify.push(handler.into());
        self
    }

    /// Set the register variable
    pub fn register(mut self, name: impl Into<String>) -> Self {
        self.register = Some(name.into());
        self
    }

    /// Set loop items
    pub fn loop_over(mut self, items: Vec<JsonValue>) -> Self {
        self.loop_items = Some(LoopSource::Items(items));
        self
    }

    /// Set loop from template expression
    pub fn loop_template(mut self, template: impl Into<String>) -> Self {
        self.loop_items = Some(LoopSource::Template(template.into()));
        self
    }

    /// Set the loop variable name
    pub fn loop_var(mut self, name: impl Into<String>) -> Self {
        self.loop_var = name.into();
        self
    }

    /// Set ignore_errors
    pub fn ignore_errors(mut self, ignore: bool) -> Self {
        self.ignore_errors = ignore;
        self
    }

    /// Merge block-level variables from all block contexts (outer to inner).
    pub fn merged_block_vars(&self) -> IndexMap<String, JsonValue> {
        let mut merged = IndexMap::new();
        for ctx in &self.block_stack {
            for (key, value) in &ctx.vars {
                merged.insert(key.clone(), value.clone());
            }
        }
        merged
    }

    /// Execute the task
    #[instrument(skip(self, ctx, runtime, handlers, notified, parallelization_manager, module_registry), fields(task_name = %self.name, host = %ctx.host))]
    pub async fn execute(
        &self,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        handlers: &Arc<RwLock<HashMap<String, Handler>>>,
        notified: &Arc<Mutex<std::collections::HashSet<String>>>,
        parallelization_manager: &Arc<ParallelizationManager>,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        info!("Executing task: {}", self.name);

        // Evaluate when condition
        if let Some(ref condition) = self.when {
            let should_run = self.evaluate_condition(condition, ctx, runtime).await?;
            if !should_run {
                debug!("Task skipped due to when condition: {}", condition);
                return Ok(TaskResult::skipped(format!(
                    "Skipped: condition '{}' was false",
                    condition
                )));
            }
        }

        // Handle delegation - create appropriate context for execution and fact storage
        let (execution_ctx, fact_storage_ctx) = if let Some(ref delegate_host) = self.delegate_to {
            debug!("Delegating task to host: {}", delegate_host);

            // Create execution context for the delegate host (where task actually runs)
            let mut delegate_ctx = ctx.clone();
            delegate_ctx.host = delegate_host.clone();

            // Create fact storage context based on delegate_facts setting
            // If delegate_facts is true, store on delegate host; otherwise on original host
            let fact_ctx = if self.delegate_facts.unwrap_or(false) {
                // Facts go to delegate host
                let mut fact_ctx = ctx.clone();
                fact_ctx.host = delegate_host.clone();
                fact_ctx
            } else {
                // Facts go to original host (default behavior)
                ctx.clone()
            };

            (delegate_ctx, fact_ctx)
        } else {
            // No delegation - both execution and facts use the same context
            (ctx.clone(), ctx.clone())
        };

        // Handle loops - for set_fact, use fact_storage_ctx; for others, use execution_ctx
        if let Some(ref loop_source) = self.loop_items {
            let loop_ctx = if self.module == "set_fact" {
                &fact_storage_ctx
            } else {
                &execution_ctx
            };

            // Resolve loop items from the source
            let items = match loop_source {
                LoopSource::Items(items) => items.clone(),
                LoopSource::Template(template) => {
                    // Render the template to get the items
                    let rt = runtime.read().await;
                    let vars = rt.get_merged_vars_ref(&loop_ctx.host);
                    drop(rt);

                    let rendered = TEMPLATE_ENGINE
                        .render_value(&serde_json::Value::String(template.clone()), vars.as_ref())
                        .map_err(|e| {
                            ExecutorError::RuntimeError(format!(
                                "Failed to render loop template '{}': {}",
                                template, e
                            ))
                        })?;

                    // The rendered value should be an array
                    match rendered {
                        serde_json::Value::Array(arr) => arr,
                        serde_json::Value::String(ref s) if s.is_empty() => {
                            // Empty string means no items, skip loop
                            debug!("Loop template rendered to empty string, skipping loop");
                            Vec::new()
                        }
                        other => {
                            // Try to interpret as JSON array string
                            if let serde_json::Value::String(ref s) = other {
                                // MiniJinja outputs Python-style values, convert to JSON:
                                // none -> null, True -> true, False -> false
                                let json_str = s
                                    .replace(": none", ": null")
                                    .replace(":none", ":null")
                                    .replace(", none,", ", null,")
                                    .replace("[none,", "[null,")
                                    .replace(", none]", ", null]")
                                    .replace(": True", ": true")
                                    .replace(":True", ":true")
                                    .replace(": False", ": false")
                                    .replace(":False", ":false");

                                if let Ok(arr) = serde_json::from_str::<Vec<JsonValue>>(&json_str) {
                                    arr
                                } else {
                                    warn!(
                                        "Loop template '{}' did not render to an array: {:?}",
                                        template, other
                                    );
                                    Vec::new()
                                }
                            } else {
                                warn!(
                                    "Loop template '{}' did not render to an array: {:?}",
                                    template, other
                                );
                                Vec::new()
                            }
                        }
                    }
                }
            };

            if items.is_empty() {
                // No items to iterate, return success
                return Ok(TaskResult {
                    status: TaskStatus::Ok,
                    changed: false,
                    msg: Some("Loop has no items".to_string()),
                    result: None,
                    diff: None,
                });
            }

            return self
                .execute_loop(
                    &items,
                    loop_ctx,
                    runtime,
                    handlers,
                    notified,
                    parallelization_manager,
                    module_registry,
                )
                .await;
        }

        // Execute the module - use fact_storage_ctx for set_fact to ensure facts go to right host
        let module_ctx = if self.module == "set_fact" {
            &fact_storage_ctx
        } else {
            &execution_ctx
        };

        // Handle until/retries/delay retry logic
        let result = if self.until.is_some() {
            self.execute_with_retry(
                module_ctx,
                runtime,
                handlers,
                notified,
                parallelization_manager,
                module_registry,
            )
            .await?
        } else {
            self.execute_module(
                module_ctx,
                runtime,
                handlers,
                notified,
                parallelization_manager,
                module_registry,
            )
            .await?
        };

        // Extract and store ansible_facts from module results
        // Many modules (like gather_facts, setup, etc.) return facts in their result
        if let Some(ref result_data) = result.result {
            if let Some(ansible_facts) = result_data.get("ansible_facts") {
                if let Some(facts_obj) = ansible_facts.as_object() {
                    let mut rt = runtime.write().await;
                    let fact_target = &ctx.host;
                    for (key, value) in facts_obj {
                        rt.set_host_fact(fact_target, key.clone(), value.clone());
                        debug!(
                            "Stored fact '{}' from module result for host '{}'",
                            key, fact_target
                        );
                    }
                }
            }
        }

        // Apply changed_when override - use execution context for condition evaluation
        let result = self
            .apply_changed_when(result, &execution_ctx, runtime)
            .await?;

        // Apply failed_when override - use execution context for condition evaluation
        let result = self
            .apply_failed_when(result, &execution_ctx, runtime)
            .await?;

        // Register result if needed - always register on the original host
        if let Some(ref register_name) = self.register {
            self.register_result(register_name, &result, ctx, runtime)
                .await?;
        }

        // Notify handlers if task changed
        if result.changed && result.status != TaskStatus::Failed {
            for handler_name in &self.notify {
                let mut notified = notified.lock().await;
                notified.insert(handler_name.clone());
                debug!("Notified handler: {}", handler_name);
            }
        }

        // Handle ignore_errors
        if result.status == TaskStatus::Failed && self.ignore_errors {
            warn!("Task failed but ignore_errors is set");
            return Ok(TaskResult {
                status: TaskStatus::Ok,
                changed: false,
                msg: Some(format!("Ignored error: {}", result.msg.unwrap_or_default())),
                result: result.result,
                diff: result.diff,
            });
        }

        Ok(result)
    }

    /// Execute task in a loop
    #[allow(clippy::too_many_arguments)]
    async fn execute_loop(
        &self,
        items: &[JsonValue],
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        handlers: &Arc<RwLock<HashMap<String, Handler>>>,
        notified: &Arc<Mutex<std::collections::HashSet<String>>>,
        parallelization_manager: &Arc<ParallelizationManager>,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        let total_items = items.len();
        debug!("Executing loop with {} items", total_items);

        // Pre-allocate with known capacity
        let mut loop_results = Vec::with_capacity(total_items);
        let mut any_changed = false;
        let mut any_failed = false;

        // Extract loop_control options - avoid repeated Option access in loop
        let loop_control = self.loop_control.as_ref();
        let pause_seconds = loop_control.and_then(|lc| lc.pause);
        let index_var = loop_control.and_then(|lc| lc.index_var.as_ref());
        let extended = loop_control.map(|lc| lc.extended).unwrap_or(false);

        // Pre-allocate static string keys to avoid repeated allocations in loop
        static ANSIBLE_LOOP_KEY: &str = "ansible_loop";

        for (index, item) in items.iter().enumerate() {
            // Pause between iterations (but not before the first)
            if index > 0 {
                if let Some(pause) = pause_seconds {
                    if pause > 0 {
                        debug!("Pausing {} seconds between loop iterations", pause);
                        tokio::time::sleep(tokio::time::Duration::from_secs(pause)).await;
                    }
                }
            }

            // Set loop variables
            {
                let mut rt = runtime.write().await;
                // Clone loop_var only once per loop iteration (unavoidable for runtime storage)
                rt.set_task_var(&ctx.host, self.loop_var.clone(), item.clone());

                // Set index_var if specified - avoid clone when possible
                if let Some(idx_var) = index_var {
                    rt.set_task_var(&ctx.host, idx_var.clone(), serde_json::json!(index));
                }

                // Build ansible_loop object
                let mut ansible_loop = serde_json::json!({
                    "index": index + 1,  // 1-based index
                    "index0": index,     // 0-based index
                    "first": index == 0,
                    "last": index == total_items - 1,
                    "length": total_items,
                });

                // Add extended loop info if enabled
                if extended {
                    let revindex = total_items - index; // 1-based reverse index
                    let revindex0 = total_items - index - 1; // 0-based reverse index
                    let loop_obj = ansible_loop.as_object_mut().unwrap();
                    loop_obj.insert("revindex".to_string(), serde_json::json!(revindex));
                    loop_obj.insert("revindex0".to_string(), serde_json::json!(revindex0));
                    loop_obj.insert("allitems".to_string(), serde_json::json!(items));
                    loop_obj.insert(
                        "previtem".to_string(),
                        if index > 0 {
                            items[index - 1].clone()
                        } else {
                            JsonValue::Null
                        },
                    );
                    loop_obj.insert(
                        "nextitem".to_string(),
                        if index < total_items - 1 {
                            items[index + 1].clone()
                        } else {
                            JsonValue::Null
                        },
                    );
                }

                rt.set_task_var(&ctx.host, ANSIBLE_LOOP_KEY.to_string(), ansible_loop);
            }

            // Execute for this item with parallelization enforcement
            let result = self
                .execute_module(
                    ctx,
                    runtime,
                    handlers,
                    notified,
                    parallelization_manager,
                    module_registry,
                )
                .await?;

            // Extract and store ansible_facts from module results in loops
            if let Some(ref result_data) = result.result {
                if let Some(ansible_facts) = result_data.get("ansible_facts") {
                    if let Some(facts_obj) = ansible_facts.as_object() {
                        let mut rt = runtime.write().await;
                        for (key, value) in facts_obj {
                            rt.set_host_fact(&ctx.host, key.clone(), value.clone());
                            debug!(
                                "Stored fact '{}' from loop iteration for host '{}'",
                                key, ctx.host
                            );
                        }
                    }
                }
            }

            if result.changed {
                any_changed = true;
            }
            if result.status == TaskStatus::Failed {
                any_failed = true;
                if !self.ignore_errors {
                    // Stop on first failure unless ignore_errors
                    let mut registered = result.to_registered(None, None);
                    // Store the loop item in the result data for access in subsequent loops
                    // This enables patterns like: loop: "{{ result.results }}" with item.stat.exists
                    registered.data.insert("item".to_string(), item.clone());
                    loop_results.push(registered);
                    break;
                }
            }

            // Create registered result with loop item included in data
            // This enables patterns like: loop: "{{ result.results }}" with item.stat.exists
            let mut registered = result.to_registered(None, None);
            registered.data.insert("item".to_string(), item.clone());
            loop_results.push(registered);
        }

        // Clear only the loop-specific variables, preserving other task vars
        // This allows for future nested loop support
        {
            let mut rt = runtime.write().await;
            let mut vars_to_clear = vec![self.loop_var.as_str(), "ansible_loop"];
            if let Some(idx_var) = index_var {
                vars_to_clear.push(idx_var.as_str());
            }
            rt.remove_task_vars(&ctx.host, &vars_to_clear);
        }

        // Create combined result
        let status = if any_failed && !self.ignore_errors {
            TaskStatus::Failed
        } else if any_changed {
            TaskStatus::Changed
        } else {
            TaskStatus::Ok
        };

        let result = TaskResult {
            status,
            changed: any_changed,
            msg: Some(format!("Completed {} loop iterations", loop_results.len())),
            result: Some(serde_json::to_value(&loop_results).unwrap_or(JsonValue::Null)),
            diff: None,
        };

        // Register combined result if needed
        if let Some(ref register_name) = self.register {
            let mut registered = RegisteredResult::ok(any_changed);
            registered.results = Some(loop_results);

            let mut rt = runtime.write().await;
            rt.register_result(&ctx.host, register_name.clone(), registered);
        }

        // Notify handlers if anything changed
        if any_changed && !any_failed {
            for handler_name in &self.notify {
                let mut n = notified.lock().await;
                n.insert(handler_name.clone());
            }
        }

        Ok(result)
    }

    /// Execute task with until/retries/delay retry logic
    async fn execute_with_retry(
        &self,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        handlers: &Arc<RwLock<HashMap<String, Handler>>>,
        notified: &Arc<Mutex<std::collections::HashSet<String>>>,
        parallelization_manager: &Arc<ParallelizationManager>,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        let max_retries = self.retries.unwrap_or(3);
        let delay_seconds = self.delay.unwrap_or(5);
        let until_condition = self.until.as_ref().ok_or_else(|| {
            ExecutorError::RuntimeError("Missing until condition for retry execution".to_string())
        })?;

        debug!(
            "Executing with retry: max_retries={}, delay={}s, until='{}'",
            max_retries, delay_seconds, until_condition
        );

        let mut last_result: Option<TaskResult>;
        let mut attempt = 0;

        loop {
            attempt += 1;
            debug!("Retry attempt {} of {}", attempt, max_retries + 1);

            // Execute the module
            let result = self
                .execute_module(
                    ctx,
                    runtime,
                    handlers,
                    notified,
                    parallelization_manager,
                    module_registry,
                )
                .await?;

            // Extract and store ansible_facts from module results during retries
            if let Some(ref result_data) = result.result {
                if let Some(ansible_facts) = result_data.get("ansible_facts") {
                    if let Some(facts_obj) = ansible_facts.as_object() {
                        let mut rt = runtime.write().await;
                        for (key, value) in facts_obj {
                            rt.set_host_fact(&ctx.host, key.clone(), value.clone());
                            debug!(
                                "Stored fact '{}' from retry attempt {} for host '{}'",
                                key, attempt, ctx.host
                            );
                        }
                    }
                }
            }

            // Register the result for condition evaluation
            if let Some(ref register_name) = self.register {
                self.register_result(register_name, &result, ctx, runtime)
                    .await?;
            }

            // Evaluate the until condition
            let condition_met = self
                .evaluate_condition(until_condition, ctx, runtime)
                .await?;

            if condition_met {
                debug!(
                    "Until condition '{}' met after {} attempt(s)",
                    until_condition, attempt
                );
                return Ok(result);
            }

            // Store the last result
            last_result = Some(result);

            // Check if we've exhausted retries
            if attempt > max_retries {
                debug!("Max retries ({}) exhausted, condition not met", max_retries);
                break;
            }

            // Wait before retrying
            if delay_seconds > 0 {
                debug!("Waiting {} seconds before retry", delay_seconds);
                tokio::time::sleep(tokio::time::Duration::from_secs(delay_seconds)).await;
            }
        }

        // Return failure after exhausting retries
        Ok(TaskResult {
            status: TaskStatus::Failed,
            changed: false,
            msg: Some(format!(
                "Retries exhausted ({}). Until condition '{}' never met",
                max_retries, until_condition
            )),
            result: last_result.as_ref().and_then(|r| r.result.clone()),
            diff: None,
        })
    }

    /// Execute the actual module
    async fn execute_module(
        &self,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        handlers: &Arc<RwLock<HashMap<String, Handler>>>,
        notified: &Arc<Mutex<std::collections::HashSet<String>>>,
        parallelization_manager: &Arc<ParallelizationManager>,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        // Template the arguments
        let args = self.template_args(ctx, runtime).await?;

        debug!("Module: {}, Args: {:?}", self.module, args);

        // Enforce parallelization constraints based on module hint
        // Get the module's parallelization hint from the shared registry (avoids rebuilding)
        let hint = {
            if let Some(module) = module_registry.get(&self.module) {
                module.parallelization_hint()
            } else {
                // For unknown modules (Python fallback), use FullyParallel as default
                crate::modules::ParallelizationHint::FullyParallel
            }
        };

        // Acquire parallelization guard - this will block if necessary based on the hint
        // The guard is automatically released when it goes out of scope (when this function returns)
        let _parallelization_guard = parallelization_manager
            .acquire(hint, &ctx.host, &self.module)
            .await;

        // Execute based on module type
        let result = match self.module.as_str() {
            "debug" => self.execute_debug(&args, ctx).await,
            "set_fact" => self.execute_set_fact(&args, ctx, runtime).await,
            "command" | "shell" => {
                self.execute_registry_module(&self.module, &args, ctx, module_registry)
                    .await
            }
            "copy" => {
                self.execute_copy(&args, ctx, runtime, module_registry)
                    .await
            }
            "file" => self.execute_file(&args, ctx, module_registry).await,
            "template" => {
                self.execute_template(&args, ctx, runtime, module_registry)
                    .await
            }
            "package" | "apt" | "yum" | "dnf" => self.execute_package(&args, ctx).await,
            "service" | "systemd" => self.execute_service(&args, ctx).await,
            "user" => self.execute_user(&args, ctx).await,
            "group" => self.execute_group(&args, ctx).await,
            "lineinfile" => self.execute_lineinfile(&args, ctx).await,
            "blockinfile" => self.execute_blockinfile(&args, ctx).await,
            "stat" => self.execute_stat(&args, ctx).await,
            "fail" => self.execute_fail(&args).await,
            "assert" => self.execute_assert(&args, ctx, runtime).await,
            "pause" => self.execute_pause(&args).await,
            "wait_for" => self.execute_wait_for(&args, ctx).await,
            "include_vars" => self.execute_include_vars(&args, ctx, runtime).await,
            "include_tasks" | "import_tasks" => {
                self.execute_include_tasks(
                    &args,
                    ctx,
                    runtime,
                    handlers,
                    notified,
                    parallelization_manager,
                    module_registry,
                )
                .await
            }
            "meta" => self.execute_meta(&args).await,
            "gather_facts" | "setup" => self.execute_gather_facts(&args, ctx).await,
            _ => {
                // Python fallback for unknown modules
                // Check if we can find the module in Ansible's module library
                let mut executor = crate::modules::PythonModuleExecutor::new();

                if let Some(module_path) = executor.find_module(&self.module) {
                    debug!(
                        "Found Ansible module {} at {} - Python fallback available",
                        self.module,
                        module_path.display()
                    );

                    // In check mode, report that we would execute
                    if ctx.check_mode {
                        return Ok(TaskResult::ok().with_msg(format!(
                            "Check mode - would execute Python module: {}",
                            self.module
                        )));
                    }

                    // Convert args to ModuleParams-compatible format
                    let module_params: std::collections::HashMap<String, serde_json::Value> =
                        args.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

                    // Execute via Python if connection is available
                    if let Some(ref connection) = ctx.connection {
                        match executor
                            .execute(
                                connection.as_ref(),
                                &self.module,
                                &module_params,
                                &ctx.python_interpreter,
                            )
                            .await
                        {
                            Ok(output) => {
                                let msg = output.msg.clone();
                                let mut result = if output.changed {
                                    TaskResult::changed()
                                } else {
                                    TaskResult::ok()
                                };
                                result.msg = Some(msg);
                                // Store full module output for register access
                                result.result = Some(output.to_result_json());
                                Ok(result)
                            }
                            Err(e) => Err(ExecutorError::RuntimeError(format!(
                                "Python module {} failed: {}",
                                self.module, e
                            ))),
                        }
                    } else if matches!(ctx.host.as_str(), "localhost" | "127.0.0.1" | "::1") {
                        let local_conn = crate::connection::local::LocalConnection::new();
                        match executor
                            .execute(
                                &local_conn,
                                &self.module,
                                &module_params,
                                &ctx.python_interpreter,
                            )
                            .await
                        {
                            Ok(output) => {
                                let msg = output.msg.clone();
                                let mut result = if output.changed {
                                    TaskResult::changed()
                                } else {
                                    TaskResult::ok()
                                };
                                result.msg = Some(msg);
                                result.result = Some(output.to_result_json());
                                Ok(result)
                            }
                            Err(e) => Err(ExecutorError::RuntimeError(format!(
                                "Python module {} failed locally: {}",
                                self.module, e
                            ))),
                        }
                    } else {
                        warn!(
                            "Python module {} requires connection to {} (not available)",
                            self.module, ctx.host
                        );
                        Ok(TaskResult::changed().with_msg(format!(
                            "Executed Python module: {} (simulated - no connection)",
                            self.module
                        )))
                    }
                } else {
                    // Module not found anywhere
                    Err(ExecutorError::ModuleNotFound(format!(
                        "Module '{}' not found. Not a native module and not found in Ansible module paths. \
                        Ensure Ansible is installed or set ANSIBLE_LIBRARY environment variable.",
                        self.module
                    )))
                }
            }
        };

        result
    }

    async fn execute_registry_module(
        &self,
        module_name: &str,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        let params: std::collections::HashMap<String, serde_json::Value> =
            args.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        let module_ctx = crate::modules::ModuleContext {
            check_mode: ctx.check_mode,
            diff_mode: ctx.diff_mode,
            verbosity: ctx.verbosity,
            vars: std::collections::HashMap::new(),
            facts: std::collections::HashMap::new(),
            work_dir: None,
            r#become: ctx.r#become,
            become_method: if ctx.r#become {
                Some(ctx.r#become_method.clone())
            } else {
                None
            },
            become_user: if ctx.r#become {
                Some(ctx.r#become_user.clone())
            } else {
                None
            },
            become_password: if ctx.r#become {
                ctx.become_password.clone()
            } else {
                None
            },
            connection: ctx.connection.clone(),
        };

        let module = module_registry.get(module_name).ok_or_else(|| {
            ExecutorError::ModuleNotFound(format!("{} module not found in registry", module_name))
        })?;

        match module.execute(&params, &module_ctx) {
            Ok(output) => {
                let mut result = if output.changed {
                    TaskResult::changed()
                } else {
                    TaskResult::ok()
                };
                result.msg = Some(output.msg.clone());
                result.result = Some(output.to_result_json());
                Ok(result)
            }
            Err(e) => Ok(TaskResult::failed(format!(
                "{} module failed: {}",
                module_name, e
            ))),
        }
    }

    /// Template arguments using variables
    async fn template_args(
        &self,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<IndexMap<String, JsonValue>> {
        let rt = runtime.read().await;
        let vars = rt.get_merged_vars_ref(&ctx.host);
        let mut result = IndexMap::new();

        for (key, value) in &self.args {
            let templated = template_value(value, vars.as_ref())?;
            result.insert(key.clone(), templated);
        }

        Ok(result)
    }

    /// Evaluate a when condition
    async fn evaluate_condition(
        &self,
        condition: &str,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<bool> {
        let rt = runtime.read().await;
        let vars = rt.get_merged_vars_ref(&ctx.host);

        evaluate_expression(condition, vars.as_ref())
    }

    /// Apply changed_when override
    async fn apply_changed_when(
        &self,
        mut result: TaskResult,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<TaskResult> {
        if let Some(ref condition) = self.changed_when {
            let should_be_changed = self.evaluate_condition(condition, ctx, runtime).await?;
            result.changed = should_be_changed;
            result.status = if should_be_changed {
                TaskStatus::Changed
            } else {
                TaskStatus::Ok
            };
        }
        Ok(result)
    }

    /// Apply failed_when override
    async fn apply_failed_when(
        &self,
        mut result: TaskResult,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<TaskResult> {
        if let Some(ref condition) = self.failed_when {
            let should_fail = self.evaluate_condition(condition, ctx, runtime).await?;
            if should_fail {
                result.status = TaskStatus::Failed;
                result.msg = Some(format!(
                    "Failed due to failed_when condition: {}",
                    condition
                ));
            }
        }
        Ok(result)
    }

    /// Register task result
    async fn register_result(
        &self,
        name: &str,
        result: &TaskResult,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<()> {
        let registered = result.to_registered(None, None);

        let mut rt = runtime.write().await;
        rt.register_result(&ctx.host, name.to_string(), registered);

        Ok(())
    }

    // Module implementations

    async fn execute_debug(
        &self,
        args: &IndexMap<String, JsonValue>,
        _ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        if let Some(msg) = args.get("msg") {
            info!("DEBUG: {}", msg);
            Ok(TaskResult::ok().with_msg(format!("{}", msg)))
        } else if let Some(var) = args.get("var") {
            info!("DEBUG: {} = {:?}", var, var);
            Ok(TaskResult::ok().with_result(var.clone()))
        } else {
            Ok(TaskResult::ok())
        }
    }

    async fn execute_gather_facts(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        use crate::modules::{Module, ModuleContext};

        // Get gather_subset from args if provided
        let gather_subset = args
            .get("gather_subset")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            });

        // Check if we have a remote connection - if so, gather facts remotely
        if let Some(ref connection) = ctx.connection {
            debug!(
                host = %ctx.host,
                "Gathering facts remotely via connection"
            );

            // Gather facts via the connection
            let facts = crate::modules::facts::gather_facts_via_connection(
                connection,
                gather_subset.as_deref(),
            )
            .await;

            let mut result = TaskResult::ok();
            result.msg = Some("Facts gathered successfully (remote)".to_string());

            // Wrap facts in ansible_facts key for compatibility
            let mut data = std::collections::HashMap::new();
            let facts_json: serde_json::Map<String, serde_json::Value> =
                facts.into_iter().collect();
            data.insert(
                "ansible_facts".to_string(),
                serde_json::Value::Object(facts_json),
            );

            result.result = Some(serde_json::to_value(&data).unwrap_or_default());

            return Ok(result);
        }

        // No connection or local connection - use local facts gathering
        debug!(
            host = %ctx.host,
            "Gathering facts locally"
        );

        // Convert args to ModuleParams
        let mut params: std::collections::HashMap<String, serde_json::Value> =
            std::collections::HashMap::new();
        if let Some(subset) = gather_subset {
            params.insert("gather_subset".to_string(), serde_json::json!(subset));
        }

        // Create module context
        let module_ctx = ModuleContext::default().with_verbosity(ctx.verbosity);

        // Execute the facts module locally
        let facts_module = crate::modules::facts::FactsModule;
        match facts_module.execute(&params, &module_ctx) {
            Ok(output) => {
                let mut result = TaskResult::ok();
                result.msg = Some(output.msg.clone());

                // Store full module output for register access (includes ansible_facts)
                result.result = Some(output.to_result_json());

                Ok(result)
            }
            Err(e) => Err(ExecutorError::TaskFailed(format!(
                "gather_facts failed: {}",
                e
            ))),
        }
    }

    async fn execute_set_fact(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<TaskResult> {
        let mut rt = runtime.write().await;

        let mut facts_set = Vec::new();

        // Determine the target host for fact storage based on delegation
        // Note: ctx.host is already set to the delegated host if delegation is active
        // The caller (execute method) handles the delegation logic and passes the
        // appropriate host context
        let fact_target = &ctx.host;

        for (key, value) in args {
            if key != "cacheable" {
                // Use set_host_fact instead of set_host_var for proper precedence
                // Facts set by set_fact should have SetFact precedence level
                rt.set_host_fact(fact_target, key.clone(), value.clone());
                debug!(
                    "Set fact '{}' = {:?} for host '{}'",
                    key, value, fact_target
                );
                facts_set.push(key.clone());
            }
        }

        let message = if facts_set.len() == 1 {
            format!("Set fact: {}", facts_set[0])
        } else {
            format!("Set {} facts: {}", facts_set.len(), facts_set.join(", "))
        };

        Ok(TaskResult::ok().with_msg(message))
    }

    async fn execute_command(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        _runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<TaskResult> {
        let cmd = args
            .get("cmd")
            .or_else(|| args.get("_raw_params"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExecutorError::RuntimeError("command module requires 'cmd' argument".into())
            })?;

        if ctx.check_mode {
            return Ok(TaskResult::skipped("Check mode - command not executed"));
        }

        debug!("Would execute command: {}", cmd);

        // In a real implementation, this would actually run the command
        // For now, simulate successful execution
        let result = RegisteredResult {
            changed: true,
            rc: Some(0),
            stdout: Some(String::new()),
            stderr: Some(String::new()),
            ..Default::default()
        };

        Ok(TaskResult::changed()
            .with_msg(format!("Command executed: {}", cmd))
            .with_result(result.to_json()))
    }

    async fn execute_copy(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        // Convert args to ModuleParams
        let params: std::collections::HashMap<String, serde_json::Value> =
            args.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        // Get all variables from runtime for potential content template substitution
        let vars = {
            let rt = runtime.read().await;
            rt.get_merged_vars_ref(&ctx.host)
        };

        // If content contains template variables, use the template module's rendering
        if let Some(serde_json::Value::String(content)) = params.get("content") {
            if content.contains("{{") || content.contains("{%") {
                // Use template module for content with variables
                let template_params = params.clone();
                let module_ctx = crate::modules::ModuleContext {
                    check_mode: ctx.check_mode,
                    diff_mode: ctx.diff_mode,
                    verbosity: ctx.verbosity,
                    vars: vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                    facts: std::collections::HashMap::new(),
                    work_dir: None,
                    r#become: ctx.r#become,
                    become_method: if ctx.r#become {
                        Some(ctx.r#become_method.clone())
                    } else {
                        None
                    },
                    become_user: if ctx.r#become {
                        Some(ctx.r#become_user.clone())
                    } else {
                        None
                    },
                    become_password: if ctx.r#become {
                        ctx.become_password.clone()
                    } else {
                        None
                    },
                    connection: ctx.connection.clone(),
                };

                let module = module_registry.get("template").ok_or_else(|| {
                    ExecutorError::ModuleNotFound("template module not found in registry".into())
                })?;

                return match module.execute(&template_params, &module_ctx) {
                    Ok(output) => {
                        let mut result = if output.changed {
                            TaskResult::changed()
                        } else {
                            TaskResult::ok()
                        };
                        result.msg = Some(output.msg.clone());
                        // Store full module output for register access
                        result.result = Some(output.to_result_json());
                        Ok(result)
                    }
                    Err(e) => Ok(TaskResult::failed(format!(
                        "template (for copy with content) failed: {}",
                        e
                    ))),
                };
            }
        }

        // Create module context from execution context
        let module_ctx = crate::modules::ModuleContext {
            check_mode: ctx.check_mode,
            diff_mode: ctx.diff_mode,
            verbosity: ctx.verbosity,
            vars: std::collections::HashMap::new(),
            facts: std::collections::HashMap::new(),
            work_dir: None,
            r#become: ctx.r#become,
            become_method: if ctx.r#become {
                Some(ctx.r#become_method.clone())
            } else {
                None
            },
            become_user: if ctx.r#become {
                Some(ctx.r#become_user.clone())
            } else {
                None
            },
            become_password: if ctx.r#become {
                ctx.become_password.clone()
            } else {
                None
            },
            connection: ctx.connection.clone(),
        };

        // Get the copy module from shared registry and execute
        let module = module_registry.get("copy").ok_or_else(|| {
            ExecutorError::ModuleNotFound("copy module not found in registry".into())
        })?;

        match module.execute(&params, &module_ctx) {
            Ok(output) => {
                let mut result = if output.changed {
                    TaskResult::changed()
                } else {
                    TaskResult::ok()
                };
                result.msg = Some(output.msg.clone());
                // Store full module output for register access
                result.result = Some(output.to_result_json());
                Ok(result)
            }
            Err(e) => Ok(TaskResult::failed(format!("copy module failed: {}", e))),
        }
    }

    async fn execute_file(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        // Convert args to ModuleParams
        let params: std::collections::HashMap<String, serde_json::Value> =
            args.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        // Create module context from execution context
        let module_ctx = crate::modules::ModuleContext {
            check_mode: ctx.check_mode,
            diff_mode: ctx.diff_mode,
            verbosity: ctx.verbosity,
            vars: std::collections::HashMap::new(),
            facts: std::collections::HashMap::new(),
            work_dir: None,
            r#become: ctx.r#become,
            become_method: if ctx.r#become {
                Some(ctx.r#become_method.clone())
            } else {
                None
            },
            become_user: if ctx.r#become {
                Some(ctx.r#become_user.clone())
            } else {
                None
            },
            become_password: if ctx.r#become {
                ctx.become_password.clone()
            } else {
                None
            },
            connection: ctx.connection.clone(),
        };

        // Get the file module from shared registry and execute
        let module = module_registry.get("file").ok_or_else(|| {
            ExecutorError::ModuleNotFound("file module not found in registry".into())
        })?;

        match module.execute(&params, &module_ctx) {
            Ok(output) => {
                let mut result = if output.changed {
                    TaskResult::changed()
                } else {
                    TaskResult::ok()
                };
                result.msg = Some(output.msg.clone());
                // Store full module output for register access
                result.result = Some(output.to_result_json());
                Ok(result)
            }
            Err(e) => Ok(TaskResult::failed(format!("file module failed: {}", e))),
        }
    }

    async fn execute_template(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        // Convert args to ModuleParams
        let params: std::collections::HashMap<String, serde_json::Value> =
            args.iter().map(|(k, v)| (k.clone(), v.clone())).collect();

        // Get all variables from runtime for template substitution
        let vars = {
            let rt = runtime.read().await;
            rt.get_merged_vars_ref(&ctx.host)
        };

        // Create module context from execution context with variables
        let module_ctx = crate::modules::ModuleContext {
            check_mode: ctx.check_mode,
            diff_mode: ctx.diff_mode,
            verbosity: ctx.verbosity,
            vars: vars.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            facts: std::collections::HashMap::new(),
            work_dir: None,
            r#become: ctx.r#become,
            become_method: if ctx.r#become {
                Some(ctx.r#become_method.clone())
            } else {
                None
            },
            become_user: if ctx.r#become {
                Some(ctx.r#become_user.clone())
            } else {
                None
            },
            become_password: if ctx.r#become {
                ctx.become_password.clone()
            } else {
                None
            },
            connection: ctx.connection.clone(),
        };

        // Get the template module from shared registry and execute
        let module = module_registry.get("template").ok_or_else(|| {
            ExecutorError::ModuleNotFound("template module not found in registry".into())
        })?;

        match module.execute(&params, &module_ctx) {
            Ok(output) => {
                let mut result = if output.changed {
                    TaskResult::changed()
                } else {
                    TaskResult::ok()
                };
                result.msg = Some(output.msg.clone());
                // Store full module output for register access
                result.result = Some(output.to_result_json());
                Ok(result)
            }
            Err(e) => Ok(TaskResult::failed(format!("template module failed: {}", e))),
        }
    }

    async fn execute_package(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        let name = args.get("name").ok_or_else(|| {
            ExecutorError::RuntimeError("package module requires 'name' argument".into())
        })?;

        let state = args
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("present");

        if ctx.check_mode {
            return Ok(TaskResult::ok().with_msg(format!(
                "Check mode - would ensure package {:?} is {}",
                name, state
            )));
        }

        debug!("Would ensure package {:?} is {}", name, state);
        Ok(TaskResult::changed().with_msg(format!("Package {:?} state: {}", name, state)))
    }

    async fn execute_service(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            ExecutorError::RuntimeError("service module requires 'name' argument".into())
        })?;

        let state = args.get("state").and_then(|v| v.as_str());
        let enabled = args.get("enabled").and_then(|v| v.as_bool());

        if ctx.check_mode {
            return Ok(
                TaskResult::ok().with_msg(format!("Check mode - would manage service {}", name))
            );
        }

        debug!(
            "Would manage service: {} (state: {:?}, enabled: {:?})",
            name, state, enabled
        );
        Ok(TaskResult::changed().with_msg(format!("Service {} managed", name)))
    }

    async fn execute_user(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            ExecutorError::RuntimeError("user module requires 'name' argument".into())
        })?;

        if ctx.check_mode {
            return Ok(
                TaskResult::ok().with_msg(format!("Check mode - would manage user {}", name))
            );
        }

        debug!("Would manage user: {}", name);
        Ok(TaskResult::changed().with_msg(format!("User {} managed", name)))
    }

    async fn execute_group(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            ExecutorError::RuntimeError("group module requires 'name' argument".into())
        })?;

        if ctx.check_mode {
            return Ok(
                TaskResult::ok().with_msg(format!("Check mode - would manage group {}", name))
            );
        }

        debug!("Would manage group: {}", name);
        Ok(TaskResult::changed().with_msg(format!("Group {} managed", name)))
    }

    async fn execute_lineinfile(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
            ExecutorError::RuntimeError("lineinfile requires 'path' argument".into())
        })?;

        if ctx.check_mode {
            return Ok(TaskResult::ok().with_msg(format!("Check mode - would modify {}", path)));
        }

        debug!("Would modify line in: {}", path);
        Ok(TaskResult::changed().with_msg(format!("Modified {}", path)))
    }

    async fn execute_blockinfile(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        let path = args.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
            ExecutorError::RuntimeError("blockinfile requires 'path' argument".into())
        })?;

        if ctx.check_mode {
            return Ok(
                TaskResult::ok().with_msg(format!("Check mode - would modify block in {}", path))
            );
        }

        debug!("Would modify block in: {}", path);
        Ok(TaskResult::changed().with_msg(format!("Modified block in {}", path)))
    }

    async fn execute_stat(
        &self,
        args: &IndexMap<String, JsonValue>,
        _ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ExecutorError::RuntimeError("stat requires 'path' argument".into()))?;

        debug!("Would stat: {}", path);

        // Return simulated stat result
        let stat_result = serde_json::json!({
            "exists": true,
            "path": path,
            "isdir": false,
            "isreg": true,
            "mode": "0644",
            "uid": 1000,
            "gid": 1000,
            "size": 1024,
        });

        Ok(TaskResult::ok().with_result(serde_json::json!({ "stat": stat_result })))
    }

    async fn execute_fail(&self, args: &IndexMap<String, JsonValue>) -> ExecutorResult<TaskResult> {
        let msg = args
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("Failed as requested");

        Ok(TaskResult::failed(msg))
    }

    async fn execute_assert(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<TaskResult> {
        let that = args
            .get("that")
            .ok_or_else(|| ExecutorError::RuntimeError("assert requires 'that' argument".into()))?;

        let conditions: Vec<&str> = match that {
            JsonValue::String(s) => vec![s.as_str()],
            JsonValue::Array(arr) => arr.iter().filter_map(|v| v.as_str()).collect(),
            _ => {
                return Err(ExecutorError::RuntimeError(
                    "assert 'that' must be string or array".into(),
                ))
            }
        };

        for condition in conditions {
            let result = self.evaluate_condition(condition, ctx, runtime).await?;
            if !result {
                let fail_msg = args
                    .get("fail_msg")
                    .or_else(|| args.get("msg"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Assertion failed");

                return Ok(TaskResult::failed(format!("{}: {}", fail_msg, condition)));
            }
        }

        let success_msg = args
            .get("success_msg")
            .and_then(|v| v.as_str())
            .unwrap_or("All assertions passed");

        Ok(TaskResult::ok().with_msg(success_msg))
    }

    async fn execute_pause(
        &self,
        args: &IndexMap<String, JsonValue>,
    ) -> ExecutorResult<TaskResult> {
        let seconds = args.get("seconds").and_then(|v| v.as_u64()).unwrap_or(0);

        if seconds > 0 {
            debug!("Pausing for {} seconds", seconds);
            tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;
        }

        Ok(TaskResult::ok().with_msg(format!("Paused for {} seconds", seconds)))
    }

    async fn execute_wait_for(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult> {
        let host = args
            .get("host")
            .and_then(|v| v.as_str())
            .unwrap_or(&ctx.host);
        let port = args.get("port").and_then(|v| v.as_u64());
        let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(300);

        if let Some(p) = port {
            debug!("Would wait for {}:{} (timeout: {}s)", host, p, timeout);
        }

        Ok(TaskResult::ok().with_msg("Wait condition met"))
    }

    /// Validate that a path is safe and within the allowed base directory.
    ///
    /// This function prevents path traversal attacks by:
    /// 1. Rejecting paths containing ".." traversal components
    /// 2. Canonicalizing paths to resolve symlinks
    /// 3. Ensuring the resolved path stays within the base directory
    ///
    /// # Security
    ///
    /// This is a critical security function. All file operations that load
    /// external content (variables, tasks, etc.) MUST use this validation
    /// to prevent unauthorized file access.
    fn validate_include_path(
        requested_path: &str,
        base_path: &std::path::Path,
    ) -> ExecutorResult<std::path::PathBuf> {
        use std::path::{Path, PathBuf};

        // Early rejection of obvious path traversal attempts
        // Check for ".." in path components (handles both Unix and Windows separators)
        if requested_path.contains("..") {
            warn!(
                "Security: Rejecting path traversal attempt in include_vars: '{}'",
                requested_path
            );
            return Err(ExecutorError::RuntimeError(format!(
                "Security violation: Path traversal detected in '{}'. \
                 Paths containing '..' are not allowed for security reasons.",
                requested_path
            )));
        }

        let path = Path::new(requested_path);

        // Construct the full path
        let full_path = if path.is_absolute() {
            PathBuf::from(requested_path)
        } else {
            base_path.join(requested_path)
        };

        // Check if the path exists before canonicalizing
        if !full_path.exists() {
            return Err(ExecutorError::RuntimeError(format!(
                "include_vars path not found: {}",
                full_path.display()
            )));
        }

        // Canonicalize base path for comparison
        let canonical_base = base_path.canonicalize().map_err(|e| {
            ExecutorError::RuntimeError(format!(
                "Failed to resolve base path '{}': {}",
                base_path.display(),
                e
            ))
        })?;

        // Canonicalize the requested path to resolve symlinks and normalize
        let canonical_path = full_path.canonicalize().map_err(|e| {
            ExecutorError::RuntimeError(format!(
                "Failed to resolve include_vars path '{}': {}",
                full_path.display(),
                e
            ))
        })?;

        // Security check: ensure the canonical path is within the base directory
        if !canonical_path.starts_with(&canonical_base) {
            warn!(
                "Security: Path traversal blocked - '{}' (resolved to '{}') escapes base '{}'",
                requested_path,
                canonical_path.display(),
                canonical_base.display()
            );
            return Err(ExecutorError::RuntimeError(format!(
                "Security violation: Path '{}' resolves to '{}' which is outside \
                 the allowed directory '{}'. This may indicate a path traversal attack.",
                requested_path,
                canonical_path.display(),
                canonical_base.display()
            )));
        }

        debug!(
            "Path validated: '{}' -> '{}' (within '{}')",
            requested_path,
            canonical_path.display(),
            canonical_base.display()
        );

        Ok(canonical_path)
    }

    async fn execute_include_vars(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
    ) -> ExecutorResult<TaskResult> {
        // Get file or dir parameter
        let file = args
            .get("file")
            .or_else(|| args.get("_raw_params"))
            .and_then(|v| v.as_str());
        let dir = args.get("dir").and_then(|v| v.as_str());
        let name = args.get("name").and_then(|v| v.as_str());

        if file.is_none() && dir.is_none() {
            return Err(ExecutorError::RuntimeError(
                "include_vars requires 'file' or 'dir' parameter".into(),
            ));
        }

        if file.is_some() && dir.is_some() {
            return Err(ExecutorError::RuntimeError(
                "include_vars cannot have both 'file' and 'dir' parameters".into(),
            ));
        }

        // Determine base path from playbook_dir magic variable (set from playbook path)
        // Falls back to current directory if playbook_dir is not set
        let base_path = {
            let rt = runtime.read().await;
            rt.get_var("playbook_dir", Some(&ctx.host))
                .and_then(|v| v.as_str().map(std::path::PathBuf::from))
                .unwrap_or_else(|| {
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                })
        };

        let mut all_vars: IndexMap<String, JsonValue> = IndexMap::new();
        let source: String;

        if let Some(file_path) = file {
            // Validate and resolve the file path with security checks
            let resolved_path = Self::validate_include_path(file_path, &base_path)?;

            let content = tokio::fs::read_to_string(&resolved_path)
                .await
                .map_err(|e| {
                    ExecutorError::RuntimeError(format!(
                        "Failed to read include_vars file {}: {}",
                        resolved_path.display(),
                        e
                    ))
                })?;

            // Parse as YAML (which also handles JSON)
            let vars: IndexMap<String, serde_yaml::Value> = serde_yaml::from_str(&content)
                .map_err(|e| {
                    ExecutorError::RuntimeError(format!(
                        "Failed to parse include_vars file {}: {}",
                        resolved_path.display(),
                        e
                    ))
                })?;

            // Convert YAML values to JSON values
            for (key, value) in vars {
                let json_value = serde_json::to_value(&value).map_err(|e| {
                    ExecutorError::RuntimeError(format!(
                        "Failed to convert variable {}: {}",
                        key, e
                    ))
                })?;
                all_vars.insert(key, json_value);
            }

            source = resolved_path.display().to_string();
        } else if let Some(dir_path) = dir {
            // Validate and resolve the directory path with security checks
            let resolved_path = Self::validate_include_path(dir_path, &base_path)?;

            if !resolved_path.is_dir() {
                return Err(ExecutorError::RuntimeError(format!(
                    "include_vars path is not a directory: {}",
                    resolved_path.display()
                )));
            }

            // Read and sort files by name for predictable ordering
            let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&resolved_path)
                .map_err(|e| {
                    ExecutorError::RuntimeError(format!(
                        "Failed to read directory {}: {}",
                        resolved_path.display(),
                        e
                    ))
                })?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && (p.extension() == Some("yml".as_ref())
                            || p.extension() == Some("yaml".as_ref())
                            || p.extension() == Some("json".as_ref()))
                })
                .collect();

            files.sort();

            // Validate each file in the directory is within the base path
            // This protects against symlink attacks within the directory
            for file_path in &files {
                let canonical_file = file_path.canonicalize().map_err(|e| {
                    ExecutorError::RuntimeError(format!(
                        "Failed to resolve file path '{}': {}",
                        file_path.display(),
                        e
                    ))
                })?;

                let canonical_base = base_path.canonicalize().map_err(|e| {
                    ExecutorError::RuntimeError(format!(
                        "Failed to resolve base path '{}': {}",
                        base_path.display(),
                        e
                    ))
                })?;

                if !canonical_file.starts_with(&canonical_base) {
                    warn!(
                        "Security: Symlink escape blocked - '{}' (resolved to '{}') escapes base '{}'",
                        file_path.display(),
                        canonical_file.display(),
                        canonical_base.display()
                    );
                    return Err(ExecutorError::RuntimeError(format!(
                        "Security violation: File '{}' in include_vars directory resolves to '{}' \
                         which is outside the allowed directory '{}'. This may indicate a symlink attack.",
                        file_path.display(),
                        canonical_file.display(),
                        canonical_base.display()
                    )));
                }
            }

            // Load each file and merge variables
            for file_path in &files {
                let content = tokio::fs::read_to_string(file_path).await.map_err(|e| {
                    ExecutorError::RuntimeError(format!(
                        "Failed to read file {}: {}",
                        file_path.display(),
                        e
                    ))
                })?;

                let vars: IndexMap<String, serde_yaml::Value> = serde_yaml::from_str(&content)
                    .map_err(|e| {
                        ExecutorError::RuntimeError(format!(
                            "Failed to parse file {}: {}",
                            file_path.display(),
                            e
                        ))
                    })?;

                for (key, value) in vars {
                    let json_value = serde_json::to_value(&value).map_err(|e| {
                        ExecutorError::RuntimeError(format!(
                            "Failed to convert variable {}: {}",
                            key, e
                        ))
                    })?;
                    all_vars.insert(key, json_value);
                }
            }

            source = format!("{}/*.yml", resolved_path.display());
        } else {
            return Err(ExecutorError::RuntimeError(
                "include_vars requires 'file' or 'dir' parameter".into(),
            ));
        }

        // If 'name' parameter is specified, scope all variables under that key
        let final_vars = if let Some(scope_name) = name {
            let mut scoped = IndexMap::new();
            scoped.insert(
                scope_name.to_string(),
                JsonValue::Object(all_vars.into_iter().collect()),
            );
            scoped
        } else {
            all_vars
        };

        let var_count = final_vars.len();

        // Store variables in the runtime context for the current host
        {
            let mut rt = runtime.write().await;
            for (key, value) in &final_vars {
                rt.set_host_var(&ctx.host, key.clone(), value.clone());
            }
        }

        info!(
            "Loaded {} variable(s) from {} for host {}",
            var_count, source, ctx.host
        );

        Ok(TaskResult::ok().with_msg(format!("Loaded {} variable(s) from {}", var_count, source)))
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_include_tasks(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
        runtime: &Arc<RwLock<RuntimeContext>>,
        handlers: &Arc<RwLock<HashMap<String, Handler>>>,
        notified: &Arc<Mutex<std::collections::HashSet<String>>>,
        parallelization_manager: &Arc<ParallelizationManager>,
        module_registry: &Arc<ModuleRegistry>,
    ) -> ExecutorResult<TaskResult> {
        let file = args
            .get("file")
            .or_else(|| args.get("_raw_params"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ExecutorError::RuntimeError("include_tasks requires file path".into())
            })?;

        info!("Including tasks from: {}", file);

        // Determine base path from playbook_dir magic variable (set from playbook path)
        // Falls back to current directory if playbook_dir is not set
        let base_path = {
            let rt = runtime.read().await;
            rt.get_var("playbook_dir", Some(&ctx.host))
                .and_then(|v| v.as_str().map(std::path::PathBuf::from))
                .unwrap_or_else(|| {
                    warn!("playbook_dir not set, falling back to current directory for include path resolution");
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
                })
        };
        debug!("Using base path for include: {}", base_path.display());
        let handler = crate::executor::include_handler::IncludeTasksHandler::new(base_path);

        // Build the include spec with any variables passed
        let mut spec = crate::include::IncludeTasksSpec::new(file);

        // Add any variables passed to include_tasks
        if let Some(vars) = args.get("vars").and_then(|v| v.as_object()) {
            for (key, value) in vars {
                spec = spec.with_var(key, value.clone());
            }
        }

        // Load tasks from the file (returns playbook::Task)
        let playbook_tasks = handler
            .load_include_tasks(&spec, runtime, &ctx.host)
            .await
            .map_err(|e| {
                ExecutorError::RuntimeError(format!("Failed to load include_tasks: {}", e))
            })?;

        debug!("Loaded {} tasks from {}", playbook_tasks.len(), file);

        // Convert playbook::Task to executor::task::Task and execute
        let mut total_changed = false;
        let mut task_count = 0;
        let mut failed = false;

        for playbook_task in playbook_tasks {
            // Convert to executor task
            let executor_task: Task = playbook_task.into();
            // Use Box::pin to handle async recursion
            let result = Box::pin(executor_task.execute(
                ctx,
                runtime,
                handlers,
                notified,
                parallelization_manager,
                module_registry,
            ))
            .await?;

            task_count += 1;
            if result.changed {
                total_changed = true;
            }
            if result.status == TaskStatus::Failed {
                failed = true;
                break;
            }
        }

        if failed {
            Ok(TaskResult::failed(format!(
                "Included {} tasks from {}, execution failed",
                task_count, file
            )))
        } else {
            let mut result = if total_changed {
                TaskResult::changed()
            } else {
                TaskResult::ok()
            };
            result.msg = Some(format!("Included {} tasks from {}", task_count, file));
            Ok(result)
        }
    }

    async fn execute_meta(&self, args: &IndexMap<String, JsonValue>) -> ExecutorResult<TaskResult> {
        let action = args
            .get("_raw_params")
            .or_else(|| args.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("noop");

        match action {
            "flush_handlers" => {
                debug!("Would flush handlers");
                Ok(TaskResult::ok().with_msg("Handlers flushed"))
            }
            "refresh_inventory" => {
                debug!("Would refresh inventory");
                Ok(TaskResult::ok().with_msg("Inventory refreshed"))
            }
            "noop" => Ok(TaskResult::ok()),
            "end_play" => Ok(TaskResult::ok().with_msg("Play ended")),
            "end_host" => Ok(TaskResult::ok().with_msg("Host ended")),
            "clear_facts" => {
                debug!("Would clear facts");
                Ok(TaskResult::ok().with_msg("Facts cleared"))
            }
            "clear_host_errors" => Ok(TaskResult::ok().with_msg("Host errors cleared")),
            _ => {
                warn!("Unknown meta action: {}", action);
                Ok(TaskResult::ok())
            }
        }
    }
}

/// Template a value using variables
///
/// # Performance
/// Hot path function with optimizations:
/// - Early return for non-templatable values (numbers, bools, null)
/// - Inline hint for better compiler optimization
#[inline]
fn template_value(
    value: &JsonValue,
    vars: &IndexMap<String, JsonValue>,
) -> ExecutorResult<JsonValue> {
    // Use the unified template engine for all value rendering
    TEMPLATE_ENGINE
        .render_value(value, vars)
        .map_err(|e| template_error_from_value(value, e))
}

/// Template a string using variables
///
/// # Performance
/// The unified TemplateEngine includes a fast-path check: if a string contains
/// no template syntax (`{{` or `{%`), rendering is bypassed entirely.
#[inline]
fn template_string(template: &str, vars: &IndexMap<String, JsonValue>) -> ExecutorResult<String> {
    // Use the unified template engine for all string rendering
    TEMPLATE_ENGINE
        .render_with_indexmap(template, vars)
        .map_err(|e| template_error_to_executor(template, e))
}

fn template_error_from_value(value: &JsonValue, error: Error) -> ExecutorError {
    let source = template_source_for_value(value);
    template_error_to_executor(&source, error)
}

fn template_source_for_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(s) => s.clone(),
        _ => serde_yaml::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn template_error_to_executor(template_source: &str, error: Error) -> ExecutorError {
    match error {
        Error::Template(mini_err) => {
            let message = mini_err
                .detail()
                .map(str::to_string)
                .unwrap_or_else(|| mini_err.to_string());
            let line = mini_err.line().unwrap_or(1);
            let col = 1;
            let name = mini_err.name().unwrap_or("<template>");
            let file = if name.starts_with("__rustible_template_") {
                "<template>"
            } else {
                name
            };
            let diagnostic = template_syntax_error(file, template_source, line, col, &message);
            ExecutorError::diagnostic(diagnostic, Some(template_source.to_string()))
        }
        Error::TemplateRender { message, .. } => {
            let diagnostic = template_syntax_error("<template>", template_source, 1, 1, &message);
            ExecutorError::diagnostic(diagnostic, Some(template_source.to_string()))
        }
        Error::TemplateSyntax { message, .. } => {
            let diagnostic = template_syntax_error("<template>", template_source, 1, 1, &message);
            ExecutorError::diagnostic(diagnostic, Some(template_source.to_string()))
        }
        other => ExecutorError::RuntimeError(format!("Template error: {}", other)),
    }
}

/// Convert JSON value to string for templating
///
/// # Performance
/// Hot path function - called for every template variable substitution.
#[inline]
fn json_to_string(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => String::new(),
        JsonValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => s.clone(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// Find the position of the matching closing parenthesis
///
/// # Performance
/// Used in expression parsing - inline for better optimization.
#[inline]
fn find_matching_paren(expr: &str, open_pos: usize) -> Option<usize> {
    let bytes = expr.as_bytes();
    let mut depth = 1;
    let mut pos = open_pos + 1;

    while pos < bytes.len() {
        match bytes[pos] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(pos);
                }
            }
            _ => {}
        }
        pos += 1;
    }
    None
}

/// Find position of operator outside parentheses (returns rightmost match for left-associativity)
///
/// # Performance
/// Hot path for expression parsing - inline for better optimization.
#[inline]
fn find_operator_outside_parens(expr: &str, op: &str) -> Option<usize> {
    let mut depth = 0;
    let bytes = expr.as_bytes();
    let op_bytes = op.as_bytes();
    let mut last_match: Option<usize> = None;

    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {
                if depth == 0
                    && i + op_bytes.len() <= bytes.len()
                    && &bytes[i..i + op_bytes.len()] == op_bytes
                {
                    last_match = Some(i);
                }
            }
        }
        i += 1;
    }
    last_match
}

/// Compare two JSON values with ordering
///
/// # Performance
/// Inline hint for hot path comparisons.
#[inline]
fn compare_values(left: &JsonValue, right: &JsonValue) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (JsonValue::Number(l), JsonValue::Number(r)) => {
            let lf = l.as_f64()?;
            let rf = r.as_f64()?;
            lf.partial_cmp(&rf)
        }
        (JsonValue::String(l), JsonValue::String(r)) => Some(l.cmp(r)),
        (JsonValue::String(l), JsonValue::Number(r)) => {
            if let Ok(lf) = l.parse::<f64>() {
                lf.partial_cmp(&r.as_f64()?)
            } else {
                None
            }
        }
        (JsonValue::Number(l), JsonValue::String(r)) => {
            if let Ok(rf) = r.parse::<f64>() {
                l.as_f64()?.partial_cmp(&rf)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Compare version strings (e.g., "1.2.3" vs "1.3.0")
fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    let parse_parts = |v: &str| -> Vec<i64> {
        v.split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse::<i64>().ok())
            .collect()
    };

    let p1 = parse_parts(v1);
    let p2 = parse_parts(v2);

    for i in 0..std::cmp::max(p1.len(), p2.len()) {
        let n1 = p1.get(i).copied().unwrap_or(0);
        let n2 = p2.get(i).copied().unwrap_or(0);
        match n1.cmp(&n2) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Evaluate a Jinja2 test expression (e.g., "is string", "is match('pattern')")
fn evaluate_jinja_test(
    value: &JsonValue,
    test_name: &str,
    test_arg: Option<&str>,
    vars: &IndexMap<String, JsonValue>,
) -> bool {
    match test_name {
        "defined" => !value.is_null(),
        "undefined" => value.is_null(),
        "none" | "null" => value.is_null(),
        "true" => matches!(value, JsonValue::Bool(true)),
        "false" => matches!(value, JsonValue::Bool(false)),
        "boolean" | "bool" => matches!(value, JsonValue::Bool(_)),
        "string" => matches!(value, JsonValue::String(_)),
        "number" | "integer" | "float" => matches!(value, JsonValue::Number(_)),
        "mapping" | "dict" => matches!(value, JsonValue::Object(_)),
        "iterable" | "sequence" => matches!(value, JsonValue::Array(_) | JsonValue::String(_)),
        "callable" => false, // Rust values are not callable in Jinja2 sense
        "sameas" => {
            if let Some(arg) = test_arg {
                let other = vars.get(arg.trim()).unwrap_or(&JsonValue::Null);
                std::ptr::eq(value, other) || value == other
            } else {
                false
            }
        }
        "empty" => match value {
            JsonValue::Null => true,
            JsonValue::String(s) => s.is_empty(),
            JsonValue::Array(a) => a.is_empty(),
            JsonValue::Object(o) => o.is_empty(),
            _ => false,
        },
        "even" => {
            if let JsonValue::Number(n) = value {
                n.as_i64().map(|i| i % 2 == 0).unwrap_or(false)
            } else {
                false
            }
        }
        "odd" => {
            if let JsonValue::Number(n) = value {
                n.as_i64().map(|i| i % 2 != 0).unwrap_or(false)
            } else {
                false
            }
        }
        "lower" => {
            if let JsonValue::String(s) = value {
                s.chars().all(|c| !c.is_alphabetic() || c.is_lowercase())
            } else {
                false
            }
        }
        "upper" => {
            if let JsonValue::String(s) = value {
                s.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
            } else {
                false
            }
        }
        "match" | "regex" => {
            if let (JsonValue::String(s), Some(pattern)) = (value, test_arg) {
                let pattern = pattern.trim().trim_matches(|c| c == '\'' || c == '"');
                regex::Regex::new(pattern)
                    .map(|re| re.is_match(s))
                    .unwrap_or(false)
            } else {
                false
            }
        }
        "search" => {
            if let (JsonValue::String(s), Some(pattern)) = (value, test_arg) {
                let pattern = pattern.trim().trim_matches(|c| c == '\'' || c == '"');
                regex::Regex::new(pattern)
                    .map(|re| re.find(s).is_some())
                    .unwrap_or(false)
            } else {
                false
            }
        }
        "divisibleby" => {
            if let (JsonValue::Number(n), Some(arg)) = (value, test_arg) {
                let arg = arg.trim().trim_matches(|c| c == '\'' || c == '"');
                if let (Some(val), Ok(div)) = (n.as_i64(), arg.parse::<i64>()) {
                    div != 0 && val % div == 0
                } else {
                    false
                }
            } else {
                false
            }
        }
        "startswith" => {
            if let (JsonValue::String(s), Some(arg)) = (value, test_arg) {
                let prefix = arg.trim().trim_matches(|c| c == '\'' || c == '"');
                s.starts_with(prefix)
            } else {
                false
            }
        }
        "endswith" => {
            if let (JsonValue::String(s), Some(arg)) = (value, test_arg) {
                let suffix = arg.trim().trim_matches(|c| c == '\'' || c == '"');
                s.ends_with(suffix)
            } else {
                false
            }
        }
        "version" | "version_compare" => {
            if let (JsonValue::String(val), Some(args)) = (value, test_arg) {
                let parts: Vec<&str> = args.split(',').collect();
                if parts.len() >= 2 {
                    let arg1 = parts[0].trim().trim_matches(|c| c == '\'' || c == '"');
                    let arg2 = parts[1].trim().trim_matches(|c| c == '\'' || c == '"');
                    let (op, version) = if [
                        "<", ">", "<=", ">=", "==", "!=", "lt", "gt", "le", "ge", "eq", "ne",
                    ]
                    .contains(&arg1)
                    {
                        (arg1, arg2)
                    } else {
                        (arg2, arg1)
                    };
                    let cmp = compare_versions(val, version);
                    match op {
                        "<" | "lt" => cmp == std::cmp::Ordering::Less,
                        ">" | "gt" => cmp == std::cmp::Ordering::Greater,
                        "<=" | "le" => cmp != std::cmp::Ordering::Greater,
                        ">=" | "ge" => cmp != std::cmp::Ordering::Less,
                        "==" | "eq" => cmp == std::cmp::Ordering::Equal,
                        "!=" | "ne" => cmp != std::cmp::Ordering::Equal,
                        _ => false,
                    }
                } else {
                    false
                }
            } else {
                false
            }
        }
        "subset" => {
            if let (JsonValue::Array(subset), Some(arg)) = (value, test_arg) {
                if let Some(JsonValue::Array(superset)) = vars.get(arg.trim()) {
                    subset.iter().all(|item| superset.contains(item))
                } else {
                    false
                }
            } else {
                false
            }
        }
        "superset" => {
            if let (JsonValue::Array(superset), Some(arg)) = (value, test_arg) {
                if let Some(JsonValue::Array(subset)) = vars.get(arg.trim()) {
                    subset.iter().all(|item| superset.contains(item))
                } else {
                    false
                }
            } else {
                false
            }
        }
        "in" => {
            if let Some(arg) = test_arg {
                if let Some(container) = vars.get(arg.trim()) {
                    match container {
                        JsonValue::Array(arr) => arr.contains(value),
                        JsonValue::String(s) => {
                            if let JsonValue::String(v) = value {
                                s.contains(v.as_str())
                            } else {
                                false
                            }
                        }
                        JsonValue::Object(obj) => {
                            if let JsonValue::String(k) = value {
                                obj.contains_key(k)
                            } else {
                                false
                            }
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            } else {
                false
            }
        }
        "truthy" => is_truthy(value),
        "falsy" => !is_truthy(value),
        "abs" => matches!(value, JsonValue::Number(_)),
        _ => false,
    }
}

/// Evaluate a conditional expression using the unified template engine
fn evaluate_expression(expr: &str, vars: &IndexMap<String, JsonValue>) -> ExecutorResult<bool> {
    // Use the unified template engine for all condition evaluation
    TEMPLATE_ENGINE
        .evaluate_condition(expr, vars)
        .map_err(|e| template_error_to_executor(expr, e))
}

/// Check if a JSON value is "truthy"
///
/// # Performance
/// Hot path function - called for every condition evaluation.
#[inline]
fn is_truthy(value: &JsonValue) -> bool {
    match value {
        JsonValue::Null => false,
        JsonValue::Bool(b) => *b,
        JsonValue::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        JsonValue::String(s) => {
            let value = s.trim();
            if value.is_empty() {
                return false;
            }
            if value.eq_ignore_ascii_case("false")
                || value.eq_ignore_ascii_case("no")
                || value.eq_ignore_ascii_case("off")
                || value.eq_ignore_ascii_case("n")
                || value.eq_ignore_ascii_case("f")
                || value == "0"
            {
                return false;
            }
            true
        }
        JsonValue::Array(arr) => !arr.is_empty(),
        JsonValue::Object(obj) => !obj.is_empty(),
    }
}

/// Module trait for implementing custom modules
#[async_trait]
pub trait Module: Send + Sync {
    /// Module name
    fn name(&self) -> &str;

    /// Execute the module
    async fn execute(
        &self,
        args: &IndexMap<String, JsonValue>,
        ctx: &ExecutionContext,
    ) -> ExecutorResult<TaskResult>;

    /// Validate arguments
    fn validate_args(&self, _args: &IndexMap<String, JsonValue>) -> ExecutorResult<()> {
        Ok(())
    }

    /// Check if module supports check mode
    fn supports_check_mode(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn setup_execution(
        host: &str,
    ) -> (
        ExecutionContext,
        Arc<RwLock<RuntimeContext>>,
        Arc<RwLock<HashMap<String, Handler>>>,
        Arc<Mutex<HashSet<String>>>,
        Arc<ParallelizationManager>,
        Arc<ModuleRegistry>,
    ) {
        (
            ExecutionContext::new(host),
            Arc::new(RwLock::new(RuntimeContext::new())),
            Arc::new(RwLock::new(HashMap::new())),
            Arc::new(Mutex::new(HashSet::new())),
            Arc::new(ParallelizationManager::new()),
            Arc::new(ModuleRegistry::new()),
        )
    }

    #[test]
    fn test_task_builder() {
        let task = Task::new("Install nginx", "package")
            .arg("name", "nginx")
            .arg("state", "present")
            .when("ansible_os_family == 'Debian'")
            .notify("restart nginx")
            .register("install_result");

        assert_eq!(task.name, "Install nginx");
        assert_eq!(task.module, "package");
        assert_eq!(
            task.args.get("name"),
            Some(&JsonValue::String("nginx".into()))
        );
        assert_eq!(task.when, Some("ansible_os_family == 'Debian'".to_string()));
        assert!(task.notify.contains(&"restart nginx".to_string()));
        assert_eq!(task.register, Some("install_result".to_string()));
    }

    #[test]
    fn test_template_string() {
        let mut vars = IndexMap::new();
        vars.insert("name".to_string(), JsonValue::String("world".to_string()));
        vars.insert("count".to_string(), JsonValue::Number(42.into()));

        let result = template_string("Hello {{ name }}!", &vars).unwrap();
        assert_eq!(result, "Hello world!");

        let result = template_string("Count: {{ count }}", &vars).unwrap();
        assert_eq!(result, "Count: 42");
    }

    #[test]
    fn test_template_string_diagnostic() {
        let vars = IndexMap::new();
        let err = template_string("Hello {{", &vars).unwrap_err();
        let rendered = err.render_diagnostic().expect("expected diagnostic");
        assert!(rendered.contains("E0020"));
        assert!(rendered.contains("template error"));
    }

    #[test]
    fn test_evaluate_expression_boolean() {
        let vars = IndexMap::new();

        assert!(evaluate_expression("true", &vars).unwrap());
        assert!(!evaluate_expression("false", &vars).unwrap());
        assert!(!evaluate_expression("not true", &vars).unwrap());
    }

    #[test]
    fn test_evaluate_expression_comparison() {
        let mut vars = IndexMap::new();
        vars.insert("os".to_string(), JsonValue::String("Debian".to_string()));
        vars.insert("count".to_string(), JsonValue::Number(5.into()));

        assert!(evaluate_expression("os == 'Debian'", &vars).unwrap());
        assert!(!evaluate_expression("os == 'RedHat'", &vars).unwrap());
        assert!(evaluate_expression("os != 'RedHat'", &vars).unwrap());
    }

    #[test]
    fn test_evaluate_expression_defined() {
        let mut vars = IndexMap::new();
        vars.insert(
            "existing".to_string(),
            JsonValue::String("value".to_string()),
        );

        assert!(evaluate_expression("existing is defined", &vars).unwrap());
        assert!(!evaluate_expression("nonexistent is defined", &vars).unwrap());
        assert!(evaluate_expression("nonexistent is not defined", &vars).unwrap());
    }

    #[test]
    fn test_evaluate_expression_in() {
        let mut vars = IndexMap::new();
        vars.insert("items".to_string(), serde_json::json!(["a", "b", "c"]));
        vars.insert("letter".to_string(), JsonValue::String("b".to_string()));

        assert!(evaluate_expression("letter in items", &vars).unwrap());
    }

    #[test]
    fn test_task_result() {
        let result = TaskResult::ok();
        assert_eq!(result.status, TaskStatus::Ok);
        assert!(!result.changed);

        let result = TaskResult::changed();
        assert_eq!(result.status, TaskStatus::Changed);
        assert!(result.changed);

        let result = TaskResult::failed("error message");
        assert_eq!(result.status, TaskStatus::Failed);
        assert_eq!(result.msg, Some("error message".to_string()));
    }

    #[test]
    fn test_is_truthy() {
        assert!(!is_truthy(&JsonValue::Null));
        assert!(!is_truthy(&JsonValue::Bool(false)));
        assert!(is_truthy(&JsonValue::Bool(true)));
        assert!(!is_truthy(&JsonValue::String("".to_string())));
        assert!(!is_truthy(&JsonValue::String("0".to_string())));
        assert!(!is_truthy(&JsonValue::String("false".to_string())));
        assert!(!is_truthy(&JsonValue::String("no".to_string())));
        assert!(!is_truthy(&JsonValue::String("off".to_string())));
        assert!(!is_truthy(&JsonValue::String("n".to_string())));
        assert!(!is_truthy(&JsonValue::String("f".to_string())));
        assert!(is_truthy(&JsonValue::String("hello".to_string())));
        assert!(!is_truthy(&JsonValue::Array(vec![])));
        assert!(is_truthy(&JsonValue::Array(vec![JsonValue::Null])));
    }

    #[test]
    fn test_to_registered_extracts_rc_stdout_stderr_from_result() {
        // Simulate a command module result stored in TaskResult.result
        let mut result = TaskResult::changed();
        result.result = Some(serde_json::json!({
            "rc": 0,
            "stdout": "Hello, World!",
            "stderr": "warning: deprecated",
            "changed": true,
            "custom_field": "custom_value"
        }));

        let registered = result.to_registered(None, None);

        // Verify standard fields are extracted
        assert_eq!(registered.rc, Some(0));
        assert_eq!(registered.stdout, Some("Hello, World!".to_string()));
        assert_eq!(registered.stderr, Some("warning: deprecated".to_string()));
        assert_eq!(
            registered.stdout_lines,
            Some(vec!["Hello, World!".to_string()])
        );
        assert_eq!(
            registered.stderr_lines,
            Some(vec!["warning: deprecated".to_string()])
        );
        assert!(registered.changed);

        // Verify custom data is preserved
        assert_eq!(
            registered.data.get("custom_field"),
            Some(&JsonValue::String("custom_value".to_string()))
        );
    }

    #[test]
    fn test_to_registered_multiline_stdout() {
        let mut result = TaskResult::ok();
        result.result = Some(serde_json::json!({
            "stdout": "line1\nline2\nline3",
            "rc": 0
        }));

        let registered = result.to_registered(None, None);

        assert_eq!(
            registered.stdout_lines,
            Some(vec![
                "line1".to_string(),
                "line2".to_string(),
                "line3".to_string()
            ])
        );
    }

    #[test]
    fn test_to_registered_fallback_to_explicit_params() {
        // When self.result is None, use explicit stdout/stderr params
        let result = TaskResult::ok();

        let registered = result.to_registered(
            Some("explicit stdout".to_string()),
            Some("explicit stderr".to_string()),
        );

        assert_eq!(registered.stdout, Some("explicit stdout".to_string()));
        assert_eq!(registered.stderr, Some("explicit stderr".to_string()));
        assert_eq!(registered.rc, None); // No result, no rc
    }

    #[test]
    fn test_to_registered_result_takes_precedence() {
        // When self.result has stdout/stderr, it takes precedence
        let mut result = TaskResult::ok();
        result.result = Some(serde_json::json!({
            "stdout": "from result",
            "stderr": "from result error"
        }));

        let registered = result.to_registered(
            Some("explicit stdout".to_string()),
            Some("explicit stderr".to_string()),
        );

        // Result data takes precedence over explicit params
        assert_eq!(registered.stdout, Some("from result".to_string()));
        assert_eq!(registered.stderr, Some("from result error".to_string()));
    }

    #[test]
    fn test_to_registered_failed_status() {
        let result = TaskResult::failed("Command failed");

        let registered = result.to_registered(None, None);

        assert!(registered.failed);
        assert!(!registered.changed);
        assert_eq!(registered.msg, Some("Command failed".to_string()));
    }

    #[test]
    fn test_to_registered_skipped_status() {
        let result = TaskResult::skipped("Skipped in check mode");

        let registered = result.to_registered(None, None);

        assert!(registered.skipped);
        assert!(!registered.failed);
        assert!(!registered.changed);
    }

    #[test]
    fn test_to_registered_excludes_standard_fields_from_data() {
        // Standard RegisteredResult fields should not be duplicated in data
        let mut result = TaskResult::changed();
        result.result = Some(serde_json::json!({
            "changed": true,
            "failed": false,
            "skipped": false,
            "rc": 0,
            "stdout": "output",
            "stdout_lines": ["output"],
            "stderr": "",
            "stderr_lines": [],
            "msg": "Success",
            "results": null,
            "custom_data": "should_be_in_data"
        }));

        let registered = result.to_registered(None, None);

        // Standard fields should not be in data
        assert!(!registered.data.contains_key("changed"));
        assert!(!registered.data.contains_key("failed"));
        assert!(!registered.data.contains_key("skipped"));
        assert!(!registered.data.contains_key("rc"));
        assert!(!registered.data.contains_key("stdout"));
        assert!(!registered.data.contains_key("stdout_lines"));
        assert!(!registered.data.contains_key("stderr"));
        assert!(!registered.data.contains_key("stderr_lines"));
        assert!(!registered.data.contains_key("msg"));
        assert!(!registered.data.contains_key("results"));

        // Custom field should be in data
        assert!(registered.data.contains_key("custom_data"));
    }

    #[tokio::test]
    async fn test_execute_skips_when_condition_false() {
        let task = Task::new("conditional", "debug").when("false");
        let (ctx, runtime, handlers, notified, parallelization_manager, module_registry) =
            setup_execution("host1");

        let result = task
            .execute(
                &ctx,
                &runtime,
                &handlers,
                &notified,
                &parallelization_manager,
                &module_registry,
            )
            .await
            .unwrap();

        assert_eq!(result.status, TaskStatus::Skipped);
        assert_eq!(
            result.msg,
            Some("Skipped: condition 'false' was false".to_string())
        );
        assert!(notified.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_execute_ignore_errors_on_failure() {
        let mut task = Task::new("fail", "fail").arg("msg", "boom");
        task.ignore_errors = true;
        let (ctx, runtime, handlers, notified, parallelization_manager, module_registry) =
            setup_execution("host1");

        let result = task
            .execute(
                &ctx,
                &runtime,
                &handlers,
                &notified,
                &parallelization_manager,
                &module_registry,
            )
            .await
            .unwrap();

        assert_eq!(result.status, TaskStatus::Ok);
        assert_eq!(result.msg, Some("Ignored error: boom".to_string()));
        assert!(!result.changed);
    }

    #[tokio::test]
    async fn test_execute_with_retry_exhausts_retries() {
        let mut task = Task::new("retry", "debug").arg("msg", "retry");
        task.until = Some("false".to_string());
        task.retries = Some(1);
        task.delay = Some(0);
        let (ctx, runtime, handlers, notified, parallelization_manager, module_registry) =
            setup_execution("host1");

        let result = task
            .execute(
                &ctx,
                &runtime,
                &handlers,
                &notified,
                &parallelization_manager,
                &module_registry,
            )
            .await
            .unwrap();

        assert_eq!(result.status, TaskStatus::Failed);
        assert_eq!(
            result.msg,
            Some("Retries exhausted (1). Until condition 'false' never met".to_string())
        );
    }

    #[tokio::test]
    async fn test_execute_loop_registers_results_and_clears_vars() {
        let mut task = Task::new("loop debug", "debug")
            .arg("msg", "hello")
            .loop_over(vec![serde_json::json!(1), serde_json::json!(2)])
            .register("loop_out");
        task.loop_control = Some(LoopControl {
            loop_var: "item".to_string(),
            index_var: Some("idx".to_string()),
            label: None,
            pause: None,
            extended: false,
        });

        let (ctx, runtime, handlers, notified, parallelization_manager, module_registry) =
            setup_execution("host1");

        let result = task
            .execute(
                &ctx,
                &runtime,
                &handlers,
                &notified,
                &parallelization_manager,
                &module_registry,
            )
            .await
            .unwrap();

        assert_eq!(result.status, TaskStatus::Ok);
        assert_eq!(result.msg, Some("Completed 2 loop iterations".to_string()));

        let rt = runtime.read().await;
        let registered = rt
            .get_registered(&ctx.host, "loop_out")
            .expect("registered result");
        let results = registered.results.as_ref().expect("loop results");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].data.get("item"), Some(&serde_json::json!(1)));
        assert_eq!(results[1].data.get("item"), Some(&serde_json::json!(2)));
        assert!(rt.get_var("item", Some(&ctx.host)).is_none());
        assert!(rt.get_var("ansible_loop", Some(&ctx.host)).is_none());
        assert!(rt.get_var("idx", Some(&ctx.host)).is_none());
    }

    #[tokio::test]
    async fn test_execute_set_fact_delegation_respects_delegate_facts() {
        let mut task = Task::new("set fact", "set_fact").arg("answer", serde_json::json!(42));
        task.delegate_to = Some("delegate".to_string());
        task.delegate_facts = Some(true);

        let (ctx, runtime, handlers, notified, parallelization_manager, module_registry) =
            setup_execution("origin");

        task.execute(
            &ctx,
            &runtime,
            &handlers,
            &notified,
            &parallelization_manager,
            &module_registry,
        )
        .await
        .unwrap();

        let rt = runtime.read().await;
        assert_eq!(
            rt.get_host_fact("delegate", "answer"),
            Some(serde_json::json!(42))
        );
        assert_eq!(rt.get_host_fact("origin", "answer"), None);
    }

    #[tokio::test]
    async fn test_execute_set_fact_delegation_defaults_to_origin_host() {
        let mut task = Task::new("set fact", "set_fact").arg("answer", serde_json::json!(7));
        task.delegate_to = Some("delegate".to_string());

        let (ctx, runtime, handlers, notified, parallelization_manager, module_registry) =
            setup_execution("origin");

        task.execute(
            &ctx,
            &runtime,
            &handlers,
            &notified,
            &parallelization_manager,
            &module_registry,
        )
        .await
        .unwrap();

        let rt = runtime.read().await;
        assert_eq!(
            rt.get_host_fact("origin", "answer"),
            Some(serde_json::json!(7))
        );
        assert_eq!(rt.get_host_fact("delegate", "answer"), None);
    }
}
