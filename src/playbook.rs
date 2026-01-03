//! Playbook definitions and parsing.
//!
//! This module provides types for representing Ansible-compatible playbooks
//! with type-safe definitions and validation.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::vars::Variables;

/// Helper function for serde to check if Variables is empty
fn is_vars_empty(vars: &Variables) -> bool {
    vars.is_empty()
}

/// Deserialize a field that can be either a string or a vector of strings
#[allow(dead_code)]
fn string_or_vec<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct StringOrVec;

    impl<'de> Visitor<'de> for StringOrVec {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or list of strings")
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(vec![value.to_string()])
        }

        fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(value) = seq.next_element()? {
                vec.push(value);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(StringOrVec)
}

/// Deserialize boolean that accepts various formats (true, True, yes, 1, etc.)
fn deserialize_bool_flexible<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct BoolVisitor;

    impl<'de> Visitor<'de> for BoolVisitor {
        type Value = bool;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a boolean value (true, false, yes, no, True, False, 1, 0)")
        }

        fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value.to_lowercase().as_str() {
                "true" | "yes" | "y" | "1" | "on" => Ok(true),
                "false" | "no" | "n" | "0" | "off" => Ok(false),
                _ => Err(de::Error::custom(format!("invalid boolean: {}", value))),
            }
        }

        fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value != 0)
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value != 0)
        }
    }

    deserializer.deserialize_any(BoolVisitor)
}

/// A playbook containing one or more plays.
///
/// Playbooks are the top-level configuration files in Rustible.
/// They contain a list of plays that define the automation workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    /// Name of the playbook (optional, derived from filename if not set)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The plays in this playbook
    #[serde(flatten)]
    pub plays: Vec<Play>,

    /// Path to the playbook file (set during loading)
    #[serde(skip)]
    pub source_path: Option<std::path::PathBuf>,
}

impl Playbook {
    /// Loads a playbook from a YAML file.
    pub async fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            Error::playbook_parse(path, format!("Failed to read file: {}", e), None)
        })?;

        Self::from_yaml(&content, Some(path.to_path_buf()))
    }

    /// Parses a playbook from a YAML string.
    pub fn from_yaml(yaml: &str, source_path: Option<std::path::PathBuf>) -> Result<Self> {
        // Playbooks are a list of plays at the top level
        let plays: Vec<Play> = serde_yaml::from_str(yaml).map_err(|e| {
            Error::playbook_parse(
                source_path
                    .as_ref()
                    .map_or("<string>".into(), |p| p.clone()),
                e.to_string(),
                None,
            )
        })?;

        let name = source_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string());

        Ok(Self {
            name,
            plays,
            source_path,
        })
    }

    /// Validates the playbook structure.
    pub fn validate(&self) -> Result<()> {
        if self.plays.is_empty() {
            return Err(Error::PlaybookValidation(
                "Playbook must contain at least one play".to_string(),
            ));
        }

        for (idx, play) in self.plays.iter().enumerate() {
            play.validate().map_err(|e| {
                Error::PlaybookValidation(format!("Play {} validation failed: {}", idx + 1, e))
            })?;
        }

        Ok(())
    }

    /// Returns the number of plays.
    pub fn play_count(&self) -> usize {
        self.plays.len()
    }

    /// Returns total number of tasks across all plays.
    pub fn task_count(&self) -> usize {
        self.plays.iter().map(|p| p.tasks.len()).sum()
    }
}

/// A play within a playbook.
///
/// A play maps a selection of hosts to tasks to be executed on those hosts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Play {
    /// Name of the play
    #[serde(default)]
    pub name: String,

    /// Host pattern to match against inventory
    pub hosts: String,

    /// Whether to gather facts before executing tasks
    #[serde(
        default = "default_gather_facts",
        deserialize_with = "deserialize_bool_flexible_default_true"
    )]
    pub gather_facts: bool,

    /// Subset of gathered facts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gather_subset: Option<Vec<String>>,

    /// Timeout for fact gathering in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gather_timeout: Option<u64>,

    /// Variables for this play
    #[serde(default, skip_serializing_if = "is_vars_empty")]
    pub vars: Variables,

    /// Variable files to load
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vars_files: Vec<String>,

    /// Roles to apply
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleRef>,

    /// Pre-tasks to run before roles
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_tasks: Vec<Task>,

    /// Tasks to run after roles
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<Task>,

    /// Post-tasks to run after tasks
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post_tasks: Vec<Task>,

    /// Handlers that can be notified
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handlers: Vec<Handler>,

    /// Become configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#become: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub become_user: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub become_method: Option<String>,

    /// Connection settings
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,

    /// Remote user
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_user: Option<String>,

    /// Port to connect on
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Execution strategy
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,

    /// Serial execution (batch size)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<SerialSpec>,

    /// Maximum failure percentage before aborting
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_fail_percentage: Option<u8>,

    /// Whether to run handlers on failure
    #[serde(default)]
    pub force_handlers: bool,

    /// Whether to ignore unreachable hosts
    #[serde(default)]
    pub ignore_unreachable: bool,

    /// Module defaults
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub module_defaults: HashMap<String, serde_json::Value>,

    /// Environment variables
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub environment: HashMap<String, String>,

    /// Tags for filtering
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

fn default_gather_facts() -> bool {
    true
}

/// Deserialize boolean with default true
fn deserialize_bool_flexible_default_true<'de, D>(
    deserializer: D,
) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bool_flexible(deserializer).or(Ok(true))
}

/// Deserialize optional boolean that accepts various formats
#[allow(dead_code)]
fn deserialize_option_bool_flexible<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct OptionBoolVisitor;

    impl<'de> Visitor<'de> for OptionBoolVisitor {
        type Value = Option<bool>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an optional boolean value")
        }

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_bool_flexible(deserializer).map(Some)
        }

        fn visit_bool<E>(self, value: bool) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(value))
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            match value.to_lowercase().as_str() {
                "true" | "yes" | "y" | "1" | "on" => Ok(Some(true)),
                "false" | "no" | "n" | "0" | "off" => Ok(Some(false)),
                _ => Err(de::Error::custom(format!("invalid boolean: {}", value))),
            }
        }

        fn visit_unit<E>(self) -> std::result::Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_option(OptionBoolVisitor)
}

impl Play {
    /// Creates a new play with the given name and host pattern.
    pub fn new(name: impl Into<String>, hosts: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            hosts: hosts.into(),
            gather_facts: true,
            gather_subset: None,
            gather_timeout: None,
            vars: Variables::new(),
            vars_files: Vec::new(),
            roles: Vec::new(),
            pre_tasks: Vec::new(),
            tasks: Vec::new(),
            post_tasks: Vec::new(),
            handlers: Vec::new(),
            r#become: None,
            become_user: None,
            become_method: None,
            connection: None,
            remote_user: None,
            port: None,
            strategy: None,
            serial: None,
            max_fail_percentage: None,
            force_handlers: false,
            ignore_unreachable: false,
            module_defaults: HashMap::new(),
            environment: HashMap::new(),
            tags: Vec::new(),
        }
    }

    /// Validates the play structure.
    pub fn validate(&self) -> Result<()> {
        if self.hosts.is_empty() {
            return Err(Error::PlaybookValidation(
                "Play must specify hosts".to_string(),
            ));
        }

        // Validate tasks
        for task in self.all_tasks() {
            task.validate()?;
        }

        // Validate handlers
        // Ansible allows handler tasks without an explicit `name` as long as they can be
        // referenced via `listen`. We only error if *both* are missing.
        for handler in &self.handlers {
            if handler.name.is_empty() && handler.listen.is_empty() {
                return Err(Error::PlaybookValidation(
                    "Handler must have a name or listen".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Returns an iterator over all tasks (pre_tasks, tasks, post_tasks).
    pub fn all_tasks(&self) -> impl Iterator<Item = &Task> {
        self.pre_tasks
            .iter()
            .chain(self.tasks.iter())
            .chain(self.post_tasks.iter())
    }

    /// Returns the total number of tasks.
    pub fn task_count(&self) -> usize {
        self.pre_tasks.len() + self.tasks.len() + self.post_tasks.len()
    }
}

/// Reference to a role with optional parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RoleRef {
    /// Simple role name
    Simple(String),

    /// Role with configuration
    Full {
        /// Role name
        role: String,

        /// Role variables
        #[serde(default, flatten)]
        vars: HashMap<String, serde_json::Value>,

        /// When condition
        #[serde(skip_serializing_if = "Option::is_none")]
        when: Option<String>,

        /// Tags
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tags: Vec<String>,
    },
}

impl RoleRef {
    /// Returns the role name.
    pub fn name(&self) -> &str {
        match self {
            Self::Simple(name) => name,
            Self::Full { role, .. } => role,
        }
    }
}

/// A task to execute.
#[derive(Debug, Clone, Serialize)]
pub struct Task {
    /// Name of the task
    pub name: String,

    /// Module to execute (the key is the module name)
    pub module: TaskModule,

    /// Conditional execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,

    /// Loop over items
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_: Option<serde_json::Value>,

    /// Alternative loop syntax
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_items: Option<serde_json::Value>,

    /// Dictionary iteration (with_dict)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_dict: Option<serde_json::Value>,

    /// File glob iteration (with_fileglob)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_fileglob: Option<serde_json::Value>,

    /// Register result in variable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub register: Option<String>,

    /// Variable to store results for loop
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_control: Option<LoopControl>,

    /// Handlers to notify on change
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notify: Vec<String>,

    /// Whether to ignore errors
    pub ignore_errors: bool,

    /// Whether to ignore unreachable
    pub ignore_unreachable: bool,

    /// Become settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#become: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub become_user: Option<String>,

    /// Delegation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate_to: Option<String>,

    /// Whether facts should be set on the delegated host instead of the original host
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegate_facts: Option<bool>,

    /// Run once
    pub run_once: bool,

    /// Changed when condition
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_when: Option<String>,

    /// Failed when condition
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_when: Option<String>,

    /// Tags
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// Task-level variables
    #[serde(skip_serializing_if = "is_vars_empty")]
    pub vars: Variables,

    /// Environment variables
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub environment: HashMap<String, String>,

    /// Async execution timeout
    #[serde(skip_serializing_if = "Option::is_none")]
    pub async_: Option<u64>,

    /// Poll interval for async
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll: Option<u64>,

    /// Number of retries
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,

    /// Delay between retries in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay: Option<u64>,

    /// Condition for retry success
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,

    /// Block of tasks (for block/rescue/always error handling)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block: Option<Vec<Task>>,

    /// Rescue tasks (run on block failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rescue: Option<Vec<Task>>,

    /// Always tasks (always run after block, regardless of success/failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub always: Option<Vec<Task>>,
}

impl<'de> Deserialize<'de> for Task {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        // Deserialize as a generic Value first
        let value = serde_json::Value::deserialize(deserializer)?;

        let obj = value
            .as_object()
            .ok_or_else(|| D::Error::custom("task must be an object"))?;

        // List of known non-module keys
        let skip_keys: std::collections::HashSet<&str> = [
            "name",
            "when",
            "loop",
            "loop_",
            "with_items",
            "with_dict",
            "with_fileglob",
            "register",
            "loop_control",
            "notify",
            "ignore_errors",
            "ignore_unreachable",
            "become",
            "become_user",
            "delegate_to",
            "delegate_facts",
            "run_once",
            "changed_when",
            "failed_when",
            "tags",
            "vars",
            "environment",
            "async",
            "async_",
            "poll",
            "retries",
            "delay",
            "until",
            "block",
            "rescue",
            "always",
            "args",
            "include_tasks",
            "import_tasks",
            "include_vars",
            "include_role",
            "import_role",
        ]
        .iter()
        .copied()
        .collect();

        // Find the module name (first key that's not in skip list)
        let module_name = obj
            .keys()
            .find(|k| !skip_keys.contains(k.as_str()))
            .cloned()
            .unwrap_or_else(|| "debug".to_string());

        // Get module arguments
        let module_args = obj
            .get(&module_name)
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        // Helper to parse bool from various formats
        let parse_bool = |v: &serde_json::Value| -> bool {
            match v {
                serde_json::Value::Bool(b) => *b,
                serde_json::Value::String(s) => {
                    matches!(s.to_lowercase().as_str(), "true" | "yes" | "y" | "1" | "on")
                }
                serde_json::Value::Number(n) => n.as_i64().map(|i| i != 0).unwrap_or(false),
                _ => false,
            }
        };

        // Helper to parse optional bool
        let parse_option_bool =
            |v: Option<&serde_json::Value>| -> Option<bool> { v.map(parse_bool) };

        // Parse notify as string or vec
        let notify = match obj.get("notify") {
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };

        // Parse tags
        let tags = match obj.get("tags") {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            _ => Vec::new(),
        };

        // Parse when condition
        let when = match obj.get("when") {
            Some(serde_json::Value::String(s)) => Some(When::Single(s.clone())),
            Some(serde_json::Value::Array(arr)) => {
                let conditions: Vec<String> = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                if conditions.is_empty() {
                    None
                } else {
                    Some(When::Multiple(conditions))
                }
            }
            Some(serde_json::Value::Bool(b)) => Some(When::Single(b.to_string())),
            _ => None,
        };

        // Parse loop (check both "loop" and "loop_")
        let loop_ = obj.get("loop").or(obj.get("loop_")).cloned();

        // Parse loop_control
        let loop_control = obj
            .get("loop_control")
            .and_then(|v| serde_json::from_value::<LoopControl>(v.clone()).ok());

        // Parse vars
        let vars = obj
            .get("vars")
            .and_then(|v| serde_json::from_value::<Variables>(v.clone()).ok())
            .unwrap_or_default();

        // Parse environment
        let environment = obj
            .get("environment")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        Ok(Task {
            name: obj
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default(),
            module: TaskModule {
                name: module_name,
                args: module_args,
            },
            when,
            loop_,
            with_items: obj.get("with_items").cloned(),
            with_dict: obj.get("with_dict").cloned(),
            with_fileglob: obj.get("with_fileglob").cloned(),
            register: obj
                .get("register")
                .and_then(|v| v.as_str())
                .map(String::from),
            loop_control,
            notify,
            ignore_errors: obj.get("ignore_errors").map(parse_bool).unwrap_or(false),
            ignore_unreachable: obj
                .get("ignore_unreachable")
                .map(parse_bool)
                .unwrap_or(false),
            r#become: parse_option_bool(obj.get("become")),
            become_user: obj
                .get("become_user")
                .and_then(|v| v.as_str())
                .map(String::from),
            delegate_to: obj
                .get("delegate_to")
                .and_then(|v| v.as_str())
                .map(String::from),
            delegate_facts: parse_option_bool(obj.get("delegate_facts")),
            run_once: obj.get("run_once").map(parse_bool).unwrap_or(false),
            changed_when: obj.get("changed_when").and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Bool(b) => Some(b.to_string()),
                _ => None,
            }),
            failed_when: obj
                .get("failed_when")
                .and_then(|v| v.as_str())
                .map(String::from),
            tags,
            vars,
            environment,
            async_: obj
                .get("async")
                .or(obj.get("async_"))
                .and_then(|v| v.as_u64()),
            poll: obj.get("poll").and_then(|v| v.as_u64()),
            retries: obj
                .get("retries")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            delay: obj.get("delay").and_then(|v| v.as_u64()),
            until: obj.get("until").and_then(|v| v.as_str()).map(String::from),
            block: obj
                .get("block")
                .and_then(|v| serde_json::from_value::<Vec<Task>>(v.clone()).ok()),
            rescue: obj
                .get("rescue")
                .and_then(|v| serde_json::from_value::<Vec<Task>>(v.clone()).ok()),
            always: obj
                .get("always")
                .and_then(|v| serde_json::from_value::<Vec<Task>>(v.clone()).ok()),
        })
    }
}

impl Task {
    /// Creates a new task.
    pub fn new(
        name: impl Into<String>,
        module: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self {
            name: name.into(),
            module: TaskModule {
                name: module.into(),
                args,
            },
            when: None,
            loop_: None,
            with_items: None,
            with_dict: None,
            with_fileglob: None,
            register: None,
            loop_control: None,
            notify: Vec::new(),
            ignore_errors: false,
            ignore_unreachable: false,
            r#become: None,
            become_user: None,
            delegate_to: None,
            delegate_facts: None,
            run_once: false,
            changed_when: None,
            failed_when: None,
            tags: Vec::new(),
            vars: Variables::new(),
            environment: HashMap::new(),
            async_: None,
            poll: None,
            retries: None,
            delay: None,
            until: None,
            block: None,
            rescue: None,
            always: None,
        }
    }

    /// Validates the task.
    pub fn validate(&self) -> Result<()> {
        if self.module.name.is_empty() {
            return Err(Error::PlaybookValidation(
                "Task must specify a module".to_string(),
            ));
        }
        Ok(())
    }

    /// Returns the module name.
    pub fn module_name(&self) -> &str {
        &self.module.name
    }

    /// Returns the module arguments.
    pub fn module_args(&self) -> &serde_json::Value {
        &self.module.args
    }
}

/// Module invocation in a task.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TaskModule {
    /// Module name - extracted from the args map during deserialization
    #[serde(skip)]
    pub name: String,

    /// Module arguments
    pub args: serde_json::Value,
}

// TaskModule is deserialized as part of Task's custom deserializer

/// Conditional expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum When {
    /// Single condition
    Single(String),
    /// Multiple conditions (AND)
    Multiple(Vec<String>),
}

impl When {
    /// Returns the conditions as a slice.
    pub fn conditions(&self) -> Vec<&str> {
        match self {
            Self::Single(s) => vec![s.as_str()],
            Self::Multiple(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

/// Loop control options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopControl {
    /// Variable name for current item
    #[serde(default = "default_loop_var")]
    pub loop_var: String,

    /// Variable name for item index
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_var: Option<String>,

    /// Label for display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Pause between iterations in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pause: Option<u64>,

    /// Extended loop info
    #[serde(default)]
    pub extended: bool,
}

fn default_loop_var() -> String {
    "item".to_string()
}

/// Serial execution specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SerialSpec {
    /// Fixed batch size
    Fixed(usize),
    /// Percentage of hosts
    Percentage(String),
    /// Progressive batch sizes
    Progressive(Vec<SerialSpec>),
}

impl SerialSpec {
    /// Calculate batch sizes for a given number of hosts.
    /// Returns a vector of batch sizes that should be used in order.
    pub fn calculate_batches(&self, total_hosts: usize) -> Vec<usize> {
        match self {
            SerialSpec::Fixed(size) => {
                if total_hosts == 0 || *size == 0 {
                    return vec![];
                }
                vec![*size]
            }
            SerialSpec::Percentage(pct) => {
                if total_hosts == 0 {
                    return vec![];
                }

                // Parse percentage (e.g., "50%" -> 50)
                let pct_value = pct
                    .trim_end_matches('%')
                    .parse::<f64>()
                    .unwrap_or(100.0)
                    .max(0.0)
                    .min(100.0);

                let batch_size = ((total_hosts as f64 * pct_value / 100.0).ceil() as usize).max(1);
                vec![batch_size]
            }
            SerialSpec::Progressive(specs) => {
                if total_hosts == 0 {
                    return vec![];
                }

                // Calculate each batch size in the progression
                specs
                    .iter()
                    .flat_map(|spec| spec.calculate_batches(total_hosts))
                    .collect()
            }
        }
    }

    /// Split hosts into batches according to the serial specification.
    pub fn batch_hosts<'a>(&self, hosts: &'a [String]) -> Vec<&'a [String]> {
        let total_hosts = hosts.len();
        if total_hosts == 0 {
            return vec![];
        }

        let batch_sizes = self.calculate_batches(total_hosts);
        if batch_sizes.is_empty() {
            return vec![hosts];
        }

        let mut batches = Vec::new();
        let mut remaining_hosts = hosts;

        // For progressive batches, cycle through batch sizes
        let mut batch_idx = 0;
        while !remaining_hosts.is_empty() {
            let batch_size = batch_sizes[batch_idx % batch_sizes.len()];
            let batch_size = batch_size.min(remaining_hosts.len());

            let (batch, rest) = remaining_hosts.split_at(batch_size);
            batches.push(batch);
            remaining_hosts = rest;

            batch_idx += 1;
        }

        batches
    }
}

/// A handler (special task triggered by notifications).
#[derive(Debug, Clone, Serialize)]
pub struct Handler {
    /// Handler name (must match notify in tasks)
    pub name: String,

    /// The task to execute
    pub task: Task,

    /// Listen to additional names (can be string or array)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub listen: Vec<String>,
}

impl<'de> Deserialize<'de> for Handler {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        // Deserialize as a generic Value first
        let value = serde_json::Value::deserialize(deserializer)?;

        let obj = value
            .as_object()
            .ok_or_else(|| D::Error::custom("handler must be an object"))?;

        // Parse listen as string or vec
        let listen = match obj.get("listen") {
            Some(serde_json::Value::String(s)) => vec![s.clone()],
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };

        // Get the handler name (optional if `listen` is provided)
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_default();

        if name.is_empty() && listen.is_empty() {
            return Err(D::Error::custom("handler must have a name or listen"));
        }

        // Deserialize the task from the same object
        let task: Task = serde_json::from_value(value.clone())
            .map_err(|e| D::Error::custom(format!("failed to parse handler task: {}", e)))?;

        Ok(Handler { name, task, listen })
    }
}

impl Handler {
    /// Creates a new handler.
    pub fn new(name: impl Into<String>, task: Task) -> Self {
        Self {
            name: name.into(),
            task,
            listen: Vec::new(),
        }
    }

    /// Returns all names this handler responds to.
    pub fn trigger_names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        if !self.name.is_empty() {
            names.push(self.name.as_str());
        }
        names.extend(self.listen.iter().map(String::as_str));
        names
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_playbook() {
        let yaml = r#"
- name: Test Play
  hosts: all
  tasks:
    - name: Echo hello
      command: echo hello
"#;
        let result = Playbook::from_yaml(yaml, None);
        assert!(result.is_ok());
        let playbook = result.unwrap();
        assert_eq!(playbook.plays.len(), 1);
        assert_eq!(playbook.plays[0].name, "Test Play");
    }
}
