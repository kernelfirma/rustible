//! Playbook structure definitions for Rustible.
//!
//! This module defines the data structures for Ansible-compatible playbooks,
//! including plays, tasks, handlers, and role inclusions.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

/// Helper function to deserialize flexible booleans (yes/no/true/false/1/0)
fn deserialize_flexible_bool<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let value = serde_yaml::Value::deserialize(deserializer)?;
    match &value {
        serde_yaml::Value::Bool(b) => Ok(*b),
        serde_yaml::Value::String(s) => match s.to_lowercase().as_str() {
            "yes" | "true" | "on" | "1" | "y" | "t" => Ok(true),
            "no" | "false" | "off" | "0" | "" | "n" | "f" => Ok(false),
            _ => Err(D::Error::custom(format!("invalid boolean string: {}", s))),
        },
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i != 0)
            } else if let Some(f) = n.as_f64() {
                Ok(f != 0.0)
            } else {
                Err(D::Error::custom("invalid boolean number"))
            }
        }
        serde_yaml::Value::Null => Ok(false),
        _ => Err(D::Error::custom(format!(
            "invalid boolean value: {:?}",
            value
        ))),
    }
}

/// Helper function to deserialize optional flexible booleans
fn deserialize_option_flexible_bool<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let value = Option::<serde_yaml::Value>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_yaml::Value::Null) => Ok(None),
        Some(serde_yaml::Value::Bool(b)) => Ok(Some(b)),
        Some(serde_yaml::Value::String(s)) => match s.to_lowercase().as_str() {
            "yes" | "true" | "on" | "1" | "y" | "t" => Ok(Some(true)),
            "no" | "false" | "off" | "0" | "" | "n" | "f" => Ok(Some(false)),
            _ => Err(D::Error::custom(format!("invalid boolean string: {}", s))),
        },
        Some(serde_yaml::Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Ok(Some(i != 0))
            } else if let Some(f) = n.as_f64() {
                Ok(Some(f != 0.0))
            } else {
                Err(D::Error::custom("invalid boolean number"))
            }
        }
        Some(other) => Err(D::Error::custom(format!(
            "invalid boolean value: {:?}",
            other
        ))),
    }
}

/// Helper function to deserialize string or sequence into Vec<String>
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_yaml::Value::deserialize(deserializer)?;
    match value {
        serde_yaml::Value::Null => Ok(Vec::new()),
        serde_yaml::Value::String(s) => Ok(vec![s]),
        serde_yaml::Value::Bool(b) => Ok(vec![b.to_string()]),
        serde_yaml::Value::Number(n) => Ok(vec![n.to_string()]),
        serde_yaml::Value::Sequence(seq) => {
            let mut result = Vec::new();
            for item in seq {
                match item {
                    serde_yaml::Value::String(s) => result.push(s),
                    serde_yaml::Value::Bool(b) => result.push(b.to_string()),
                    serde_yaml::Value::Number(n) => result.push(n.to_string()),
                    other => result.push(format!("{:?}", other)),
                }
            }
            Ok(result)
        }
        other => Ok(vec![format!("{:?}", other)]),
    }
}

/// Deserialize a YAML sequence, treating `null`/`~` as an empty list.
fn deserialize_seq_or_null<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// A complete playbook containing multiple plays
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    /// Ordered list of plays
    pub plays: Vec<Play>,

    /// Source file path
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

impl Playbook {
    /// Create a new empty playbook
    pub fn new() -> Self {
        Self {
            plays: Vec::new(),
            source_path: None,
        }
    }

    /// Create a playbook with a source path
    pub fn with_source(source: PathBuf) -> Self {
        Self {
            plays: Vec::new(),
            source_path: Some(source),
        }
    }

    /// Add a play to the playbook
    pub fn add_play(&mut self, play: Play) {
        self.plays.push(play);
    }

    /// Get the number of plays
    pub fn play_count(&self) -> usize {
        self.plays.len()
    }

    /// Get total number of tasks across all plays
    pub fn task_count(&self) -> usize {
        self.plays.iter().map(|p| p.tasks.len()).sum()
    }
}

impl Default for Playbook {
    fn default() -> Self {
        Self::new()
    }
}

/// A play targeting a set of hosts with a list of tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Play {
    /// Play name/description
    #[serde(default)]
    pub name: String,

    /// Host pattern to target
    pub hosts: String,

    /// Gather facts before running tasks
    #[serde(
        default = "default_gather_facts",
        deserialize_with = "deserialize_flexible_bool"
    )]
    pub gather_facts: bool,

    /// Gather facts subset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gather_subset: Option<Vec<String>>,

    /// Gather facts timeout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gather_timeout: Option<u32>,

    /// Remote user for SSH
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_user: Option<String>,

    /// Enable privilege escalation
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub r#become: bool,

    /// Privilege escalation method
    #[serde(skip_serializing_if = "Option::is_none")]
    pub become_method: Option<String>,

    /// User to become
    #[serde(skip_serializing_if = "Option::is_none")]
    pub become_user: Option<String>,

    /// Connection type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,

    /// Environment variables for all tasks
    #[serde(default)]
    pub environment: IndexMap<String, String>,

    /// Play-level variables
    #[serde(default)]
    pub vars: IndexMap<String, serde_yaml::Value>,

    /// Variable files to include
    #[serde(default)]
    pub vars_files: Vec<String>,

    /// Variable prompts
    #[serde(default)]
    pub vars_prompt: Vec<VarsPrompt>,

    /// Pre-tasks (run before roles)
    #[serde(default)]
    pub pre_tasks: Vec<Task>,

    /// Roles to include
    #[serde(default)]
    pub roles: Vec<RoleInclusion>,

    /// Main tasks
    #[serde(default)]
    pub tasks: Vec<Task>,

    /// Post-tasks (run after roles and tasks)
    #[serde(default)]
    pub post_tasks: Vec<Task>,

    /// Handlers
    #[serde(default)]
    pub handlers: Vec<Handler>,

    /// Maximum concurrent hosts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<SerialSpec>,

    /// Maximum failure percentage before aborting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_fail_percentage: Option<f32>,

    /// Continue on errors
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub ignore_errors: bool,

    /// Continue on unreachable hosts
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub ignore_unreachable: bool,

    /// Module defaults
    #[serde(default)]
    pub module_defaults: IndexMap<String, IndexMap<String, serde_yaml::Value>>,

    /// Play tags
    #[serde(default)]
    pub tags: Vec<String>,

    /// Strategy (linear, free, debug)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,

    /// Throttle (limit concurrent tasks per host)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle: Option<u32>,

    /// Order of host execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<PlayOrder>,

    /// Force all handlers to run at end of play
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub force_handlers: bool,

    /// Run once on first host only
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub run_once: bool,

    /// Conditional execution
    #[serde(
        default,
        rename = "when",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub when_condition: Vec<String>,

    /// Become password (should be from vault/prompt)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub become_password: Option<String>,

    /// Any host is ok (don't fail if some hosts unreachable)
    #[serde(default)]
    pub any_errors_fatal: bool,
}

fn default_gather_facts() -> bool {
    true
}

impl Play {
    /// Create a new play targeting the specified hosts
    pub fn new(hosts: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            hosts: hosts.into(),
            gather_facts: true,
            gather_subset: None,
            gather_timeout: None,
            remote_user: None,
            r#become: false,
            become_method: None,
            become_user: None,
            connection: None,
            environment: IndexMap::new(),
            vars: IndexMap::new(),
            vars_files: Vec::new(),
            vars_prompt: Vec::new(),
            pre_tasks: Vec::new(),
            roles: Vec::new(),
            tasks: Vec::new(),
            post_tasks: Vec::new(),
            handlers: Vec::new(),
            serial: None,
            max_fail_percentage: None,
            ignore_errors: false,
            ignore_unreachable: false,
            module_defaults: IndexMap::new(),
            tags: Vec::new(),
            strategy: None,
            throttle: None,
            order: None,
            force_handlers: false,
            run_once: false,
            when_condition: Vec::new(),
            become_password: None,
            any_errors_fatal: false,
        }
    }

    /// Set the play name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Add a task to the play
    pub fn add_task(&mut self, task: Task) {
        self.tasks.push(task);
    }

    /// Add a handler to the play
    pub fn add_handler(&mut self, handler: Handler) {
        self.handlers.push(handler);
    }

    /// Add a role to the play
    pub fn add_role(&mut self, role: RoleInclusion) {
        self.roles.push(role);
    }

    /// Set a variable
    pub fn set_var(&mut self, key: impl Into<String>, value: serde_yaml::Value) {
        self.vars.insert(key.into(), value);
    }

    /// Get all tasks including pre_tasks, role tasks, tasks, and post_tasks
    pub fn all_tasks(&self) -> impl Iterator<Item = &Task> {
        self.pre_tasks
            .iter()
            .chain(self.tasks.iter())
            .chain(self.post_tasks.iter())
    }
}

impl Default for Play {
    fn default() -> Self {
        Self::new("all")
    }
}

/// Serial execution specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SerialSpec {
    /// Fixed number of hosts
    Count(u32),
    /// Percentage of hosts
    Percentage(String),
    /// List of batch sizes
    Batches(Vec<SerialBatch>),
}

/// A single batch in serial execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SerialBatch {
    Count(u32),
    Percentage(String),
}

/// Host execution order
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayOrder {
    /// Default inventory order
    Inventory,
    /// Reverse inventory order
    Reverse,
    /// Sorted alphabetically
    Sorted,
    /// Reverse sorted
    ReverseSorted,
    /// Random order
    Shuffle,
}

/// Variable prompt definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VarsPrompt {
    /// Variable name
    pub name: String,

    /// Prompt message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,

    /// Default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,

    /// Hide input (for passwords)
    #[serde(default)]
    pub private: bool,

    /// Confirm input
    #[serde(default)]
    pub confirm: bool,

    /// Encrypt with hash algorithm
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypt: Option<String>,

    /// Salt size for encryption
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salt_size: Option<u32>,
}

/// A task definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Task name
    #[serde(default)]
    pub name: String,

    /// The module to execute (action)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<ModuleCall>,

    /// Short module syntax (module_name: args)
    #[serde(flatten)]
    pub module: IndexMap<String, serde_yaml::Value>,

    /// Conditional execution
    #[serde(
        default,
        rename = "when",
        deserialize_with = "deserialize_string_or_vec"
    )]
    pub when_condition: Vec<String>,

    /// Loop over items
    #[serde(skip_serializing_if = "Option::is_none", rename = "loop")]
    pub loop_over: Option<LoopSpec>,

    /// Legacy with_* loops
    #[serde(flatten)]
    pub with_loops: IndexMap<String, serde_yaml::Value>,

    /// Loop control options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_control: Option<LoopControl>,

    /// Register result in variable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub register: Option<String>,

    /// Handlers to notify on change
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub notify: Vec<String>,

    /// Ignore errors
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub ignore_errors: bool,

    /// Ignore unreachable hosts
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub ignore_unreachable: bool,

    /// Changed when condition
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub changed_when: Vec<String>,

    /// Failed when condition
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub failed_when: Vec<String>,

    /// Task tags
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub tags: Vec<String>,

    /// Become (privilege escalation)
    #[serde(
        skip_serializing_if = "Option::is_none",
        default,
        deserialize_with = "deserialize_option_flexible_bool"
    )]
    pub r#become: Option<bool>,

    /// Become method
    #[serde(skip_serializing_if = "Option::is_none")]
    pub become_method: Option<String>,

    /// Become user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub become_user: Option<String>,

    /// Delegate to another host
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate_to: Option<String>,

    /// Delegate facts
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub delegate_facts: bool,

    /// Local action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_action: Option<ModuleCall>,

    /// Run once (only on first host)
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub run_once: bool,

    /// Retry count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,

    /// Delay between retries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<u32>,

    /// Until condition for retries
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub until: Vec<String>,

    /// Async execution timeout
    #[serde(skip_serializing_if = "Option::is_none", rename = "async")]
    pub async_timeout: Option<u32>,

    /// Polling interval for async
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll: Option<u32>,

    /// Environment variables
    #[serde(default)]
    pub environment: IndexMap<String, String>,

    /// Task-level variables
    #[serde(default)]
    pub vars: IndexMap<String, serde_yaml::Value>,

    /// Args for the module
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<IndexMap<String, serde_yaml::Value>>,

    /// Block of tasks (for block/rescue/always)
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub block: Vec<Task>,

    /// Rescue tasks (run on block failure)
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub rescue: Vec<Task>,

    /// Always tasks (always run after block)
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub always: Vec<Task>,

    /// Connection type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,

    /// Throttle (limit concurrent executions)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttle: Option<u32>,

    /// Timeout for task execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,

    /// No log (hide output for sensitive data)
    #[serde(default)]
    pub no_log: bool,

    /// Diff mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<bool>,

    /// Check mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_mode: Option<bool>,

    /// Module defaults group
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_defaults: Option<String>,

    /// Any errors are fatal
    #[serde(default)]
    pub any_errors_fatal: bool,
}

impl Task {
    /// Create a new task with a name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            action: None,
            module: IndexMap::new(),
            when_condition: Vec::new(),
            loop_over: None,
            with_loops: IndexMap::new(),
            loop_control: None,
            register: None,
            notify: Vec::new(),
            ignore_errors: false,
            ignore_unreachable: false,
            changed_when: Vec::new(),
            failed_when: Vec::new(),
            tags: Vec::new(),
            r#become: None,
            become_method: None,
            become_user: None,
            delegate_to: None,
            delegate_facts: false,
            local_action: None,
            run_once: false,
            retries: None,
            delay: None,
            until: Vec::new(),
            async_timeout: None,
            poll: None,
            environment: IndexMap::new(),
            vars: IndexMap::new(),
            args: None,
            block: Vec::new(),
            rescue: Vec::new(),
            always: Vec::new(),
            connection: None,
            throttle: None,
            timeout: None,
            no_log: false,
            diff: None,
            check_mode: None,
            module_defaults: None,
            any_errors_fatal: false,
        }
    }

    /// Create a task with a module call
    pub fn with_module(
        name: impl Into<String>,
        module: impl Into<String>,
        args: serde_yaml::Value,
    ) -> Self {
        let mut task = Self::new(name);
        task.module.insert(module.into(), args);
        task
    }

    /// Check if this is a block task
    pub fn is_block(&self) -> bool {
        !self.block.is_empty()
    }

    /// Get the module name being called
    pub fn get_module_name(&self) -> Option<&str> {
        // Check action first
        if let Some(action) = &self.action {
            return Some(&action.module);
        }

        // Check local_action
        if let Some(local) = &self.local_action {
            return Some(&local.module);
        }

        // Check module shorthand (skip known non-module keys)
        let non_module_keys = [
            "name",
            "when",
            "loop",
            "register",
            "notify",
            "ignore_errors",
            "ignore_unreachable",
            "changed_when",
            "failed_when",
            "tags",
            "become",
            "become_method",
            "become_user",
            "delegate_to",
            "delegate_facts",
            "run_once",
            "retries",
            "delay",
            "until",
            "async",
            "poll",
            "environment",
            "vars",
            "args",
            "block",
            "rescue",
            "always",
            "connection",
            "throttle",
            "timeout",
            "no_log",
            "diff",
            "check_mode",
            "module_defaults",
            "any_errors_fatal",
            "loop_control",
        ];

        for key in self.module.keys() {
            if !non_module_keys.contains(&key.as_str()) && !key.starts_with("with_") {
                return Some(key);
            }
        }

        None
    }

    /// Get the module arguments
    pub fn get_module_args(&self) -> Option<&serde_yaml::Value> {
        if let Some(action) = &self.action {
            return Some(&action.args);
        }

        if let Some(local) = &self.local_action {
            return Some(&local.args);
        }

        if let Some(module_name) = self.get_module_name() {
            return self.module.get(module_name);
        }

        self.args.as_ref().map(|_args| {
            // Convert IndexMap to Value - this is a workaround
            // In real code, we'd handle this differently
            static EMPTY: serde_yaml::Value = serde_yaml::Value::Null;
            &EMPTY
        })
    }
}

impl Default for Task {
    fn default() -> Self {
        Self::new("")
    }
}

/// A module call specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleCall {
    /// Module name
    pub module: String,

    /// Module arguments
    #[serde(default)]
    pub args: serde_yaml::Value,
}

impl ModuleCall {
    /// Create a new module call
    pub fn new(module: impl Into<String>) -> Self {
        Self {
            module: module.into(),
            args: serde_yaml::Value::Null,
        }
    }

    /// Create a module call with arguments
    pub fn with_args(module: impl Into<String>, args: serde_yaml::Value) -> Self {
        Self {
            module: module.into(),
            args,
        }
    }
}

/// Loop specification
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoopSpec {
    /// Simple list
    List(Vec<serde_yaml::Value>),
    /// Expression (template string)
    Expression(String),
}

/// Loop control options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopControl {
    /// Variable name for current item
    #[serde(default = "default_loop_var")]
    pub loop_var: String,

    /// Variable name for index
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_var: Option<String>,

    /// Label for output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Pause between iterations (seconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause: Option<f32>,

    /// Extended loop information
    #[serde(default)]
    pub extended: bool,
}

fn default_loop_var() -> String {
    "item".to_string()
}

impl Default for LoopControl {
    fn default() -> Self {
        Self {
            loop_var: default_loop_var(),
            index_var: None,
            label: None,
            pause: None,
            extended: false,
        }
    }
}

/// Handler definition (task triggered by notify)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handler {
    /// Handler name (optional if listen is provided)
    #[serde(default)]
    pub name: String,

    /// Handler is actually a task
    #[serde(flatten)]
    pub task: Task,

    /// Listen to additional trigger names
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub listen: Vec<String>,
}

impl Handler {
    /// Create a new handler
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            name: name.clone(),
            task: Task::new(&name),
            listen: Vec::new(),
        }
    }

    /// Create a handler with a module call
    pub fn with_module(
        name: impl Into<String>,
        module: impl Into<String>,
        args: serde_yaml::Value,
    ) -> Self {
        let name = name.into();
        Self {
            name: name.clone(),
            task: Task::with_module(&name, module, args),
            listen: Vec::new(),
        }
    }

    /// Check if this handler responds to a notification
    pub fn responds_to(&self, notification: &str) -> bool {
        (!self.name.is_empty() && self.name == notification)
            || self.listen.contains(&notification.to_string())
    }
}

/// Role inclusion in a play
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RoleInclusion {
    /// Simple role name
    Name(String),
    /// Role with parameters
    Full(RoleSpec),
}

impl RoleInclusion {
    /// Get the role name
    pub fn name(&self) -> &str {
        match self {
            RoleInclusion::Name(name) => name,
            RoleInclusion::Full(spec) => &spec.role,
        }
    }

    /// Get role variables
    pub fn vars(&self) -> Option<&IndexMap<String, serde_yaml::Value>> {
        match self {
            RoleInclusion::Name(_) => None,
            RoleInclusion::Full(spec) => Some(&spec.vars),
        }
    }
}

/// Full role specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleSpec {
    /// Role name or path
    pub role: String,

    /// Role variables
    #[serde(default)]
    pub vars: IndexMap<String, serde_yaml::Value>,

    /// Tags for the role
    #[serde(default)]
    pub tags: Vec<String>,

    /// Conditional execution
    #[serde(default, rename = "when")]
    pub when_condition: Vec<String>,

    /// Become settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#become: Option<bool>,

    /// Become user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub become_user: Option<String>,

    /// Delegate to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate_to: Option<String>,

    /// Apply settings (for include_role/import_role)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply: Option<TaskApply>,

    /// Public (expose vars)
    #[serde(default)]
    pub public: bool,

    /// Allow duplicates
    #[serde(default)]
    pub allow_duplicates: bool,

    /// Handlers from (use handlers from another role)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handlers_from: Option<String>,

    /// Tasks from (use tasks from another role)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tasks_from: Option<String>,

    /// Vars from (use vars from another role)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vars_from: Option<String>,

    /// Defaults from (use defaults from another role)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults_from: Option<String>,
}

impl RoleSpec {
    /// Create a new role specification
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            vars: IndexMap::new(),
            tags: Vec::new(),
            when_condition: Vec::new(),
            r#become: None,
            become_user: None,
            delegate_to: None,
            apply: None,
            public: false,
            allow_duplicates: false,
            handlers_from: None,
            tasks_from: None,
            vars_from: None,
            defaults_from: None,
        }
    }

    /// Set a variable
    pub fn set_var(&mut self, key: impl Into<String>, value: serde_yaml::Value) {
        self.vars.insert(key.into(), value);
    }
}

/// Apply settings for dynamic includes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskApply {
    /// Tags to apply
    #[serde(default)]
    pub tags: Vec<String>,

    /// Become settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#become: Option<bool>,

    /// Other settings
    #[serde(flatten)]
    pub other: IndexMap<String, serde_yaml::Value>,
}

/// Include types for importing external files
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncludeType {
    /// Import playbook (static)
    ImportPlaybook(String),
    /// Include tasks (dynamic)
    IncludeTasks(String),
    /// Import tasks (static)
    ImportTasks(String),
    /// Include role (dynamic)
    IncludeRole(RoleSpec),
    /// Import role (static)
    ImportRole(RoleSpec),
    /// Include vars
    IncludeVars(String),
}

/// Builder for creating tasks
#[derive(Debug, Default)]
pub struct TaskBuilder {
    name: String,
    module: Option<String>,
    args: IndexMap<String, serde_yaml::Value>,
    when: Vec<String>,
    register: Option<String>,
    notify: Vec<String>,
    tags: Vec<String>,
    r#become: Option<bool>,
    ignore_errors: bool,
    loop_over: Option<LoopSpec>,
}

impl TaskBuilder {
    /// Create a new task builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Set the module
    pub fn module(mut self, module: impl Into<String>) -> Self {
        self.module = Some(module.into());
        self
    }

    /// Add an argument
    pub fn arg(mut self, key: impl Into<String>, value: serde_yaml::Value) -> Self {
        self.args.insert(key.into(), value);
        self
    }

    /// Add a when condition
    pub fn when(mut self, condition: impl Into<String>) -> Self {
        self.when.push(condition.into());
        self
    }

    /// Register result
    pub fn register(mut self, var: impl Into<String>) -> Self {
        self.register = Some(var.into());
        self
    }

    /// Add a notify handler
    pub fn notify(mut self, handler: impl Into<String>) -> Self {
        self.notify.push(handler.into());
        self
    }

    /// Add a tag
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Enable become
    pub fn r#become(mut self, r#become: bool) -> Self {
        self.r#become = Some(r#become);
        self
    }

    /// Ignore errors
    pub fn ignore_errors(mut self, ignore: bool) -> Self {
        self.ignore_errors = ignore;
        self
    }

    /// Add a loop
    pub fn loop_items(mut self, items: Vec<serde_yaml::Value>) -> Self {
        self.loop_over = Some(LoopSpec::List(items));
        self
    }

    /// Build the task
    pub fn build(self) -> Task {
        let mut task = Task::new(self.name);

        if let Some(module) = self.module {
            if self.args.is_empty() {
                task.module.insert(module, serde_yaml::Value::Null);
            } else {
                let mut map = serde_yaml::Mapping::new();
                for (k, v) in self.args {
                    map.insert(serde_yaml::Value::String(k), v);
                }
                task.module.insert(module, serde_yaml::Value::Mapping(map));
            }
        }

        task.when_condition = self.when;
        task.register = self.register;
        task.notify = self.notify;
        task.tags = self.tags;
        task.r#become = self.r#become;
        task.ignore_errors = self.ignore_errors;
        task.loop_over = self.loop_over;

        task
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playbook_new() {
        let playbook = Playbook::new();
        assert_eq!(playbook.play_count(), 0);
    }

    #[test]
    fn test_play_new() {
        let play = Play::new("webservers");
        assert_eq!(play.hosts, "webservers");
        assert!(play.gather_facts);
    }

    #[test]
    fn test_task_builder() {
        let task = TaskBuilder::new("Install nginx")
            .module("apt")
            .arg("name", serde_yaml::Value::String("nginx".into()))
            .arg("state", serde_yaml::Value::String("present".into()))
            .r#become(true)
            .notify("restart nginx")
            .build();

        assert_eq!(task.name, "Install nginx");
        assert_eq!(task.r#become, Some(true));
        assert!(task.notify.contains(&"restart nginx".to_string()));
    }

    #[test]
    fn test_task_boolean_variants() {
        let yaml = r#"
name: Boolean task
ignore_errors: y
ignore_unreachable: f
"#;

        let task: Task = serde_yaml::from_str(yaml).unwrap();
        assert!(task.ignore_errors);
        assert!(!task.ignore_unreachable);
    }

    #[test]
    fn test_task_block_null_defaults() {
        let yaml = r#"
name: Block task
block:
rescue:
always:
"#;

        let task: Task = serde_yaml::from_str(yaml).unwrap();
        assert!(task.block.is_empty());
        assert!(task.rescue.is_empty());
        assert!(task.always.is_empty());
    }

    #[test]
    fn test_handler() {
        let handler = Handler::with_module(
            "restart nginx",
            "service",
            serde_yaml::Value::String("nginx state=restarted".into()),
        );

        assert!(handler.responds_to("restart nginx"));
        assert!(!handler.responds_to("stop nginx"));
    }

    #[test]
    fn test_role_inclusion() {
        let simple = RoleInclusion::Name("common".to_string());
        assert_eq!(simple.name(), "common");

        let full = RoleInclusion::Full(RoleSpec::new("nginx"));
        assert_eq!(full.name(), "nginx");
    }
}
