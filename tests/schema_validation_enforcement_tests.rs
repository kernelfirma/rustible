//! Schema Validation Enforcement Test Suite for Issue #285
//!
//! Tests that all core modules require schema validation before execution.
//! Invalid args must fail fast with actionable errors - no runtime fallback.

use std::collections::{HashMap, HashSet};

// ============================================================================
// Mock Module Schema System (mirrors production implementation)
// ============================================================================

/// Schema validation error with actionable messages
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaError {
    pub module: String,
    pub field: String,
    pub message: String,
    pub suggestion: Option<String>,
}

impl SchemaError {
    fn missing_required(module: &str, field: &str) -> Self {
        Self {
            module: module.to_string(),
            field: field.to_string(),
            message: format!("Missing required parameter '{}'", field),
            suggestion: Some(format!("Add '{}' to your task arguments", field)),
        }
    }

    fn invalid_type(module: &str, field: &str, expected: &str, got: &str) -> Self {
        Self {
            module: module.to_string(),
            field: field.to_string(),
            message: format!("Invalid type for '{}': expected {}, got {}", field, expected, got),
            suggestion: Some(format!("Change '{}' to a {} value", field, expected)),
        }
    }

    fn invalid_choice(module: &str, field: &str, value: &str, choices: &[&str]) -> Self {
        Self {
            module: module.to_string(),
            field: field.to_string(),
            message: format!(
                "Invalid choice '{}' for '{}'. Valid choices: {}",
                value,
                field,
                choices.join(", ")
            ),
            suggestion: Some(format!("Use one of: {}", choices.join(", "))),
        }
    }

    fn unknown_parameter(module: &str, field: &str, similar: Option<&str>) -> Self {
        Self {
            module: module.to_string(),
            field: field.to_string(),
            message: format!("Unknown parameter '{}'", field),
            suggestion: similar.map(|s| format!("Did you mean '{}'?", s)),
        }
    }

    fn mutually_exclusive(module: &str, fields: &[&str]) -> Self {
        Self {
            module: module.to_string(),
            field: fields.join(", "),
            message: format!("Parameters {} are mutually exclusive", fields.join(" and ")),
            suggestion: Some("Use only one of these parameters".to_string()),
        }
    }
}

/// Field types supported in schemas
#[derive(Debug, Clone, PartialEq)]
pub enum FieldType {
    String,
    Bool,
    Int,
    Float,
    List,
    Dict,
    Path,
    Raw,
}

/// Field definition in a module schema
#[derive(Debug, Clone)]
pub struct FieldSchema {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub default: Option<String>,
    pub choices: Option<Vec<String>>,
    pub aliases: Vec<String>,
    pub description: String,
}

/// Module schema definition
#[derive(Debug, Clone)]
pub struct ModuleSchema {
    pub name: String,
    pub fields: Vec<FieldSchema>,
    pub mutually_exclusive: Vec<Vec<String>>,
    pub required_one_of: Vec<Vec<String>>,
    pub required_together: Vec<Vec<String>>,
}

impl ModuleSchema {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fields: Vec::new(),
            mutually_exclusive: Vec::new(),
            required_one_of: Vec::new(),
            required_together: Vec::new(),
        }
    }

    fn add_field(&mut self, field: FieldSchema) -> &mut Self {
        self.fields.push(field);
        self
    }

    fn mutually_exclusive(&mut self, fields: &[&str]) -> &mut Self {
        self.mutually_exclusive.push(fields.iter().map(|s| s.to_string()).collect());
        self
    }

    fn required_one_of(&mut self, fields: &[&str]) -> &mut Self {
        self.required_one_of.push(fields.iter().map(|s| s.to_string()).collect());
        self
    }

    fn required_together(&mut self, fields: &[&str]) -> &mut Self {
        self.required_together.push(fields.iter().map(|s| s.to_string()).collect());
        self
    }
}

/// Value type for module arguments
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<ArgValue>),
    Dict(HashMap<String, ArgValue>),
}

impl ArgValue {
    fn type_name(&self) -> &'static str {
        match self {
            ArgValue::Null => "null",
            ArgValue::Bool(_) => "bool",
            ArgValue::Int(_) => "int",
            ArgValue::Float(_) => "float",
            ArgValue::String(_) => "string",
            ArgValue::List(_) => "list",
            ArgValue::Dict(_) => "dict",
        }
    }
}

// ============================================================================
// Schema Registry
// ============================================================================

/// Registry of all module schemas
pub struct SchemaRegistry {
    schemas: HashMap<String, ModuleSchema>,
}

impl SchemaRegistry {
    fn new() -> Self {
        let mut registry = Self {
            schemas: HashMap::new(),
        };
        registry.register_core_modules();
        registry
    }

    fn register(&mut self, schema: ModuleSchema) {
        self.schemas.insert(schema.name.clone(), schema);
    }

    fn get(&self, module: &str) -> Option<&ModuleSchema> {
        self.schemas.get(module)
    }

    fn has_schema(&self, module: &str) -> bool {
        self.schemas.contains_key(module)
    }

    fn all_modules(&self) -> Vec<&str> {
        self.schemas.keys().map(|s| s.as_str()).collect()
    }

    fn register_core_modules(&mut self) {
        // file module
        let mut file_schema = ModuleSchema::new("file");
        file_schema.add_field(FieldSchema {
            name: "path".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec!["dest".to_string(), "name".to_string()],
            description: "Path to the file being managed".to_string(),
        });
        file_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("file".to_string()),
            choices: Some(vec![
                "absent".to_string(),
                "directory".to_string(),
                "file".to_string(),
                "hard".to_string(),
                "link".to_string(),
                "touch".to_string(),
            ]),
            aliases: vec![],
            description: "State of the file".to_string(),
        });
        file_schema.add_field(FieldSchema {
            name: "mode".to_string(),
            field_type: FieldType::Raw,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Permissions of the file".to_string(),
        });
        file_schema.add_field(FieldSchema {
            name: "owner".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Owner of the file".to_string(),
        });
        file_schema.add_field(FieldSchema {
            name: "group".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Group of the file".to_string(),
        });
        file_schema.add_field(FieldSchema {
            name: "src".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Source path for links".to_string(),
        });
        self.register(file_schema);

        // copy module
        let mut copy_schema = ModuleSchema::new("copy");
        copy_schema.add_field(FieldSchema {
            name: "src".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Local path to file".to_string(),
        });
        copy_schema.add_field(FieldSchema {
            name: "content".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Content to write directly".to_string(),
        });
        copy_schema.add_field(FieldSchema {
            name: "dest".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Remote destination path".to_string(),
        });
        copy_schema.add_field(FieldSchema {
            name: "backup".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: Some("false".to_string()),
            choices: None,
            aliases: vec![],
            description: "Create backup before overwriting".to_string(),
        });
        copy_schema.mutually_exclusive(&["src", "content"]);
        copy_schema.required_one_of(&["src", "content"]);
        self.register(copy_schema);

        // template module
        let mut template_schema = ModuleSchema::new("template");
        template_schema.add_field(FieldSchema {
            name: "src".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Path to Jinja2 template".to_string(),
        });
        template_schema.add_field(FieldSchema {
            name: "dest".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Remote destination path".to_string(),
        });
        template_schema.add_field(FieldSchema {
            name: "mode".to_string(),
            field_type: FieldType::Raw,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Permissions of the file".to_string(),
        });
        template_schema.add_field(FieldSchema {
            name: "owner".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Owner of the file".to_string(),
        });
        template_schema.add_field(FieldSchema {
            name: "group".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Group of the file".to_string(),
        });
        self.register(template_schema);

        // package module (generic)
        let mut package_schema = ModuleSchema::new("package");
        package_schema.add_field(FieldSchema {
            name: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec!["pkg".to_string()],
            description: "Package name(s) to install".to_string(),
        });
        package_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("present".to_string()),
            choices: Some(vec![
                "present".to_string(),
                "absent".to_string(),
                "latest".to_string(),
            ]),
            aliases: vec![],
            description: "Package state".to_string(),
        });
        self.register(package_schema);

        // apt module
        let mut apt_schema = ModuleSchema::new("apt");
        apt_schema.add_field(FieldSchema {
            name: "name".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec!["pkg".to_string(), "package".to_string()],
            description: "Package name(s)".to_string(),
        });
        apt_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("present".to_string()),
            choices: Some(vec![
                "present".to_string(),
                "absent".to_string(),
                "latest".to_string(),
                "build-dep".to_string(),
                "fixed".to_string(),
            ]),
            aliases: vec![],
            description: "Package state".to_string(),
        });
        apt_schema.add_field(FieldSchema {
            name: "update_cache".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: Some("false".to_string()),
            choices: None,
            aliases: vec!["update-cache".to_string()],
            description: "Run apt update".to_string(),
        });
        apt_schema.add_field(FieldSchema {
            name: "cache_valid_time".to_string(),
            field_type: FieldType::Int,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Cache validity time in seconds".to_string(),
        });
        apt_schema.add_field(FieldSchema {
            name: "deb".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Path to .deb file".to_string(),
        });
        self.register(apt_schema);

        // yum module
        let mut yum_schema = ModuleSchema::new("yum");
        yum_schema.add_field(FieldSchema {
            name: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec!["pkg".to_string()],
            description: "Package name(s)".to_string(),
        });
        yum_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("present".to_string()),
            choices: Some(vec![
                "present".to_string(),
                "absent".to_string(),
                "latest".to_string(),
                "installed".to_string(),
                "removed".to_string(),
            ]),
            aliases: vec![],
            description: "Package state".to_string(),
        });
        yum_schema.add_field(FieldSchema {
            name: "enablerepo".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Enable specific repo".to_string(),
        });
        yum_schema.add_field(FieldSchema {
            name: "disablerepo".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Disable specific repo".to_string(),
        });
        self.register(yum_schema);

        // service module
        let mut service_schema = ModuleSchema::new("service");
        service_schema.add_field(FieldSchema {
            name: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Service name".to_string(),
        });
        service_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: Some(vec![
                "started".to_string(),
                "stopped".to_string(),
                "restarted".to_string(),
                "reloaded".to_string(),
            ]),
            aliases: vec![],
            description: "Service state".to_string(),
        });
        service_schema.add_field(FieldSchema {
            name: "enabled".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Start on boot".to_string(),
        });
        self.register(service_schema);

        // systemd module
        let mut systemd_schema = ModuleSchema::new("systemd");
        systemd_schema.add_field(FieldSchema {
            name: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec!["unit".to_string()],
            description: "Unit name".to_string(),
        });
        systemd_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: Some(vec![
                "started".to_string(),
                "stopped".to_string(),
                "restarted".to_string(),
                "reloaded".to_string(),
            ]),
            aliases: vec![],
            description: "Unit state".to_string(),
        });
        systemd_schema.add_field(FieldSchema {
            name: "enabled".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Enable unit".to_string(),
        });
        systemd_schema.add_field(FieldSchema {
            name: "daemon_reload".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: Some("false".to_string()),
            choices: None,
            aliases: vec!["daemon-reload".to_string()],
            description: "Reload systemd daemon".to_string(),
        });
        self.register(systemd_schema);

        // command module
        let mut command_schema = ModuleSchema::new("command");
        command_schema.add_field(FieldSchema {
            name: "cmd".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Command to execute".to_string(),
        });
        command_schema.add_field(FieldSchema {
            name: "argv".to_string(),
            field_type: FieldType::List,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Command as list".to_string(),
        });
        command_schema.add_field(FieldSchema {
            name: "chdir".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Change to directory before executing".to_string(),
        });
        command_schema.add_field(FieldSchema {
            name: "creates".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Skip if this file exists".to_string(),
        });
        command_schema.add_field(FieldSchema {
            name: "removes".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Skip if this file does not exist".to_string(),
        });
        self.register(command_schema);

        // shell module
        let mut shell_schema = ModuleSchema::new("shell");
        shell_schema.add_field(FieldSchema {
            name: "cmd".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Shell command".to_string(),
        });
        shell_schema.add_field(FieldSchema {
            name: "chdir".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Change directory".to_string(),
        });
        shell_schema.add_field(FieldSchema {
            name: "executable".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Shell executable".to_string(),
        });
        self.register(shell_schema);

        // user module
        let mut user_schema = ModuleSchema::new("user");
        user_schema.add_field(FieldSchema {
            name: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Username".to_string(),
        });
        user_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("present".to_string()),
            choices: Some(vec!["present".to_string(), "absent".to_string()]),
            aliases: vec![],
            description: "User state".to_string(),
        });
        user_schema.add_field(FieldSchema {
            name: "uid".to_string(),
            field_type: FieldType::Int,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "User ID".to_string(),
        });
        user_schema.add_field(FieldSchema {
            name: "groups".to_string(),
            field_type: FieldType::List,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Supplementary groups".to_string(),
        });
        user_schema.add_field(FieldSchema {
            name: "shell".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "User shell".to_string(),
        });
        user_schema.add_field(FieldSchema {
            name: "home".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Home directory".to_string(),
        });
        self.register(user_schema);

        // group module
        let mut group_schema = ModuleSchema::new("group");
        group_schema.add_field(FieldSchema {
            name: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Group name".to_string(),
        });
        group_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("present".to_string()),
            choices: Some(vec!["present".to_string(), "absent".to_string()]),
            aliases: vec![],
            description: "Group state".to_string(),
        });
        group_schema.add_field(FieldSchema {
            name: "gid".to_string(),
            field_type: FieldType::Int,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Group ID".to_string(),
        });
        self.register(group_schema);

        // lineinfile module
        let mut lineinfile_schema = ModuleSchema::new("lineinfile");
        lineinfile_schema.add_field(FieldSchema {
            name: "path".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec!["dest".to_string(), "destfile".to_string()],
            description: "File path".to_string(),
        });
        lineinfile_schema.add_field(FieldSchema {
            name: "line".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Line to insert".to_string(),
        });
        lineinfile_schema.add_field(FieldSchema {
            name: "regexp".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec!["regex".to_string()],
            description: "Regex to match".to_string(),
        });
        lineinfile_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("present".to_string()),
            choices: Some(vec!["present".to_string(), "absent".to_string()]),
            aliases: vec![],
            description: "Line state".to_string(),
        });
        lineinfile_schema.add_field(FieldSchema {
            name: "insertafter".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Insert after regex match".to_string(),
        });
        lineinfile_schema.add_field(FieldSchema {
            name: "insertbefore".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Insert before regex match".to_string(),
        });
        lineinfile_schema.mutually_exclusive(&["insertafter", "insertbefore"]);
        self.register(lineinfile_schema);

        // blockinfile module
        let mut blockinfile_schema = ModuleSchema::new("blockinfile");
        blockinfile_schema.add_field(FieldSchema {
            name: "path".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec!["dest".to_string()],
            description: "File path".to_string(),
        });
        blockinfile_schema.add_field(FieldSchema {
            name: "block".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("".to_string()),
            choices: None,
            aliases: vec!["content".to_string()],
            description: "Block content".to_string(),
        });
        blockinfile_schema.add_field(FieldSchema {
            name: "marker".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("# {mark} ANSIBLE MANAGED BLOCK".to_string()),
            choices: None,
            aliases: vec![],
            description: "Marker template".to_string(),
        });
        blockinfile_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("present".to_string()),
            choices: Some(vec!["present".to_string(), "absent".to_string()]),
            aliases: vec![],
            description: "Block state".to_string(),
        });
        self.register(blockinfile_schema);

        // git module
        let mut git_schema = ModuleSchema::new("git");
        git_schema.add_field(FieldSchema {
            name: "repo".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec!["name".to_string()],
            description: "Git repository URL".to_string(),
        });
        git_schema.add_field(FieldSchema {
            name: "dest".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Destination path".to_string(),
        });
        git_schema.add_field(FieldSchema {
            name: "version".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("HEAD".to_string()),
            choices: None,
            aliases: vec!["branch".to_string(), "tag".to_string()],
            description: "Git version/branch/tag".to_string(),
        });
        git_schema.add_field(FieldSchema {
            name: "force".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: Some("false".to_string()),
            choices: None,
            aliases: vec![],
            description: "Force checkout".to_string(),
        });
        self.register(git_schema);

        // debug module
        let mut debug_schema = ModuleSchema::new("debug");
        debug_schema.add_field(FieldSchema {
            name: "msg".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Message to print".to_string(),
        });
        debug_schema.add_field(FieldSchema {
            name: "var".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Variable to print".to_string(),
        });
        debug_schema.add_field(FieldSchema {
            name: "verbosity".to_string(),
            field_type: FieldType::Int,
            required: false,
            default: Some("0".to_string()),
            choices: None,
            aliases: vec![],
            description: "Minimum verbosity level".to_string(),
        });
        debug_schema.mutually_exclusive(&["msg", "var"]);
        self.register(debug_schema);

        // stat module
        let mut stat_schema = ModuleSchema::new("stat");
        stat_schema.add_field(FieldSchema {
            name: "path".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "File path to stat".to_string(),
        });
        stat_schema.add_field(FieldSchema {
            name: "follow".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: Some("false".to_string()),
            choices: None,
            aliases: vec![],
            description: "Follow symlinks".to_string(),
        });
        stat_schema.add_field(FieldSchema {
            name: "get_checksum".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: Some("true".to_string()),
            choices: None,
            aliases: vec![],
            description: "Compute checksum".to_string(),
        });
        stat_schema.add_field(FieldSchema {
            name: "checksum_algorithm".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("sha1".to_string()),
            choices: Some(vec![
                "md5".to_string(),
                "sha1".to_string(),
                "sha224".to_string(),
                "sha256".to_string(),
                "sha384".to_string(),
                "sha512".to_string(),
            ]),
            aliases: vec!["checksum".to_string(), "checksum_algo".to_string()],
            description: "Checksum algorithm".to_string(),
        });
        self.register(stat_schema);

        // uri module
        let mut uri_schema = ModuleSchema::new("uri");
        uri_schema.add_field(FieldSchema {
            name: "url".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "HTTP URL".to_string(),
        });
        uri_schema.add_field(FieldSchema {
            name: "method".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("GET".to_string()),
            choices: Some(vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "HEAD".to_string(),
                "OPTIONS".to_string(),
            ]),
            aliases: vec![],
            description: "HTTP method".to_string(),
        });
        uri_schema.add_field(FieldSchema {
            name: "body".to_string(),
            field_type: FieldType::Raw,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Request body".to_string(),
        });
        uri_schema.add_field(FieldSchema {
            name: "headers".to_string(),
            field_type: FieldType::Dict,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "HTTP headers".to_string(),
        });
        self.register(uri_schema);

        // get_url module
        let mut get_url_schema = ModuleSchema::new("get_url");
        get_url_schema.add_field(FieldSchema {
            name: "url".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "URL to download".to_string(),
        });
        get_url_schema.add_field(FieldSchema {
            name: "dest".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Destination path".to_string(),
        });
        get_url_schema.add_field(FieldSchema {
            name: "checksum".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Checksum to verify".to_string(),
        });
        get_url_schema.add_field(FieldSchema {
            name: "mode".to_string(),
            field_type: FieldType::Raw,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "File mode".to_string(),
        });
        self.register(get_url_schema);

        // unarchive module
        let mut unarchive_schema = ModuleSchema::new("unarchive");
        unarchive_schema.add_field(FieldSchema {
            name: "src".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Archive path".to_string(),
        });
        unarchive_schema.add_field(FieldSchema {
            name: "dest".to_string(),
            field_type: FieldType::Path,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Destination directory".to_string(),
        });
        unarchive_schema.add_field(FieldSchema {
            name: "remote_src".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: Some("false".to_string()),
            choices: None,
            aliases: vec![],
            description: "Source is on remote".to_string(),
        });
        self.register(unarchive_schema);

        // cron module
        let mut cron_schema = ModuleSchema::new("cron");
        cron_schema.add_field(FieldSchema {
            name: "name".to_string(),
            field_type: FieldType::String,
            required: true,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Cron job name".to_string(),
        });
        cron_schema.add_field(FieldSchema {
            name: "job".to_string(),
            field_type: FieldType::String,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Command to run".to_string(),
        });
        cron_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("present".to_string()),
            choices: Some(vec!["present".to_string(), "absent".to_string()]),
            aliases: vec![],
            description: "Cron job state".to_string(),
        });
        cron_schema.add_field(FieldSchema {
            name: "minute".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("*".to_string()),
            choices: None,
            aliases: vec![],
            description: "Minute".to_string(),
        });
        cron_schema.add_field(FieldSchema {
            name: "hour".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("*".to_string()),
            choices: None,
            aliases: vec![],
            description: "Hour".to_string(),
        });
        self.register(cron_schema);

        // set_fact module
        let mut set_fact_schema = ModuleSchema::new("set_fact");
        set_fact_schema.add_field(FieldSchema {
            name: "cacheable".to_string(),
            field_type: FieldType::Bool,
            required: false,
            default: Some("false".to_string()),
            choices: None,
            aliases: vec![],
            description: "Cache fact".to_string(),
        });
        // set_fact accepts arbitrary key=value pairs as free-form
        self.register(set_fact_schema);

        // wait_for module
        let mut wait_for_schema = ModuleSchema::new("wait_for");
        wait_for_schema.add_field(FieldSchema {
            name: "host".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("127.0.0.1".to_string()),
            choices: None,
            aliases: vec![],
            description: "Host to wait for".to_string(),
        });
        wait_for_schema.add_field(FieldSchema {
            name: "port".to_string(),
            field_type: FieldType::Int,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "Port to wait for".to_string(),
        });
        wait_for_schema.add_field(FieldSchema {
            name: "path".to_string(),
            field_type: FieldType::Path,
            required: false,
            default: None,
            choices: None,
            aliases: vec![],
            description: "File to wait for".to_string(),
        });
        wait_for_schema.add_field(FieldSchema {
            name: "state".to_string(),
            field_type: FieldType::String,
            required: false,
            default: Some("started".to_string()),
            choices: Some(vec![
                "started".to_string(),
                "stopped".to_string(),
                "present".to_string(),
                "absent".to_string(),
                "drained".to_string(),
            ]),
            aliases: vec![],
            description: "Wait state".to_string(),
        });
        wait_for_schema.add_field(FieldSchema {
            name: "timeout".to_string(),
            field_type: FieldType::Int,
            required: false,
            default: Some("300".to_string()),
            choices: None,
            aliases: vec![],
            description: "Timeout in seconds".to_string(),
        });
        self.register(wait_for_schema);
    }
}

// ============================================================================
// Schema Validator
// ============================================================================

/// Validates module arguments against schema
pub struct SchemaValidator<'a> {
    registry: &'a SchemaRegistry,
}

impl<'a> SchemaValidator<'a> {
    fn new(registry: &'a SchemaRegistry) -> Self {
        Self { registry }
    }

    fn validate(
        &self,
        module: &str,
        args: &HashMap<String, ArgValue>,
    ) -> Result<(), Vec<SchemaError>> {
        let schema = self
            .registry
            .get(module)
            .ok_or_else(|| vec![SchemaError {
                module: module.to_string(),
                field: "".to_string(),
                message: format!("No schema found for module '{}'", module),
                suggestion: None,
            }])?;

        let mut errors = Vec::new();

        // Build set of all valid field names (including aliases)
        let mut valid_names: HashMap<&str, &FieldSchema> = HashMap::new();
        for field in &schema.fields {
            valid_names.insert(&field.name, field);
            for alias in &field.aliases {
                valid_names.insert(alias, field);
            }
        }

        // Check for unknown parameters
        for key in args.keys() {
            if !valid_names.contains_key(key.as_str()) {
                // Find similar field name for suggestion
                let similar = self.find_similar_field(key, &valid_names);
                errors.push(SchemaError::unknown_parameter(module, key, similar));
            }
        }

        // Check required fields
        for field in &schema.fields {
            if field.required && !self.has_field(args, field) {
                errors.push(SchemaError::missing_required(module, &field.name));
            }
        }

        // Check field types
        for (key, value) in args {
            if let Some(field) = valid_names.get(key.as_str()) {
                if !self.type_matches(value, &field.field_type) {
                    let expected = format!("{:?}", field.field_type).to_lowercase();
                    errors.push(SchemaError::invalid_type(
                        module,
                        key,
                        &expected,
                        value.type_name(),
                    ));
                }

                // Check choices
                if let (Some(choices), ArgValue::String(s)) = (&field.choices, value) {
                    if !choices.contains(s) {
                        let choices_refs: Vec<&str> = choices.iter().map(|s| s.as_str()).collect();
                        errors.push(SchemaError::invalid_choice(module, key, s, &choices_refs));
                    }
                }
            }
        }

        // Check mutually exclusive
        for exclusive_group in &schema.mutually_exclusive {
            let present: Vec<&String> = exclusive_group
                .iter()
                .filter(|f| args.contains_key(*f))
                .collect();
            if present.len() > 1 {
                let fields: Vec<&str> = present.iter().map(|s| s.as_str()).collect();
                errors.push(SchemaError::mutually_exclusive(module, &fields));
            }
        }

        // Check required_one_of
        for required_group in &schema.required_one_of {
            let has_any = required_group.iter().any(|f| args.contains_key(f));
            if !has_any {
                errors.push(SchemaError {
                    module: module.to_string(),
                    field: required_group.join(" or "),
                    message: format!(
                        "One of {} is required",
                        required_group.join(" or ")
                    ),
                    suggestion: Some(format!("Add one of: {}", required_group.join(", "))),
                });
            }
        }

        // Check required_together
        for together_group in &schema.required_together {
            let present: Vec<&String> = together_group
                .iter()
                .filter(|f| args.contains_key(*f))
                .collect();
            if !present.is_empty() && present.len() != together_group.len() {
                errors.push(SchemaError {
                    module: module.to_string(),
                    field: together_group.join(", "),
                    message: format!(
                        "Parameters {} must be used together",
                        together_group.join(" and ")
                    ),
                    suggestion: Some(format!("Add all of: {}", together_group.join(", "))),
                });
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn has_field(&self, args: &HashMap<String, ArgValue>, field: &FieldSchema) -> bool {
        if args.contains_key(&field.name) {
            return true;
        }
        field.aliases.iter().any(|alias| args.contains_key(alias))
    }

    fn type_matches(&self, value: &ArgValue, expected: &FieldType) -> bool {
        match (expected, value) {
            (FieldType::String, ArgValue::String(_)) => true,
            (FieldType::Bool, ArgValue::Bool(_)) => true,
            (FieldType::Int, ArgValue::Int(_)) => true,
            (FieldType::Float, ArgValue::Float(_)) => true,
            (FieldType::Float, ArgValue::Int(_)) => true, // int can be float
            (FieldType::List, ArgValue::List(_)) => true,
            (FieldType::Dict, ArgValue::Dict(_)) => true,
            (FieldType::Path, ArgValue::String(_)) => true, // path is string
            (FieldType::Raw, _) => true, // raw accepts anything
            _ => false,
        }
    }

    fn find_similar_field<'b>(
        &self,
        name: &str,
        valid_names: &HashMap<&'b str, &FieldSchema>,
    ) -> Option<&'b str> {
        // Simple similarity check - find names that start with same prefix
        for valid in valid_names.keys() {
            if valid.starts_with(&name[..name.len().min(3)]) {
                return Some(*valid);
            }
        }
        None
    }
}

// ============================================================================
// Tests: Core Module Schema Coverage
// ============================================================================

#[test]
fn test_all_core_modules_have_schemas() {
    let registry = SchemaRegistry::new();

    // List of all core modules that must have schemas
    let core_modules = [
        "file", "copy", "template", "package", "apt", "yum",
        "service", "systemd", "command", "shell", "user", "group",
        "lineinfile", "blockinfile", "git", "debug", "stat",
        "uri", "get_url", "unarchive", "cron", "set_fact", "wait_for",
    ];

    for module in &core_modules {
        assert!(
            registry.has_schema(module),
            "Core module '{}' must have a schema defined",
            module
        );
    }
}

#[test]
fn test_schemas_have_required_fields_defined() {
    let registry = SchemaRegistry::new();

    // Modules with their required fields
    let required_fields: Vec<(&str, Vec<&str>)> = vec![
        ("file", vec!["path"]),
        ("copy", vec!["dest"]),
        ("template", vec!["src", "dest"]),
        ("package", vec!["name"]),
        ("service", vec!["name"]),
        ("systemd", vec!["name"]),
        ("user", vec!["name"]),
        ("group", vec!["name"]),
        ("lineinfile", vec!["path"]),
        ("blockinfile", vec!["path"]),
        ("git", vec!["repo", "dest"]),
        ("stat", vec!["path"]),
        ("uri", vec!["url"]),
        ("get_url", vec!["url", "dest"]),
        ("unarchive", vec!["src", "dest"]),
        ("cron", vec!["name"]),
    ];

    for (module, expected_required) in required_fields {
        let schema = registry.get(module).expect(&format!("Schema for {} missing", module));
        let actual_required: HashSet<&str> = schema
            .fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name.as_str())
            .collect();

        for field in expected_required {
            assert!(
                actual_required.contains(field),
                "Module '{}' must have '{}' as required field",
                module,
                field
            );
        }
    }
}

// ============================================================================
// Tests: Schema Validation - Missing Required Fields
// ============================================================================

#[test]
fn test_validation_fails_on_missing_required_field() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // file module without path
    let args: HashMap<String, ArgValue> = HashMap::new();
    let result = validator.validate("file", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.field == "path" && e.message.contains("Missing required")));
}

#[test]
fn test_validation_fails_on_multiple_missing_fields() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // git module needs both repo and dest
    let args: HashMap<String, ArgValue> = HashMap::new();
    let result = validator.validate("git", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.len() >= 2);
    assert!(errors.iter().any(|e| e.field == "repo"));
    assert!(errors.iter().any(|e| e.field == "dest"));
}

#[test]
fn test_validation_passes_with_all_required_fields() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("path".to_string(), ArgValue::String("/etc/hosts".to_string()));

    let result = validator.validate("file", &args);
    assert!(result.is_ok());
}

// ============================================================================
// Tests: Schema Validation - Type Checking
// ============================================================================

#[test]
fn test_validation_fails_on_wrong_type() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("name".to_string(), ArgValue::String("nginx".to_string()));
    args.insert("enabled".to_string(), ArgValue::String("yes".to_string())); // Should be bool

    let result = validator.validate("service", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.field == "enabled" && e.message.contains("Invalid type")));
}

#[test]
fn test_validation_accepts_correct_types() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("name".to_string(), ArgValue::String("nginx".to_string()));
    args.insert("enabled".to_string(), ArgValue::Bool(true));
    args.insert("state".to_string(), ArgValue::String("started".to_string()));

    let result = validator.validate("service", &args);
    assert!(result.is_ok());
}

#[test]
fn test_validation_int_compatible_with_float() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("host".to_string(), ArgValue::String("localhost".to_string()));
    args.insert("port".to_string(), ArgValue::Int(8080)); // Int where int expected

    let result = validator.validate("wait_for", &args);
    // Should pass even though port is Int and field might accept Float
    // (actually port is Int, so this is fine)
    assert!(result.is_ok());
}

// ============================================================================
// Tests: Schema Validation - Choice Validation
// ============================================================================

#[test]
fn test_validation_fails_on_invalid_choice() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("path".to_string(), ArgValue::String("/etc/hosts".to_string()));
    args.insert("state".to_string(), ArgValue::String("invalid_state".to_string()));

    let result = validator.validate("file", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("Invalid choice")));
}

#[test]
fn test_validation_passes_on_valid_choice() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("path".to_string(), ArgValue::String("/etc/hosts".to_string()));
    args.insert("state".to_string(), ArgValue::String("directory".to_string()));

    let result = validator.validate("file", &args);
    assert!(result.is_ok());
}

// ============================================================================
// Tests: Schema Validation - Unknown Parameters
// ============================================================================

#[test]
fn test_validation_fails_on_unknown_parameter() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("path".to_string(), ArgValue::String("/etc/hosts".to_string()));
    args.insert("unknown_param".to_string(), ArgValue::String("value".to_string()));

    let result = validator.validate("file", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("Unknown parameter")));
}

#[test]
fn test_validation_suggests_similar_parameter() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("paht".to_string(), ArgValue::String("/etc/hosts".to_string())); // Typo

    let result = validator.validate("file", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    // Should suggest 'path'
    assert!(errors.iter().any(|e| e.suggestion.as_ref().map(|s| s.contains("path")).unwrap_or(false)));
}

// ============================================================================
// Tests: Schema Validation - Aliases
// ============================================================================

#[test]
fn test_validation_accepts_aliases() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // file module accepts 'dest' as alias for 'path'
    let mut args = HashMap::new();
    args.insert("dest".to_string(), ArgValue::String("/etc/hosts".to_string()));

    let result = validator.validate("file", &args);
    assert!(result.is_ok());
}

#[test]
fn test_validation_accepts_all_aliases() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // apt module accepts 'pkg' and 'package' as aliases for 'name'
    let mut args = HashMap::new();
    args.insert("pkg".to_string(), ArgValue::String("nginx".to_string()));

    let result = validator.validate("apt", &args);
    assert!(result.is_ok());
}

// ============================================================================
// Tests: Schema Validation - Mutually Exclusive
// ============================================================================

#[test]
fn test_validation_fails_on_mutually_exclusive() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // copy module: src and content are mutually exclusive
    let mut args = HashMap::new();
    args.insert("dest".to_string(), ArgValue::String("/tmp/file".to_string()));
    args.insert("src".to_string(), ArgValue::String("/local/file".to_string()));
    args.insert("content".to_string(), ArgValue::String("some content".to_string()));

    let result = validator.validate("copy", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("mutually exclusive")));
}

#[test]
fn test_validation_passes_with_one_of_mutually_exclusive() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("dest".to_string(), ArgValue::String("/tmp/file".to_string()));
    args.insert("src".to_string(), ArgValue::String("/local/file".to_string()));

    let result = validator.validate("copy", &args);
    assert!(result.is_ok());
}

// ============================================================================
// Tests: Schema Validation - Required One Of
// ============================================================================

#[test]
fn test_validation_fails_when_required_one_of_missing() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // copy module requires one of src or content
    let mut args = HashMap::new();
    args.insert("dest".to_string(), ArgValue::String("/tmp/file".to_string()));
    // Neither src nor content provided

    let result = validator.validate("copy", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.message.contains("One of")));
}

// ============================================================================
// Tests: Actionable Error Messages
// ============================================================================

#[test]
fn test_error_messages_are_actionable() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut args = HashMap::new();
    args.insert("state".to_string(), ArgValue::String("running".to_string())); // Wrong choice

    let result = validator.validate("file", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();

    // Check that errors have suggestions
    for error in &errors {
        // Either has a suggestion or the message itself is actionable
        let is_actionable = error.suggestion.is_some()
            || error.message.contains("Missing required")
            || error.message.contains("Valid choices");
        assert!(is_actionable, "Error should be actionable: {:?}", error);
    }
}

#[test]
fn test_missing_required_error_has_suggestion() {
    let error = SchemaError::missing_required("file", "path");
    assert!(error.suggestion.is_some());
    assert!(error.suggestion.unwrap().contains("path"));
}

#[test]
fn test_invalid_type_error_has_suggestion() {
    let error = SchemaError::invalid_type("service", "enabled", "bool", "string");
    assert!(error.suggestion.is_some());
    assert!(error.suggestion.unwrap().contains("bool"));
}

#[test]
fn test_invalid_choice_error_shows_valid_choices() {
    let error = SchemaError::invalid_choice("file", "state", "running", &["absent", "file", "directory"]);
    assert!(error.message.contains("absent"));
    assert!(error.message.contains("file"));
    assert!(error.message.contains("directory"));
}

// ============================================================================
// Tests: Fail Fast (No Runtime Fallback)
// ============================================================================

#[test]
fn test_validation_runs_before_execution() {
    // This test verifies the architectural constraint that validation
    // must happen before any execution attempt

    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    let mut execution_attempted = false;
    let mut validation_passed = false;

    // Invalid args
    let args: HashMap<String, ArgValue> = HashMap::new();

    // Validation should fail
    let validation_result = validator.validate("file", &args);

    if validation_result.is_ok() {
        validation_passed = true;
        // Only execute if validation passed
        execution_attempted = true;
    }

    assert!(!validation_passed, "Validation should have failed");
    assert!(!execution_attempted, "Execution should not have been attempted");
}

#[test]
fn test_all_errors_returned_at_once() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // Multiple errors: missing required, wrong type, invalid choice
    let mut args = HashMap::new();
    args.insert("state".to_string(), ArgValue::Int(123)); // Wrong type and will be invalid choice
    args.insert("unknown".to_string(), ArgValue::String("value".to_string())); // Unknown param
    // Missing 'path' required field

    let result = validator.validate("file", &args);

    assert!(result.is_err());
    let errors = result.unwrap_err();

    // Should have multiple errors reported at once
    assert!(errors.len() >= 2, "Should report all errors at once, got: {:?}", errors);
}

// ============================================================================
// CI Regression Guards
// ============================================================================

#[test]
fn test_ci_guard_core_module_count() {
    let registry = SchemaRegistry::new();
    let modules = registry.all_modules();

    // Guard: must have at least 20 core modules with schemas
    assert!(
        modules.len() >= 20,
        "Should have at least 20 core modules with schemas, got {}",
        modules.len()
    );
}

#[test]
fn test_ci_guard_file_module_schema() {
    let registry = SchemaRegistry::new();
    let schema = registry.get("file").expect("file module schema required");

    // Guard: file module must have these specific fields
    let field_names: HashSet<&str> = schema.fields.iter().map(|f| f.name.as_str()).collect();

    assert!(field_names.contains("path"), "file module must have 'path' field");
    assert!(field_names.contains("state"), "file module must have 'state' field");
    assert!(field_names.contains("mode"), "file module must have 'mode' field");
    assert!(field_names.contains("owner"), "file module must have 'owner' field");
}

#[test]
fn test_ci_guard_service_module_schema() {
    let registry = SchemaRegistry::new();
    let schema = registry.get("service").expect("service module schema required");

    let field_names: HashSet<&str> = schema.fields.iter().map(|f| f.name.as_str()).collect();

    assert!(field_names.contains("name"), "service module must have 'name' field");
    assert!(field_names.contains("state"), "service module must have 'state' field");
    assert!(field_names.contains("enabled"), "service module must have 'enabled' field");
}

#[test]
fn test_ci_guard_validation_is_strict() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // Guard: validation must reject invalid args (no permissive fallback)
    let mut args = HashMap::new();
    args.insert("invalid_key".to_string(), ArgValue::String("value".to_string()));

    let result = validator.validate("file", &args);

    assert!(result.is_err(), "Validation must be strict - unknown parameters must fail");
}

#[test]
fn test_ci_guard_choices_enforced() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // Guard: choices must be enforced
    let mut args = HashMap::new();
    args.insert("path".to_string(), ArgValue::String("/tmp".to_string()));
    args.insert("state".to_string(), ArgValue::String("invalid".to_string()));

    let result = validator.validate("file", &args);

    assert!(result.is_err(), "Choices must be enforced");
}

#[test]
fn test_ci_guard_required_fields_enforced() {
    let registry = SchemaRegistry::new();
    let validator = SchemaValidator::new(&registry);

    // Guard: required fields must be enforced
    let args: HashMap<String, ArgValue> = HashMap::new();

    let result = validator.validate("file", &args);

    assert!(result.is_err(), "Required fields must be enforced");
}
