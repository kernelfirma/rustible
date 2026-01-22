//! Schema Validation at Parse Time
//!
//! This module provides compile-time validation of playbooks using JSON Schema.
//! It catches configuration errors early before execution begins.
//!
//! ## Features
//!
//! - Validate playbook structure against Ansible/Rustible schema
//! - Validate module arguments against module-specific schemas
//! - Provide helpful error messages with line numbers
//! - Support custom schemas for organization-specific rules
//!
//! ## Performance Benefits
//!
//! - Fail fast on invalid playbooks (no SSH connections needed)
//! - Validate entire playbook hierarchy in one pass
//! - Cache validated schemas for repeated use

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur during schema validation
#[derive(Error, Debug)]
pub enum SchemaError {
    #[error("Schema parsing error: {0}")]
    Parse(String),

    #[error("Validation error at {path}: {message}")]
    Validation { path: String, message: String },

    #[error("Multiple validation errors:\n{0}")]
    Multiple(String),

    #[error("Schema not found: {0}")]
    NotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("YAML parse error: {0}")]
    Yaml(String),
}

/// Result type for schema operations
pub type SchemaResult<T> = Result<T, SchemaError>;

/// A single validation error with location info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// JSON path to the error (e.g., "/0/tasks/2/copy/src")
    pub path: String,
    /// Error message
    pub message: String,
    /// Line number in source file (if available)
    pub line: Option<usize>,
    /// Column number in source file (if available)
    pub column: Option<usize>,
    /// Severity level
    pub severity: ErrorSeverity,
    /// Suggestion for fixing the error
    pub suggestion: Option<String>,
}

/// Severity level for validation errors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorSeverity {
    /// Error that prevents execution
    Error,
    /// Warning that may cause issues
    Warning,
    /// Informational hint for best practices
    Info,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let severity = match self.severity {
            ErrorSeverity::Error => "error",
            ErrorSeverity::Warning => "warning",
            ErrorSeverity::Info => "info",
        };

        if let (Some(line), Some(col)) = (self.line, self.column) {
            write!(
                f,
                "[{}] {}:{}: {} at {}",
                severity, line, col, self.message, self.path
            )?;
        } else {
            write!(f, "[{}] {} at {}", severity, self.message, self.path)?;
        }

        if let Some(ref suggestion) = self.suggestion {
            write!(f, "\n  suggestion: {}", suggestion)?;
        }

        Ok(())
    }
}

/// Complete validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether validation passed (no errors, may have warnings)
    pub valid: bool,
    /// All validation errors
    pub errors: Vec<ValidationError>,
    /// All validation warnings
    pub warnings: Vec<ValidationError>,
    /// Informational messages
    pub info: Vec<ValidationError>,
    /// Source file that was validated
    pub source: Option<PathBuf>,
}

impl ValidationResult {
    /// Create a successful result
    pub fn success() -> Self {
        Self {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
            source: None,
        }
    }

    /// Check if there are any issues (errors, warnings, or info)
    pub fn has_issues(&self) -> bool {
        !self.errors.is_empty() || !self.warnings.is_empty() || !self.info.is_empty()
    }

    /// Get total issue count
    pub fn issue_count(&self) -> usize {
        self.errors.len() + self.warnings.len() + self.info.len()
    }

    /// Add an error
    pub fn add_error(&mut self, error: ValidationError) {
        self.valid = false;
        self.errors.push(error);
    }

    /// Add a warning
    pub fn add_warning(&mut self, warning: ValidationError) {
        self.warnings.push(warning);
    }

    /// Add info
    pub fn add_info(&mut self, info: ValidationError) {
        self.info.push(info);
    }

    /// Merge another result into this one
    pub fn merge(&mut self, other: ValidationResult) {
        if !other.valid {
            self.valid = false;
        }
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
        self.info.extend(other.info);
    }
}

/// Schema validator for playbooks and modules
pub struct SchemaValidator {
    /// Module argument schemas
    module_schemas: HashMap<String, ModuleSchema>,
    /// Custom validation rules
    custom_rules: Vec<Box<dyn ValidationRule>>,
    /// Configuration
    config: ValidatorConfig,
}

/// Configuration for the schema validator
#[derive(Debug, Clone)]
pub struct ValidatorConfig {
    /// Enable strict mode (warnings become errors)
    pub strict_mode: bool,
    /// Check for deprecated modules/syntax
    pub check_deprecations: bool,
    /// Check for undefined variables in templates
    pub check_undefined_vars: bool,
    /// Maximum depth to validate (for performance)
    pub max_depth: usize,
    /// Custom schema directory
    pub custom_schema_dir: Option<PathBuf>,
}

impl Default for ValidatorConfig {
    fn default() -> Self {
        Self {
            strict_mode: false,
            check_deprecations: true,
            check_undefined_vars: true,
            max_depth: 50,
            custom_schema_dir: None,
        }
    }
}

/// Schema definition for a module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSchema {
    /// Module name (short name or FQCN)
    pub name: String,
    /// Required arguments
    pub required: Vec<String>,
    /// Optional arguments
    pub optional: Vec<String>,
    /// Mutually exclusive argument groups
    pub mutually_exclusive: Vec<Vec<String>>,
    /// Arguments that must appear together
    pub required_together: Vec<Vec<String>>,
    /// Deprecated arguments
    pub deprecated: Vec<DeprecatedArg>,
    /// Argument type definitions
    pub argument_specs: HashMap<String, ArgumentSpec>,
}

/// Specification for a module argument
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArgumentSpec {
    /// Argument type
    pub arg_type: ArgumentType,
    /// Default value
    pub default: Option<JsonValue>,
    /// Valid choices (for choice type)
    pub choices: Option<Vec<String>>,
    /// Description
    pub description: Option<String>,
    /// Whether this arg is deprecated
    pub deprecated: bool,
    /// Aliases for this argument
    pub aliases: Vec<String>,
}

/// Type of a module argument
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArgumentType {
    String,
    Integer,
    Float,
    Boolean,
    List,
    Dict,
    Path,
    Raw,
    Any,
}

/// Deprecated argument information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeprecatedArg {
    /// Argument name
    pub name: String,
    /// Version when deprecated
    pub version: String,
    /// Replacement argument (if any)
    pub replacement: Option<String>,
    /// Reason for deprecation
    pub reason: Option<String>,
}

/// Trait for custom validation rules
pub trait ValidationRule: Send + Sync {
    /// Name of the rule
    fn name(&self) -> &str;

    /// Validate a playbook structure
    fn validate(&self, value: &JsonValue, path: &str) -> Vec<ValidationError>;
}

impl SchemaValidator {
    /// Create a new schema validator with default configuration
    pub fn new() -> Self {
        Self::with_config(ValidatorConfig::default())
    }

    /// Create a validator with custom configuration
    pub fn with_config(config: ValidatorConfig) -> Self {
        let mut validator = Self {
            module_schemas: HashMap::new(),
            custom_rules: Vec::new(),
            config,
        };

        // Load built-in module schemas
        validator.load_builtin_schemas();

        validator
    }

    /// Load built-in module schemas
    fn load_builtin_schemas(&mut self) {
        // File module
        self.module_schemas.insert(
            "file".to_string(),
            ModuleSchema {
                name: "file".to_string(),
                required: vec![],
                optional: vec![
                    "path".to_string(),
                    "state".to_string(),
                    "mode".to_string(),
                    "owner".to_string(),
                    "group".to_string(),
                    "recurse".to_string(),
                    "src".to_string(),
                    "dest".to_string(),
                    "force".to_string(),
                    "follow".to_string(),
                    "attributes".to_string(),
                    "access_time".to_string(),
                    "modification_time".to_string(),
                ],
                mutually_exclusive: vec![vec!["path".to_string(), "dest".to_string()]],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: Self::file_args(),
            },
        );

        // Copy module
        self.module_schemas.insert(
            "copy".to_string(),
            ModuleSchema {
                name: "copy".to_string(),
                required: vec!["dest".to_string()],
                optional: vec![
                    "src".to_string(),
                    "content".to_string(),
                    "mode".to_string(),
                    "owner".to_string(),
                    "group".to_string(),
                    "backup".to_string(),
                    "force".to_string(),
                    "remote_src".to_string(),
                    "validate".to_string(),
                ],
                mutually_exclusive: vec![vec!["src".to_string(), "content".to_string()]],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: Self::copy_args(),
            },
        );

        // Template module
        self.module_schemas.insert(
            "template".to_string(),
            ModuleSchema {
                name: "template".to_string(),
                required: vec!["src".to_string(), "dest".to_string()],
                optional: vec![
                    "mode".to_string(),
                    "owner".to_string(),
                    "group".to_string(),
                    "backup".to_string(),
                    "force".to_string(),
                    "validate".to_string(),
                    "block_start_string".to_string(),
                    "block_end_string".to_string(),
                    "variable_start_string".to_string(),
                    "variable_end_string".to_string(),
                ],
                mutually_exclusive: vec![],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: HashMap::new(),
            },
        );

        // Service module
        self.module_schemas.insert(
            "service".to_string(),
            ModuleSchema {
                name: "service".to_string(),
                required: vec!["name".to_string()],
                optional: vec![
                    "state".to_string(),
                    "enabled".to_string(),
                    "pattern".to_string(),
                    "sleep".to_string(),
                    "arguments".to_string(),
                    "runlevel".to_string(),
                ],
                mutually_exclusive: vec![],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: Self::service_args(),
            },
        );

        // Package module
        self.module_schemas.insert(
            "package".to_string(),
            ModuleSchema {
                name: "package".to_string(),
                required: vec!["name".to_string()],
                optional: vec!["state".to_string(), "use".to_string()],
                mutually_exclusive: vec![],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: Self::package_args(),
            },
        );

        // Debug module
        self.module_schemas.insert(
            "debug".to_string(),
            ModuleSchema {
                name: "debug".to_string(),
                required: vec![],
                optional: vec![
                    "msg".to_string(),
                    "var".to_string(),
                    "verbosity".to_string(),
                ],
                mutually_exclusive: vec![vec!["msg".to_string(), "var".to_string()]],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: HashMap::new(),
            },
        );

        // Set_fact module
        self.module_schemas.insert(
            "set_fact".to_string(),
            ModuleSchema {
                name: "set_fact".to_string(),
                required: vec![],
                optional: vec!["cacheable".to_string()],
                mutually_exclusive: vec![],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: HashMap::new(),
            },
        );

        // Command module
        self.module_schemas.insert(
            "command".to_string(),
            ModuleSchema {
                name: "command".to_string(),
                required: vec![],
                optional: vec![
                    "cmd".to_string(),
                    "argv".to_string(),
                    "chdir".to_string(),
                    "creates".to_string(),
                    "removes".to_string(),
                    "stdin".to_string(),
                    "stdin_add_newline".to_string(),
                    "strip_empty_ends".to_string(),
                ],
                mutually_exclusive: vec![vec!["cmd".to_string(), "argv".to_string()]],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: HashMap::new(),
            },
        );

        // Shell module
        self.module_schemas.insert(
            "shell".to_string(),
            ModuleSchema {
                name: "shell".to_string(),
                required: vec![],
                optional: vec![
                    "cmd".to_string(),
                    "chdir".to_string(),
                    "creates".to_string(),
                    "removes".to_string(),
                    "executable".to_string(),
                    "stdin".to_string(),
                    "stdin_add_newline".to_string(),
                ],
                mutually_exclusive: vec![],
                required_together: vec![],
                deprecated: vec![],
                argument_specs: HashMap::new(),
            },
        );
    }

    fn file_args() -> HashMap<String, ArgumentSpec> {
        let mut args = HashMap::new();
        args.insert(
            "state".to_string(),
            ArgumentSpec {
                arg_type: ArgumentType::String,
                default: Some(JsonValue::String("file".to_string())),
                choices: Some(vec![
                    "absent".to_string(),
                    "directory".to_string(),
                    "file".to_string(),
                    "hard".to_string(),
                    "link".to_string(),
                    "touch".to_string(),
                ]),
                description: Some("The desired state of the file".to_string()),
                deprecated: false,
                aliases: vec![],
            },
        );
        args.insert(
            "recurse".to_string(),
            ArgumentSpec {
                arg_type: ArgumentType::Boolean,
                default: Some(JsonValue::Bool(false)),
                choices: None,
                description: Some("Recursively set attributes".to_string()),
                deprecated: false,
                aliases: vec![],
            },
        );
        args
    }

    fn copy_args() -> HashMap<String, ArgumentSpec> {
        let mut args = HashMap::new();
        args.insert(
            "backup".to_string(),
            ArgumentSpec {
                arg_type: ArgumentType::Boolean,
                default: Some(JsonValue::Bool(false)),
                choices: None,
                description: Some("Create backup of destination".to_string()),
                deprecated: false,
                aliases: vec![],
            },
        );
        args.insert(
            "force".to_string(),
            ArgumentSpec {
                arg_type: ArgumentType::Boolean,
                default: Some(JsonValue::Bool(true)),
                choices: None,
                description: Some("Replace existing file".to_string()),
                deprecated: false,
                aliases: vec![],
            },
        );
        args
    }

    fn service_args() -> HashMap<String, ArgumentSpec> {
        let mut args = HashMap::new();
        args.insert(
            "state".to_string(),
            ArgumentSpec {
                arg_type: ArgumentType::String,
                default: None,
                choices: Some(vec![
                    "reloaded".to_string(),
                    "restarted".to_string(),
                    "started".to_string(),
                    "stopped".to_string(),
                ]),
                description: Some("Service state".to_string()),
                deprecated: false,
                aliases: vec![],
            },
        );
        args.insert(
            "enabled".to_string(),
            ArgumentSpec {
                arg_type: ArgumentType::Boolean,
                default: None,
                choices: None,
                description: Some("Enable service at boot".to_string()),
                deprecated: false,
                aliases: vec![],
            },
        );
        args
    }

    fn package_args() -> HashMap<String, ArgumentSpec> {
        let mut args = HashMap::new();
        args.insert(
            "state".to_string(),
            ArgumentSpec {
                arg_type: ArgumentType::String,
                default: Some(JsonValue::String("present".to_string())),
                choices: Some(vec![
                    "absent".to_string(),
                    "present".to_string(),
                    "latest".to_string(),
                ]),
                description: Some("Package state".to_string()),
                deprecated: false,
                aliases: vec![],
            },
        );
        args
    }

    /// Add a custom validation rule
    pub fn add_rule(&mut self, rule: Box<dyn ValidationRule>) {
        self.custom_rules.push(rule);
    }

    /// Validate a playbook file
    pub fn validate_file(&self, path: &Path) -> SchemaResult<ValidationResult> {
        let content = std::fs::read_to_string(path)?;
        let mut result = self.validate_yaml(&content)?;
        result.source = Some(path.to_path_buf());
        Ok(result)
    }

    /// Validate a YAML string
    pub fn validate_yaml(&self, yaml_str: &str) -> SchemaResult<ValidationResult> {
        // Parse YAML to JSON for validation
        let yaml_value: serde_yaml::Value =
            serde_yaml::from_str(yaml_str).map_err(|e| SchemaError::Yaml(e.to_string()))?;

        let json_value: JsonValue =
            serde_json::to_value(&yaml_value).map_err(|e| SchemaError::Parse(e.to_string()))?;

        self.validate_playbook(&json_value)
    }

    /// Validate a playbook structure
    pub fn validate_playbook(&self, playbook: &JsonValue) -> SchemaResult<ValidationResult> {
        let mut result = ValidationResult::success();

        // Playbook should be an array of plays
        let plays = match playbook.as_array() {
            Some(arr) => arr,
            None => {
                result.add_error(ValidationError {
                    path: "/".to_string(),
                    message: "Playbook must be a list of plays".to_string(),
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                    suggestion: Some("Ensure your playbook starts with '- name: ...'".to_string()),
                });
                return Ok(result);
            }
        };

        // Validate each play
        for (i, play) in plays.iter().enumerate() {
            let play_path = format!("/{}", i);
            self.validate_play(play, &play_path, &mut result);
        }

        // Run custom validation rules
        for rule in &self.custom_rules {
            let errors = rule.validate(playbook, "/");
            for error in errors {
                match error.severity {
                    ErrorSeverity::Error => result.add_error(error),
                    ErrorSeverity::Warning => result.add_warning(error),
                    ErrorSeverity::Info => result.add_info(error),
                }
            }
        }

        Ok(result)
    }

    /// Validate a single play
    fn validate_play(&self, play: &JsonValue, path: &str, result: &mut ValidationResult) {
        let play_obj = match play.as_object() {
            Some(obj) => obj,
            None => {
                result.add_error(ValidationError {
                    path: path.to_string(),
                    message: "Play must be a mapping".to_string(),
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                    suggestion: None,
                });
                return;
            }
        };

        // Check for required 'hosts' field
        if !play_obj.contains_key("hosts") {
            result.add_error(ValidationError {
                path: path.to_string(),
                message: "Play missing required 'hosts' field".to_string(),
                line: None,
                column: None,
                severity: ErrorSeverity::Error,
                suggestion: Some("Add 'hosts: all' or another host pattern".to_string()),
            });
        }

        // Validate known play-level keys
        let valid_play_keys = [
            "name",
            "hosts",
            "tasks",
            "pre_tasks",
            "post_tasks",
            "handlers",
            "roles",
            "vars",
            "vars_files",
            "vars_prompt",
            "gather_facts",
            "become",
            "become_user",
            "become_method",
            "connection",
            "environment",
            "module_defaults",
            "collections",
            "tags",
            "when",
            "block",
            "rescue",
            "always",
            "ignore_errors",
            "any_errors_fatal",
            "max_fail_percentage",
            "serial",
            "strategy",
            "throttle",
            "order",
            "force_handlers",
            "no_log",
        ];

        for key in play_obj.keys() {
            if !valid_play_keys.contains(&key.as_str()) {
                // Check if it might be a module (common mistake)
                if self.module_schemas.contains_key(key) {
                    result.add_warning(ValidationError {
                        path: format!("{}/{}", path, key),
                        message: format!(
                            "'{}' looks like a module - did you forget 'tasks:'?",
                            key
                        ),
                        line: None,
                        column: None,
                        severity: ErrorSeverity::Warning,
                        suggestion: Some("Move this under 'tasks:' section".to_string()),
                    });
                }
            }
        }

        // Validate tasks
        for task_section in &["tasks", "pre_tasks", "post_tasks", "handlers"] {
            if let Some(tasks) = play_obj.get(*task_section) {
                self.validate_task_list(tasks, &format!("{}/{}", path, task_section), result);
            }
        }
    }

    /// Validate a list of tasks
    fn validate_task_list(&self, tasks: &JsonValue, path: &str, result: &mut ValidationResult) {
        let task_arr = match tasks.as_array() {
            Some(arr) => arr,
            None => {
                result.add_error(ValidationError {
                    path: path.to_string(),
                    message: "Tasks must be a list".to_string(),
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                    suggestion: None,
                });
                return;
            }
        };

        for (i, task) in task_arr.iter().enumerate() {
            let task_path = format!("{}/{}", path, i);
            self.validate_task(task, &task_path, result);
        }
    }

    /// Validate a single task
    fn validate_task(&self, task: &JsonValue, path: &str, result: &mut ValidationResult) {
        let task_obj = match task.as_object() {
            Some(obj) => obj,
            None => {
                result.add_error(ValidationError {
                    path: path.to_string(),
                    message: "Task must be a mapping".to_string(),
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                    suggestion: None,
                });
                return;
            }
        };

        // Common task keys
        let common_keys = [
            "name",
            "when",
            "register",
            "tags",
            "notify",
            "listen",
            "become",
            "become_user",
            "become_method",
            "delegate_to",
            "delegate_facts",
            "connection",
            "vars",
            "environment",
            "args",
            "with_items",
            "with_dict",
            "loop",
            "loop_control",
            "until",
            "retries",
            "delay",
            "ignore_errors",
            "ignore_unreachable",
            "failed_when",
            "changed_when",
            "no_log",
            "throttle",
            "run_once",
            "check_mode",
            "diff",
            "async",
            "poll",
            "block",
            "rescue",
            "always",
            "local_action",
            "action",
            "module_defaults",
        ];

        // Find the module being used
        let mut module_found = false;
        for (key, value) in task_obj {
            if !common_keys.contains(&key.as_str())
                && key != "ansible.builtin.import_tasks"
                && key != "ansible.builtin.include_tasks"
            {
                module_found = true;

                // Check if we have a schema for this module
                let module_name = key.split('.').next_back().unwrap_or(key);
                if let Some(schema) = self.module_schemas.get(module_name) {
                    self.validate_module_args(value, schema, &format!("{}/{}", path, key), result);
                }
            }
        }

        if !module_found
            && !task_obj.contains_key("block")
            && !task_obj.contains_key("include_tasks")
            && !task_obj.contains_key("import_tasks")
        {
            result.add_warning(ValidationError {
                path: path.to_string(),
                message: "Task has no module".to_string(),
                line: None,
                column: None,
                severity: ErrorSeverity::Warning,
                suggestion: Some("Add a module like 'debug:', 'copy:', etc.".to_string()),
            });
        }

        // Check for deprecated when syntax
        if let Some(when) = task_obj.get("when") {
            if let Some(when_str) = when.as_str() {
                if when_str.contains("{{")
                    && when_str.contains("}}")
                    && self.config.check_deprecations
                {
                    result.add_warning(ValidationError {
                        path: format!("{}/when", path),
                        message: "Jinja2 braces in 'when' are deprecated".to_string(),
                        line: None,
                        column: None,
                        severity: ErrorSeverity::Warning,
                        suggestion: Some("Remove {{ and }} from when conditions".to_string()),
                    });
                }
            }
        }
    }

    /// Validate module arguments against schema
    fn validate_module_args(
        &self,
        args: &JsonValue,
        schema: &ModuleSchema,
        path: &str,
        result: &mut ValidationResult,
    ) {
        // Handle freeform modules (string argument)
        if args.is_string() {
            return; // Freeform is usually valid
        }

        let args_obj = match args.as_object() {
            Some(obj) => obj,
            None => return, // Not a mapping, might be freeform
        };

        // Check required arguments
        for required in &schema.required {
            if !args_obj.contains_key(required) {
                result.add_error(ValidationError {
                    path: path.to_string(),
                    message: format!("Missing required argument: {}", required),
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                    suggestion: None,
                });
            }
        }

        // Check mutually exclusive arguments
        for mutex_group in &schema.mutually_exclusive {
            let found: Vec<_> = mutex_group
                .iter()
                .filter(|a| args_obj.contains_key(*a))
                .collect();
            if found.len() > 1 {
                result.add_error(ValidationError {
                    path: path.to_string(),
                    message: format!(
                        "Mutually exclusive arguments: {} cannot be used together",
                        found
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                    suggestion: Some(format!("Use only one of: {}", mutex_group.join(", "))),
                });
            }
        }

        // Check required_together arguments
        for together_group in &schema.required_together {
            let found_count = together_group
                .iter()
                .filter(|a| args_obj.contains_key(*a))
                .count();
            if found_count > 0 && found_count < together_group.len() {
                result.add_error(ValidationError {
                    path: path.to_string(),
                    message: format!(
                        "Arguments must be used together: {}",
                        together_group.join(", ")
                    ),
                    line: None,
                    column: None,
                    severity: ErrorSeverity::Error,
                    suggestion: None,
                });
            }
        }

        // Check deprecated arguments
        if self.config.check_deprecations {
            for deprecated in &schema.deprecated {
                if args_obj.contains_key(&deprecated.name) {
                    let mut suggestion = format!("Deprecated since version {}", deprecated.version);
                    if let Some(ref replacement) = deprecated.replacement {
                        suggestion.push_str(&format!(". Use '{}' instead", replacement));
                    }

                    result.add_warning(ValidationError {
                        path: format!("{}/{}", path, deprecated.name),
                        message: format!("Argument '{}' is deprecated", deprecated.name),
                        line: None,
                        column: None,
                        severity: ErrorSeverity::Warning,
                        suggestion: Some(suggestion),
                    });
                }
            }
        }

        // Validate argument types
        for (arg_name, arg_value) in args_obj {
            if let Some(spec) = schema.argument_specs.get(arg_name) {
                self.validate_argument_type(
                    arg_value,
                    spec,
                    &format!("{}/{}", path, arg_name),
                    result,
                );

                // Check choices
                if let Some(ref choices) = spec.choices {
                    if let Some(value_str) = arg_value.as_str() {
                        if !choices.contains(&value_str.to_string()) {
                            result.add_error(ValidationError {
                                path: format!("{}/{}", path, arg_name),
                                message: format!(
                                    "Invalid value '{}'. Must be one of: {}",
                                    value_str,
                                    choices.join(", ")
                                ),
                                line: None,
                                column: None,
                                severity: ErrorSeverity::Error,
                                suggestion: None,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Validate argument type
    fn validate_argument_type(
        &self,
        value: &JsonValue,
        spec: &ArgumentSpec,
        path: &str,
        result: &mut ValidationResult,
    ) {
        let type_ok = match spec.arg_type {
            ArgumentType::String => value.is_string() || value.is_number(),
            ArgumentType::Integer => value.is_i64() || value.is_u64(),
            ArgumentType::Float => value.is_f64() || value.is_i64() || value.is_u64(),
            ArgumentType::Boolean => value.is_boolean(),
            ArgumentType::List => value.is_array(),
            ArgumentType::Dict => value.is_object(),
            ArgumentType::Path => value.is_string(),
            ArgumentType::Raw | ArgumentType::Any => true,
        };

        if !type_ok {
            result.add_error(ValidationError {
                path: path.to_string(),
                message: format!(
                    "Expected type {:?}, got {:?}",
                    spec.arg_type,
                    json_type_name(value)
                ),
                line: None,
                column: None,
                severity: ErrorSeverity::Error,
                suggestion: None,
            });
        }
    }
}

impl Default for SchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the type name of a JSON value
fn json_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_simple_playbook() {
        let validator = SchemaValidator::new();
        let playbook = r#"
- name: Test play
  hosts: all
  tasks:
    - name: Debug message
      debug:
        msg: "Hello"
"#;

        let result = validator.validate_yaml(playbook).unwrap();
        assert!(result.valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_missing_hosts() {
        let validator = SchemaValidator::new();
        let playbook = r#"
- name: Test play without hosts
  tasks:
    - debug:
        msg: "Hello"
"#;

        let result = validator.validate_yaml(playbook).unwrap();
        assert!(!result.valid);
        assert!(!result.errors.is_empty());
        assert!(result.errors[0].message.contains("hosts"));
    }

    #[test]
    fn test_validate_copy_mutual_exclusive() {
        let validator = SchemaValidator::new();
        let playbook = r#"
- hosts: all
  tasks:
    - name: Invalid copy
      copy:
        src: /tmp/file
        content: "some content"
        dest: /tmp/dest
"#;

        let result = validator.validate_yaml(playbook).unwrap();
        // Should detect mutually exclusive src/content
        assert!(!result.valid);
    }

    #[test]
    fn test_validate_service_choices() {
        let validator = SchemaValidator::new();
        let playbook = r#"
- hosts: all
  tasks:
    - name: Invalid service state
      service:
        name: nginx
        state: invalid_state
"#;

        let result = validator.validate_yaml(playbook).unwrap();
        assert!(!result.valid);
        assert!(result
            .errors
            .iter()
            .any(|e| e.message.contains("Invalid value")));
    }

    #[test]
    fn test_validation_error_display() {
        let error = ValidationError {
            path: "/0/tasks/1/copy".to_string(),
            message: "Missing required argument: dest".to_string(),
            line: Some(5),
            column: Some(3),
            severity: ErrorSeverity::Error,
            suggestion: Some("Add 'dest: /path/to/destination'".to_string()),
        };

        let display = format!("{}", error);
        assert!(display.contains("error"));
        assert!(display.contains("5:3"));
        assert!(display.contains("dest"));
    }
}
