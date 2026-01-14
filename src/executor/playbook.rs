//! Playbook runner for Rustible
//!
//! This module provides:
//! - Play execution
//! - Role inclusion
//! - Import/include task files

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value as JsonValue;
use tracing::debug;

/// Helper function to deserialize flexible booleans (yes/no/true/false/1/0)
fn deserialize_flexible_bool<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let value = JsonValue::deserialize(deserializer)?;
    match &value {
        JsonValue::Bool(b) => Ok(*b),
        JsonValue::String(s) => match s.to_lowercase().as_str() {
            "yes" | "true" | "on" | "1" => Ok(true),
            "no" | "false" | "off" | "0" | "" => Ok(false),
            _ => Err(D::Error::custom(format!("invalid boolean string: {}", s))),
        },
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i != 0)
            } else {
                Err(D::Error::custom("invalid boolean number"))
            }
        }
        JsonValue::Null => Ok(false),
        _ => Err(D::Error::custom(format!(
            "invalid boolean value: {:?}",
            value
        ))),
    }
}

/// Helper function to deserialize optional flexible booleans
#[allow(dead_code)]
fn deserialize_option_flexible_bool<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let value = Option::<JsonValue>::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::Bool(b)) => Ok(Some(b)),
        Some(JsonValue::String(s)) => match s.to_lowercase().as_str() {
            "yes" | "true" | "on" | "1" => Ok(Some(true)),
            "no" | "false" | "off" | "0" | "" => Ok(Some(false)),
            _ => Err(D::Error::custom(format!("invalid boolean string: {}", s))),
        },
        Some(JsonValue::Number(n)) => {
            if let Some(i) = n.as_i64() {
                Ok(Some(i != 0))
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

/// Deserialize a YAML/JSON sequence, treating `null`/`~` as an empty list.
fn deserialize_seq_or_null<'de, D, T>(deserializer: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
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

use crate::executor::task::{Handler, Task};
use crate::executor::{ExecutorError, ExecutorResult};

/// A complete playbook containing multiple plays
#[derive(Debug, Clone, Default)]
pub struct Playbook {
    /// Name of the playbook
    pub name: String,
    /// Path to the playbook file
    pub path: Option<PathBuf>,
    /// Global variables for the playbook
    pub vars: IndexMap<String, JsonValue>,
    /// Var files to include
    pub vars_files: Vec<String>,
    /// Plays in this playbook
    pub plays: Vec<Play>,
}

impl Playbook {
    /// Create a new empty playbook
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// Load a playbook from a YAML file
    pub fn load<P: AsRef<Path>>(path: P) -> ExecutorResult<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(|e| ExecutorError::IoError(e))?;

        Self::parse(&content, Some(path.to_path_buf()))
    }

    /// Parse a playbook from YAML content
    pub fn parse(content: &str, path: Option<PathBuf>) -> ExecutorResult<Self> {
        // Ansible playbooks are arrays of plays at the top level
        let plays: Vec<PlayDefinition> = serde_yaml::from_str(content)
            .map_err(|e| ExecutorError::ParseError(format!("YAML parse error: {}", e)))?;

        if plays.is_empty() {
            return Err(ExecutorError::ParseError(
                "Playbook must contain at least one play".to_string(),
            ));
        }

        let mut playbook = Playbook::default();
        playbook.path = path.clone();

        if let Some(ref p) = path {
            playbook.name = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string();
        }

        for play_def in plays {
            let play = Play::from_definition(play_def, path.as_ref())?;
            playbook.plays.push(play);
        }

        Ok(playbook)
    }

    /// Add a play to the playbook
    pub fn add_play(&mut self, play: Play) {
        self.plays.push(play);
    }

    /// Set a variable
    pub fn set_var(&mut self, name: impl Into<String>, value: JsonValue) {
        self.vars.insert(name.into(), value);
    }

    /// Get the playbook directory
    pub fn get_playbook_dir(&self) -> Option<PathBuf> {
        self.path
            .as_ref()
            .and_then(|p| p.parent().map(PathBuf::from))
    }
}

/// Raw play definition from YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayDefinition {
    /// Play name
    #[serde(default)]
    pub name: String,
    /// Target hosts pattern
    #[serde(default = "default_hosts")]
    pub hosts: String,
    /// Gather facts
    #[serde(
        default = "default_gather_facts",
        deserialize_with = "deserialize_flexible_bool"
    )]
    pub gather_facts: bool,
    /// Become root
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub r#become: bool,
    /// User to become
    #[serde(default)]
    pub become_user: Option<String>,
    /// Become method
    #[serde(default)]
    pub become_method: Option<String>,
    /// Connection type
    #[serde(default)]
    pub connection: Option<String>,
    /// Remote user
    #[serde(default)]
    pub remote_user: Option<String>,
    /// Play variables
    #[serde(default)]
    pub vars: IndexMap<String, JsonValue>,
    /// Variable files to include
    #[serde(default)]
    pub vars_files: Vec<String>,
    /// Roles to include
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub roles: Vec<RoleDefinition>,
    /// Pre-tasks (run before roles)
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub pre_tasks: Vec<TaskDefinition>,
    /// Main tasks
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub tasks: Vec<TaskDefinition>,
    /// Post-tasks (run after roles)
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub post_tasks: Vec<TaskDefinition>,
    /// Handlers
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub handlers: Vec<HandlerDefinition>,
    /// Environment variables
    #[serde(default)]
    pub environment: IndexMap<String, JsonValue>,
    /// Tags for this play
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub tags: Vec<String>,
    /// Serial execution (number of hosts at a time)
    #[serde(default)]
    pub serial: Option<SerialValue>,
    /// Maximum failure percentage
    #[serde(default)]
    pub max_fail_percentage: Option<u8>,
    /// Strategy override
    #[serde(default)]
    pub strategy: Option<String>,
    /// Ignore unreachable hosts
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub ignore_unreachable: bool,
    /// Force handlers to run even if play fails
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub force_handlers: bool,
    /// Fact gathering subset
    #[serde(default, deserialize_with = "deserialize_seq_or_null")]
    pub gather_subset: Vec<String>,
    /// Order of host execution
    #[serde(default)]
    pub order: Option<String>,
    /// Module defaults
    #[serde(default)]
    pub module_defaults: IndexMap<String, IndexMap<String, JsonValue>>,
}

fn default_hosts() -> String {
    "all".to_string()
}

fn default_gather_facts() -> bool {
    true
}

/// Serial value can be a number, percentage, or list
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SerialValue {
    Number(usize),
    Percentage(String),
    List(Vec<SerialValue>),
}

/// Role definition in a play
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RoleDefinition {
    /// Simple role name
    Name(String),
    /// Role with parameters
    Full {
        /// Role name or path
        #[serde(alias = "name")]
        role: String,
        /// When condition
        #[serde(default)]
        when: Option<String>,
        /// Tags
        #[serde(default)]
        tags: Vec<String>,
        /// Role variables
        #[serde(default)]
        vars: IndexMap<String, JsonValue>,
        /// Become override
        #[serde(default)]
        r#become: Option<bool>,
        /// Task include options
        #[serde(default)]
        tasks_from: Option<String>,
        #[serde(default)]
        vars_from: Option<String>,
        #[serde(default)]
        defaults_from: Option<String>,
        #[serde(default)]
        handlers_from: Option<String>,
    },
}

impl RoleDefinition {
    /// Get the role name
    pub fn name(&self) -> &str {
        match self {
            RoleDefinition::Name(name) => name,
            RoleDefinition::Full { role, .. } => role,
        }
    }

    /// Get role variables
    pub fn vars(&self) -> IndexMap<String, JsonValue> {
        match self {
            RoleDefinition::Name(_) => IndexMap::new(),
            RoleDefinition::Full { vars, .. } => vars.clone(),
        }
    }

    /// Get when condition
    pub fn when(&self) -> Option<&str> {
        match self {
            RoleDefinition::Name(_) => None,
            RoleDefinition::Full { when, .. } => when.as_deref(),
        }
    }
}

/// Task definition from YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDefinition {
    /// Task name
    #[serde(default)]
    pub name: String,
    /// When condition
    #[serde(default)]
    pub when: Option<WhenCondition>,
    /// Register result
    #[serde(default)]
    pub register: Option<String>,
    /// Notify handlers
    #[serde(default)]
    pub notify: NotifyValue,
    /// Loop items
    #[serde(default, alias = "loop", alias = "with_items", alias = "with_list")]
    pub loop_items: Option<LoopValue>,
    /// Loop control
    #[serde(default)]
    pub loop_control: Option<LoopControl>,
    /// Ignore errors
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub ignore_errors: bool,
    /// Changed when condition
    #[serde(default)]
    pub changed_when: Option<WhenCondition>,
    /// Failed when condition
    #[serde(default)]
    pub failed_when: Option<WhenCondition>,
    /// Delegate to host
    #[serde(default)]
    pub delegate_to: Option<String>,
    /// Run once
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub run_once: bool,
    /// Tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Become
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub r#become: bool,
    /// Become user
    #[serde(default)]
    pub become_user: Option<String>,
    /// Block of tasks
    #[serde(default)]
    pub block: Option<Vec<TaskDefinition>>,
    /// Rescue tasks (run if block fails)
    #[serde(default)]
    pub rescue: Option<Vec<TaskDefinition>>,
    /// Always tasks (run regardless)
    #[serde(default)]
    pub always: Option<Vec<TaskDefinition>>,
    /// Include tasks file
    #[serde(default)]
    pub include_tasks: Option<String>,
    /// Import tasks file
    #[serde(default)]
    pub import_tasks: Option<String>,
    /// Include role
    #[serde(default)]
    pub include_role: Option<IncludeRoleDefinition>,
    /// Import role
    #[serde(default)]
    pub import_role: Option<IncludeRoleDefinition>,
    /// Task arguments (module-specific)
    #[serde(default, alias = "args")]
    pub module_args: Option<IndexMap<String, JsonValue>>,
    /// Environment variables
    #[serde(default)]
    pub environment: IndexMap<String, JsonValue>,
    /// Retries
    #[serde(default)]
    pub retries: Option<usize>,
    /// Delay between retries
    #[serde(default)]
    pub delay: Option<usize>,
    /// Until condition for retries
    #[serde(default)]
    pub until: Option<WhenCondition>,
    /// Module name and args (catch-all for module: args format)
    #[serde(flatten)]
    pub module: IndexMap<String, JsonValue>,
}

/// Include/Import role definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncludeRoleDefinition {
    pub name: String,
    #[serde(default)]
    pub tasks_from: Option<String>,
    #[serde(default)]
    pub vars_from: Option<String>,
    #[serde(default)]
    pub defaults_from: Option<String>,
    #[serde(default)]
    pub handlers_from: Option<String>,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub allow_duplicates: bool,
}

/// When condition can be a string, boolean, or list
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WhenCondition {
    Bool(bool),
    Single(String),
    List(Vec<String>),
}

impl WhenCondition {
    /// Convert to a single condition string (AND-joined if list)
    pub fn to_condition(&self) -> String {
        match self {
            WhenCondition::Bool(b) => b.to_string(),
            WhenCondition::Single(s) => s.clone(),
            WhenCondition::List(list) => {
                if list.len() == 1 {
                    list[0].clone()
                } else {
                    list.iter()
                        .map(|s| format!("({})", s))
                        .collect::<Vec<_>>()
                        .join(" and ")
                }
            }
        }
    }
}

/// Notify value can be a string or list
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(untagged)]
pub enum NotifyValue {
    #[default]
    None,
    Single(String),
    List(Vec<String>),
}

impl NotifyValue {
    pub fn to_vec(&self) -> Vec<String> {
        match self {
            NotifyValue::None => vec![],
            NotifyValue::Single(s) => vec![s.clone()],
            NotifyValue::List(list) => list.clone(),
        }
    }
}

/// Loop value can be various types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoopValue {
    Items(Vec<JsonValue>),
    Variable(String),
}

/// Loop control options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopControl {
    #[serde(default = "default_loop_var")]
    pub loop_var: String,
    #[serde(default)]
    pub index_var: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub pause: Option<f64>,
    #[serde(default)]
    pub extended: bool,
}

fn default_loop_var() -> String {
    "item".to_string()
}

/// Handler definition from YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerDefinition {
    /// Handler name (optional - handlers can be identified by listen topics)
    #[serde(default)]
    pub name: String,
    /// Listen for additional notification names
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub listen: Vec<String>,
    /// When condition
    #[serde(default)]
    pub when: Option<WhenCondition>,
    /// Module name and args
    #[serde(flatten)]
    pub module: IndexMap<String, JsonValue>,
}

/// A play within a playbook
#[derive(Debug, Clone)]
pub struct Play {
    /// Play name
    pub name: String,
    /// Target hosts pattern
    pub hosts: String,
    /// Whether to gather facts
    pub gather_facts: bool,
    /// Become root
    pub r#become: bool,
    /// User to become
    pub become_user: Option<String>,
    /// Connection type
    pub connection: Option<String>,
    /// Remote user
    pub remote_user: Option<String>,
    /// Play variables
    pub vars: IndexMap<String, JsonValue>,
    /// Variable files to include
    pub vars_files: Vec<String>,
    /// Roles to execute
    pub roles: Vec<Role>,
    /// Pre-tasks
    pub pre_tasks: Vec<Task>,
    /// Main tasks
    pub tasks: Vec<Task>,
    /// Post-tasks
    pub post_tasks: Vec<Task>,
    /// Handlers
    pub handlers: Vec<Handler>,
    /// Environment variables
    pub environment: IndexMap<String, JsonValue>,
    /// Tags
    pub tags: Vec<String>,
    /// Serial execution
    pub serial: Option<crate::playbook::SerialSpec>,
    /// Max failure percentage
    pub max_fail_percentage: Option<u8>,
    /// Strategy
    pub strategy: Option<String>,
    /// Ignore unreachable hosts
    pub ignore_unreachable: bool,
    /// Force handlers to run even if play fails
    pub force_handlers: bool,
}

impl Play {
    /// Create a new play
    pub fn new(name: impl Into<String>, hosts: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            hosts: hosts.into(),
            gather_facts: true,
            r#become: false,
            become_user: None,
            connection: None,
            remote_user: None,
            vars: IndexMap::new(),
            vars_files: Vec::new(),
            roles: Vec::new(),
            pre_tasks: Vec::new(),
            tasks: Vec::new(),
            post_tasks: Vec::new(),
            handlers: Vec::new(),
            environment: IndexMap::new(),
            tags: Vec::new(),
            serial: None,
            max_fail_percentage: None,
            strategy: None,
            ignore_unreachable: false,
            force_handlers: false,
        }
    }

    /// Create a Play from a PlayDefinition
    pub fn from_definition(
        def: PlayDefinition,
        playbook_path: Option<&PathBuf>,
    ) -> ExecutorResult<Self> {
        let mut play = Play::new(&def.name, &def.hosts);

        play.gather_facts = def.gather_facts;
        play.r#become = def.r#become;
        play.r#become_user = def.r#become_user;
        play.connection = def.connection;
        play.remote_user = def.remote_user;
        play.vars = def.vars;
        play.vars_files = def.vars_files;
        play.environment = def.environment;
        play.tags = def.tags;
        play.strategy = def.strategy;
        play.ignore_unreachable = def.ignore_unreachable;
        play.force_handlers = def.force_handlers;
        play.max_fail_percentage = def.max_fail_percentage;

        // Parse serial value into SerialSpec
        if let Some(serial) = def.serial {
            play.serial = Some(convert_serial_value_to_spec(serial));
        }

        // Parse roles
        for role_def in def.roles {
            let role = Role::from_definition(role_def, playbook_path)?;
            play.roles.push(role);
        }

        // Parse pre_tasks
        for task_def in def.pre_tasks {
            let tasks = parse_task_definition(task_def, playbook_path)?;
            play.pre_tasks.extend(tasks);
        }

        // Parse tasks
        for task_def in def.tasks {
            let tasks = parse_task_definition(task_def, playbook_path)?;
            play.tasks.extend(tasks);
        }

        // Parse post_tasks
        for task_def in def.post_tasks {
            let tasks = parse_task_definition(task_def, playbook_path)?;
            play.post_tasks.extend(tasks);
        }

        // Parse handlers
        for handler_def in def.handlers {
            let handler = parse_handler_definition(handler_def)?;
            play.handlers.push(handler);
        }

        Ok(play)
    }

    /// Add a task to the play
    pub fn add_task(&mut self, task: Task) {
        self.tasks.push(task);
    }

    /// Add a handler to the play
    pub fn add_handler(&mut self, handler: Handler) {
        self.handlers.push(handler);
    }

    /// Set a variable
    pub fn set_var(&mut self, name: impl Into<String>, value: JsonValue) {
        self.vars.insert(name.into(), value);
    }
}

/// A role to be executed
#[derive(Debug, Clone)]
pub struct Role {
    /// Role name
    pub name: String,
    /// Role path
    pub path: Option<PathBuf>,
    /// Role variables (passed when including role - highest role precedence)
    pub vars: IndexMap<String, JsonValue>,
    /// When condition
    pub when: Option<String>,
    /// Tags
    pub tags: Vec<String>,
    /// Become override
    pub r#become: Option<bool>,
    /// Default variables (from defaults/main.yml - lowest precedence)
    pub defaults: IndexMap<String, JsonValue>,
    /// Role vars (from vars/main.yml - higher precedence than defaults)
    pub role_vars: IndexMap<String, JsonValue>,
    /// Tasks from role
    pub tasks: Vec<Task>,
    /// Handlers from role
    pub handlers: Vec<Handler>,
    /// Files included by tasks_from
    pub tasks_from: Option<String>,
    /// Files included by vars_from
    pub vars_from: Option<String>,
    /// Files included by defaults_from
    pub defaults_from: Option<String>,
    /// Files included by handlers_from
    pub handlers_from: Option<String>,
    /// Dependencies
    pub dependencies: Vec<Role>,
}

impl Role {
    /// Create a new role
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            path: None,
            vars: IndexMap::new(),
            when: None,
            tags: Vec::new(),
            r#become: None,
            defaults: IndexMap::new(),
            role_vars: IndexMap::new(),
            tasks: Vec::new(),
            handlers: Vec::new(),
            tasks_from: None,
            vars_from: None,
            defaults_from: None,
            handlers_from: None,
            dependencies: Vec::new(),
        }
    }

    /// Create a Role from a RoleDefinition
    pub fn from_definition(
        def: RoleDefinition,
        playbook_path: Option<&PathBuf>,
    ) -> ExecutorResult<Self> {
        let mut role = Role::new(def.name());
        role.vars = def.vars();
        role.when = def.when().map(String::from);

        // Extract all options from Full variant
        if let RoleDefinition::Full {
            tags,
            r#become: become_opt,
            tasks_from,
            vars_from,
            defaults_from,
            handlers_from,
            ..
        } = &def
        {
            role.tags = tags.clone();
            role.r#become = *become_opt;
            role.tasks_from = tasks_from.clone();
            role.vars_from = vars_from.clone();
            role.defaults_from = defaults_from.clone();
            role.handlers_from = handlers_from.clone();
        }

        // Load the role from disk
        // Looking in: ./roles/<name>, ~/.ansible/roles/<name>, /etc/ansible/roles/<name>

        if let Some(playbook_path) = playbook_path {
            let playbook_dir = playbook_path.parent().unwrap_or(Path::new("."));
            let role_path = playbook_dir.join("roles").join(role.name.clone());

            if role_path.exists() {
                role.path = Some(role_path.clone());

                // Load defaults/main.yml (or defaults_from if specified)
                let defaults_file = if let Some(ref defaults_from) = role.defaults_from {
                    role_path
                        .join("defaults")
                        .join(format!("{}.yml", defaults_from))
                } else {
                    role_path.join("defaults").join("main.yml")
                };
                if defaults_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&defaults_file) {
                        if let Ok(defaults) =
                            serde_yaml::from_str::<IndexMap<String, JsonValue>>(&content)
                        {
                            role.defaults = defaults;
                        }
                    }
                }

                // Load vars/main.yml (or vars_from if specified) - higher precedence than defaults
                let vars_file = if let Some(ref vars_from) = role.vars_from {
                    role_path.join("vars").join(format!("{}.yml", vars_from))
                } else {
                    role_path.join("vars").join("main.yml")
                };
                if vars_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&vars_file) {
                        if let Ok(role_vars) =
                            serde_yaml::from_str::<IndexMap<String, JsonValue>>(&content)
                        {
                            role.role_vars = role_vars;
                        }
                    }
                }

                // Load tasks/main.yml (or tasks_from if specified)
                let tasks_file = if let Some(ref tasks_from) = role.tasks_from {
                    role_path.join("tasks").join(format!("{}.yml", tasks_from))
                } else {
                    role_path.join("tasks").join("main.yml")
                };

                if tasks_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&tasks_file) {
                        if let Ok(task_defs) = serde_yaml::from_str::<Vec<TaskDefinition>>(&content)
                        {
                            for task_def in task_defs {
                                if let Ok(tasks) =
                                    parse_task_definition(task_def, Some(&tasks_file))
                                {
                                    role.tasks.extend(tasks);
                                }
                            }
                        }
                    }
                }

                // Load handlers/main.yml (or handlers_from if specified)
                let handlers_file = if let Some(ref handlers_from) = role.handlers_from {
                    role_path
                        .join("handlers")
                        .join(format!("{}.yml", handlers_from))
                } else {
                    role_path.join("handlers").join("main.yml")
                };
                if handlers_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&handlers_file) {
                        if let Ok(handler_defs) =
                            serde_yaml::from_str::<Vec<HandlerDefinition>>(&content)
                        {
                            for handler_def in handler_defs {
                                if let Ok(handler) = parse_handler_definition(handler_def) {
                                    role.handlers.push(handler);
                                }
                            }
                        }
                    }
                }

                // Load meta/main.yml for dependencies
                let meta_file = role_path.join("meta").join("main.yml");
                if meta_file.exists() {
                    if let Ok(content) = std::fs::read_to_string(&meta_file) {
                        if let Ok(meta) = serde_yaml::from_str::<RoleMeta>(&content) {
                            for dep in meta.dependencies {
                                if let Ok(dep_role) =
                                    Role::from_definition(dep, Some(playbook_path))
                                {
                                    role.dependencies.push(dep_role);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(role)
    }

    /// Get all tasks including from dependencies
    pub fn get_all_tasks(&self) -> Vec<Task> {
        let mut all_tasks = Vec::new();

        // First, add dependency tasks
        for dep in &self.dependencies {
            all_tasks.extend(dep.get_all_tasks());
        }

        // Then add our tasks
        all_tasks.extend(self.tasks.clone());

        all_tasks
    }

    /// Get all handlers including from dependencies
    pub fn get_all_handlers(&self) -> Vec<Handler> {
        let mut all_handlers = Vec::new();

        for dep in &self.dependencies {
            all_handlers.extend(dep.get_all_handlers());
        }

        all_handlers.extend(self.handlers.clone());

        all_handlers
    }

    /// Get all variables with proper precedence (lowest to highest):
    /// 1. defaults (from defaults/main.yml)
    /// 2. role_vars (from vars/main.yml)
    /// 3. vars (passed when including role)
    ///
    /// Returns a merged map where higher precedence values override lower ones.
    pub fn get_all_vars(&self) -> IndexMap<String, JsonValue> {
        let mut all_vars = IndexMap::new();

        // Start with defaults (lowest precedence)
        for (key, value) in &self.defaults {
            all_vars.insert(key.clone(), value.clone());
        }

        // Override with role vars (from vars/main.yml)
        for (key, value) in &self.role_vars {
            all_vars.insert(key.clone(), value.clone());
        }

        // Override with vars passed at include time (highest role precedence)
        for (key, value) in &self.vars {
            all_vars.insert(key.clone(), value.clone());
        }

        all_vars
    }

    /// Get all defaults from this role and its dependencies
    pub fn get_all_defaults(&self) -> IndexMap<String, JsonValue> {
        let mut all_defaults = IndexMap::new();

        // Get dependency defaults first
        for dep in &self.dependencies {
            for (key, value) in dep.get_all_defaults() {
                all_defaults.insert(key, value);
            }
        }

        // Our defaults override dependency defaults
        for (key, value) in &self.defaults {
            all_defaults.insert(key.clone(), value.clone());
        }

        all_defaults
    }

    /// Get all role_vars from this role and its dependencies
    pub fn get_all_role_vars(&self) -> IndexMap<String, JsonValue> {
        let mut all_role_vars = IndexMap::new();

        // Get dependency role_vars first
        for dep in &self.dependencies {
            for (key, value) in dep.get_all_role_vars() {
                all_role_vars.insert(key, value);
            }
        }

        // Our role_vars override dependency role_vars
        for (key, value) in &self.role_vars {
            all_role_vars.insert(key.clone(), value.clone());
        }

        all_role_vars
    }
}

/// Role metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoleMeta {
    #[serde(default)]
    dependencies: Vec<RoleDefinition>,
    #[serde(default)]
    galaxy_info: Option<GalaxyInfo>,
    #[serde(default)]
    allow_duplicates: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GalaxyInfo {
    role_name: Option<String>,
    author: Option<String>,
    description: Option<String>,
    license: Option<String>,
    min_ansible_version: Option<String>,
    platforms: Option<Vec<Platform>>,
    galaxy_tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Platform {
    name: String,
    versions: Option<Vec<String>>,
}

/// Parse a task definition into Task(s)
fn parse_task_definition(
    def: TaskDefinition,
    playbook_path: Option<&PathBuf>,
) -> ExecutorResult<Vec<Task>> {
    let mut tasks = Vec::new();

    // Handle block/rescue/always
    if let Some(block_tasks) = def.block {
        use crate::executor::task::BlockRole;
        use uuid::Uuid;

        // Generate a unique block ID
        let block_id = Uuid::new_v4().to_string();

        let mut block_parsed = Vec::new();
        for task_def in block_tasks {
            block_parsed.extend(parse_task_definition(task_def, playbook_path)?);
        }

        // Apply block-level properties to all tasks and mark as block tasks
        for mut task in block_parsed {
            task.block_id = Some(block_id.clone());
            task.block_role = BlockRole::Normal;
            if def.r#become {
                task.r#become = true;
            }
            if let Some(ref when) = def.when {
                if task.when.is_none() {
                    task.when = Some(when.to_condition());
                }
            }
            tasks.push(task);
        }

        // Handle rescue tasks
        if let Some(rescue_tasks) = def.rescue {
            for task_def in rescue_tasks {
                let mut rescue_parsed = parse_task_definition(task_def, playbook_path)?;
                // Mark these as rescue tasks
                for task in &mut rescue_parsed {
                    task.block_id = Some(block_id.clone());
                    task.block_role = BlockRole::Rescue;
                }
                tasks.extend(rescue_parsed);
            }
        }

        // Handle always tasks
        if let Some(always_tasks) = def.always {
            for task_def in always_tasks {
                let mut always_parsed = parse_task_definition(task_def, playbook_path)?;
                // Mark these as always tasks
                for task in &mut always_parsed {
                    task.block_id = Some(block_id.clone());
                    task.block_role = BlockRole::Always;
                }
                tasks.extend(always_parsed);
            }
        }

        return Ok(tasks);
    }

    // Handle include_tasks
    if let Some(ref include_file) = def.include_tasks {
        debug!("Would include tasks from: {}", include_file);
        // In a full implementation, load and parse the included file
        let task = Task {
            name: def.name.clone(),
            module: "include_tasks".to_string(),
            args: {
                let mut args = IndexMap::new();
                args.insert("file".to_string(), JsonValue::String(include_file.clone()));
                args
            },
            when: def.when.as_ref().map(|w| w.to_condition()),
            ..Default::default()
        };
        return Ok(vec![task]);
    }

    // Handle import_tasks
    if let Some(ref import_file) = def.import_tasks {
        debug!("Would import tasks from: {}", import_file);
        // In a full implementation, load and parse the imported file at parse time
        let task = Task {
            name: def.name.clone(),
            module: "import_tasks".to_string(),
            args: {
                let mut args = IndexMap::new();
                args.insert("file".to_string(), JsonValue::String(import_file.clone()));
                args
            },
            when: def.when.as_ref().map(|w| w.to_condition()),
            ..Default::default()
        };
        return Ok(vec![task]);
    }

    // Handle include_role
    if let Some(ref include_role) = def.include_role {
        let task = Task {
            name: if def.name.is_empty() {
                format!("Include role: {}", include_role.name)
            } else {
                def.name.clone()
            },
            module: "include_role".to_string(),
            args: {
                let mut args = IndexMap::new();
                args.insert(
                    "name".to_string(),
                    JsonValue::String(include_role.name.clone()),
                );
                if let Some(ref tasks_from) = include_role.tasks_from {
                    args.insert(
                        "tasks_from".to_string(),
                        JsonValue::String(tasks_from.clone()),
                    );
                }
                args
            },
            when: def.when.as_ref().map(|w| w.to_condition()),
            ..Default::default()
        };
        return Ok(vec![task]);
    }

    // Handle import_role
    if let Some(ref import_role) = def.import_role {
        let task = Task {
            name: if def.name.is_empty() {
                format!("Import role: {}", import_role.name)
            } else {
                def.name.clone()
            },
            module: "import_role".to_string(),
            args: {
                let mut args = IndexMap::new();
                args.insert(
                    "name".to_string(),
                    JsonValue::String(import_role.name.clone()),
                );
                args
            },
            when: def.when.as_ref().map(|w| w.to_condition()),
            ..Default::default()
        };
        return Ok(vec![task]);
    }

    // Find the module in the flattened definition
    let (module_name, module_args) = find_module_in_definition(&def)?;

    // Build the task
    let task = Task {
        name: def.name,
        module: module_name,
        args: module_args,
        when: def.when.as_ref().map(|w| w.to_condition()),
        notify: def.notify.to_vec(),
        register: def.register,
        loop_items: match def.loop_items {
            Some(LoopValue::Items(items)) => Some(crate::executor::task::LoopSource::Items(items)),
            Some(LoopValue::Variable(template)) => {
                Some(crate::executor::task::LoopSource::Template(template))
            }
            None => None,
        },
        loop_var: def
            .loop_control
            .as_ref()
            .map(|lc| lc.loop_var.clone())
            .unwrap_or_else(|| "item".to_string()),
        loop_control: def
            .loop_control
            .map(|lc| crate::executor::task::LoopControl {
                loop_var: lc.loop_var,
                index_var: lc.index_var,
                label: lc.label,
                pause: lc.pause.map(|p| p as u64),
                extended: lc.extended,
            }),
        ignore_errors: def.ignore_errors,
        changed_when: def.changed_when.as_ref().map(|w| w.to_condition()),
        failed_when: def.failed_when.as_ref().map(|w| w.to_condition()),
        delegate_to: def.delegate_to,
        delegate_facts: None, // Not in old TaskDefinition, would need to add to parser
        run_once: def.run_once,
        tags: def.tags,
        r#become: def.r#become,
        become_user: def.become_user,
        block_id: None,
        block_role: crate::executor::task::BlockRole::Normal,
        retries: None,
        delay: None,
        until: None,
    };

    tasks.push(task);
    Ok(tasks)
}

/// Find the module name and args in a task definition
fn find_module_in_definition(
    def: &TaskDefinition,
) -> ExecutorResult<(String, IndexMap<String, JsonValue>)> {
    // Known non-module keys
    let non_module_keys = [
        "name",
        "when",
        "register",
        "notify",
        "loop",
        "loop_items",
        "with_items",
        "with_list",
        "loop_control",
        "ignore_errors",
        "changed_when",
        "failed_when",
        "delegate_to",
        "run_once",
        "tags",
        "become",
        "become_user",
        "block",
        "rescue",
        "always",
        "include_tasks",
        "import_tasks",
        "include_role",
        "import_role",
        "environment",
        "retries",
        "delay",
        "until",
        "vars",
        "module_args",
        "args",
        "no_log",
        "throttle",
        "any_errors_fatal",
        "check_mode",
        "diff",
        "connection",
        "async",
        "poll",
    ];

    // Check explicit args first
    if let Some(ref args) = def.module_args {
        // Find the module in the flattened fields
        for (key, value) in &def.module {
            if !non_module_keys.contains(&key.as_str()) {
                // This is the module
                let mut full_args = args.clone();

                // If value is a map, merge it
                if let JsonValue::Object(obj) = value {
                    for (k, v) in obj {
                        full_args.insert(k.clone(), v.clone());
                    }
                } else if let JsonValue::String(s) = value {
                    // Free-form module args
                    full_args.insert("_raw_params".to_string(), JsonValue::String(s.clone()));
                }

                return Ok((key.clone(), full_args));
            }
        }
    }

    // Look for module in flattened fields
    for (key, value) in &def.module {
        if !non_module_keys.contains(&key.as_str()) {
            let args = match value {
                JsonValue::Object(obj) => obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
                JsonValue::String(s) => {
                    let mut args = IndexMap::new();
                    args.insert("_raw_params".to_string(), JsonValue::String(s.clone()));
                    args
                }
                JsonValue::Null => IndexMap::new(),
                other => {
                    let mut args = IndexMap::new();
                    args.insert("_raw_params".to_string(), other.clone());
                    args
                }
            };

            return Ok((key.clone(), args));
        }
    }

    // Default to debug module if nothing found
    Ok(("debug".to_string(), IndexMap::new()))
}

/// Convert SerialValue to SerialSpec
fn convert_serial_value_to_spec(value: SerialValue) -> crate::playbook::SerialSpec {
    use crate::playbook::SerialSpec;

    match value {
        SerialValue::Number(n) => SerialSpec::Fixed(n),
        SerialValue::Percentage(p) => SerialSpec::Percentage(p),
        SerialValue::List(list) => {
            let specs: Vec<SerialSpec> =
                list.into_iter().map(convert_serial_value_to_spec).collect();
            SerialSpec::Progressive(specs)
        }
    }
}

/// Parse a handler definition
fn parse_handler_definition(def: HandlerDefinition) -> ExecutorResult<Handler> {
    let (module_name, module_args) = {
        let non_module_keys = ["name", "listen", "when"];

        let mut module_name = "debug".to_string();
        let mut module_args = IndexMap::new();

        for (key, value) in &def.module {
            if !non_module_keys.contains(&key.as_str()) {
                module_name = key.clone();
                module_args = match value {
                    JsonValue::Object(obj) => {
                        obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                    }
                    JsonValue::String(s) => {
                        let mut args = IndexMap::new();
                        args.insert("_raw_params".to_string(), JsonValue::String(s.clone()));
                        args
                    }
                    _ => IndexMap::new(),
                };
                break;
            }
        }

        (module_name, module_args)
    };

    Ok(Handler {
        name: def.name,
        module: module_name,
        args: module_args,
        when: def.when.map(|w| w.to_condition()),
        listen: def.listen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playbook_new() {
        let playbook = Playbook::new("Test Playbook");
        assert_eq!(playbook.name, "Test Playbook");
        assert!(playbook.plays.is_empty());
    }

    #[test]
    fn test_play_new() {
        let play = Play::new("Install nginx", "webservers");
        assert_eq!(play.name, "Install nginx");
        assert_eq!(play.hosts, "webservers");
        assert!(play.gather_facts);
    }

    #[test]
    fn test_role_new() {
        let role = Role::new("nginx");
        assert_eq!(role.name, "nginx");
        assert!(role.tasks.is_empty());
    }

    #[test]
    fn test_parse_simple_playbook() {
        let yaml = r#"
- name: Test Play
  hosts: all
  gather_facts: false
  tasks:
    - name: Debug message
      debug:
        msg: "Hello World"
"#;

        let playbook = Playbook::parse(yaml, None).unwrap();
        assert_eq!(playbook.plays.len(), 1);

        let play = &playbook.plays[0];
        assert_eq!(play.name, "Test Play");
        assert_eq!(play.hosts, "all");
        assert!(!play.gather_facts);
        assert_eq!(play.tasks.len(), 1);

        let task = &play.tasks[0];
        assert_eq!(task.name, "Debug message");
        assert_eq!(task.module, "debug");
    }

    #[test]
    fn test_parse_when_condition() {
        let single = WhenCondition::Single("ansible_os_family == 'Debian'".to_string());
        assert_eq!(single.to_condition(), "ansible_os_family == 'Debian'");

        let list = WhenCondition::List(vec![
            "ansible_os_family == 'Debian'".to_string(),
            "ansible_distribution_major_version >= '20'".to_string(),
        ]);
        assert!(list.to_condition().contains(" and "));
    }

    #[test]
    fn test_parse_notify() {
        let none = NotifyValue::None;
        assert!(none.to_vec().is_empty());

        let single = NotifyValue::Single("restart nginx".to_string());
        assert_eq!(single.to_vec(), vec!["restart nginx"]);

        let list = NotifyValue::List(vec![
            "restart nginx".to_string(),
            "reload config".to_string(),
        ]);
        assert_eq!(list.to_vec().len(), 2);
    }

    #[test]
    fn test_role_definition_name() {
        let simple = RoleDefinition::Name("nginx".to_string());
        assert_eq!(simple.name(), "nginx");

        let full = RoleDefinition::Full {
            role: "nginx".to_string(),
            when: Some("ansible_os_family == 'Debian'".to_string()),
            tags: vec!["web".to_string()],
            vars: IndexMap::new(),
            r#become: Some(true),
            tasks_from: None,
            vars_from: None,
            defaults_from: None,
            handlers_from: None,
        };
        assert_eq!(full.name(), "nginx");
    }

    #[test]
    fn test_parse_playbook_with_roles() {
        let yaml = r#"
- name: Web Server Setup
  hosts: webservers
  roles:
    - nginx
    - role: php
      when: install_php
      vars:
        php_version: "8.1"
"#;

        let playbook = Playbook::parse(yaml, None).unwrap();
        assert_eq!(playbook.plays.len(), 1);

        let play = &playbook.plays[0];
        assert_eq!(play.roles.len(), 2);
        assert_eq!(play.roles[0].name, "nginx");
        assert_eq!(play.roles[1].name, "php");
    }

    #[test]
    fn test_parse_playbook_with_handlers() {
        let yaml = r#"
- name: Configure nginx
  hosts: webservers
  tasks:
    - name: Copy config
      copy:
        src: nginx.conf
        dest: /etc/nginx/nginx.conf
      notify: restart nginx
  handlers:
    - name: restart nginx
      service:
        name: nginx
        state: restarted
"#;

        let playbook = Playbook::parse(yaml, None).unwrap();
        let play = &playbook.plays[0];

        assert_eq!(play.handlers.len(), 1);
        assert_eq!(play.handlers[0].name, "restart nginx");
        assert_eq!(play.handlers[0].module, "service");
    }
}
