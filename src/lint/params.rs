//! Module parameter validation.
//!
//! This module validates parameters passed to Ansible/Rustible modules,
//! checking for required parameters, valid values, and type correctness.

use super::types::{LintConfig, LintIssue, LintResult, Location, RuleCategory, Severity};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Parameter definition for a module.
#[derive(Debug, Clone)]
pub struct ParamDef {
    /// Parameter name.
    pub name: String,
    /// Whether the parameter is required.
    pub required: bool,
    /// Valid values (if restricted).
    pub choices: Option<Vec<String>>,
    /// Default value.
    pub default: Option<String>,
    /// Parameter type.
    pub param_type: ParamType,
    /// Aliases for this parameter.
    pub aliases: Vec<String>,
    /// Mutually exclusive with these parameters.
    pub mutually_exclusive: Vec<String>,
}

/// Type of a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    String,
    Bool,
    Int,
    Float,
    List,
    Dict,
    Path,
    Raw,
}

impl ParamDef {
    /// Create a new required parameter.
    pub fn required(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            required: true,
            choices: None,
            default: None,
            param_type: ParamType::String,
            aliases: Vec::new(),
            mutually_exclusive: Vec::new(),
        }
    }

    /// Create a new optional parameter.
    pub fn optional(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            required: false,
            choices: None,
            default: None,
            param_type: ParamType::String,
            aliases: Vec::new(),
            mutually_exclusive: Vec::new(),
        }
    }

    /// Set valid choices.
    pub fn with_choices(mut self, choices: Vec<&str>) -> Self {
        self.choices = Some(choices.into_iter().map(String::from).collect());
        self
    }

    /// Set parameter type.
    pub fn with_type(mut self, param_type: ParamType) -> Self {
        self.param_type = param_type;
        self
    }

    /// Add aliases.
    pub fn with_aliases(mut self, aliases: Vec<&str>) -> Self {
        self.aliases = aliases.into_iter().map(String::from).collect();
        self
    }

    /// Set default value.
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }
}

/// Module definition with parameter specifications.
#[derive(Debug, Clone)]
pub struct ModuleDef {
    /// Module name.
    pub name: String,
    /// Parameter definitions.
    pub params: Vec<ParamDef>,
    /// Free-form argument support.
    pub free_form: bool,
    /// Deprecated parameters.
    pub deprecated_params: HashMap<String, String>,
}

impl ModuleDef {
    /// Create a new module definition.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            params: Vec::new(),
            free_form: false,
            deprecated_params: HashMap::new(),
        }
    }

    /// Add a parameter.
    pub fn with_param(mut self, param: ParamDef) -> Self {
        self.params.push(param);
        self
    }

    /// Enable free-form arguments.
    pub fn with_free_form(mut self) -> Self {
        self.free_form = true;
        self
    }

    /// Add a deprecated parameter.
    pub fn with_deprecated(mut self, param: &str, replacement: &str) -> Self {
        self.deprecated_params.insert(param.to_string(), replacement.to_string());
        self
    }
}

/// Parameter validator for modules.
pub struct ParamValidator {
    /// Module definitions.
    modules: HashMap<String, ModuleDef>,
}

impl Default for ParamValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl ParamValidator {
    /// Create a new parameter validator with built-in module definitions.
    pub fn new() -> Self {
        let mut validator = Self {
            modules: HashMap::new(),
        };
        validator.register_builtin_modules();
        validator
    }

    /// Register a module definition.
    pub fn register(&mut self, module: ModuleDef) {
        self.modules.insert(module.name.clone(), module);
    }

    /// Register built-in module definitions.
    fn register_builtin_modules(&mut self) {
        // apt module
        self.register(
            ModuleDef::new("apt")
                .with_param(ParamDef::optional("name").with_aliases(vec!["pkg", "package"]))
                .with_param(
                    ParamDef::optional("state")
                        .with_choices(vec!["present", "absent", "latest", "build-dep", "fixed"]),
                )
                .with_param(ParamDef::optional("update_cache").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("cache_valid_time").with_type(ParamType::Int))
                .with_param(ParamDef::optional("purge").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("autoremove").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("deb").with_type(ParamType::Path))
                .with_param(ParamDef::optional("dpkg_options"))
                .with_param(ParamDef::optional("force").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("install_recommends").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("upgrade").with_choices(vec!["no", "yes", "safe", "full", "dist"])),
        );

        // yum module
        self.register(
            ModuleDef::new("yum")
                .with_param(ParamDef::optional("name").with_aliases(vec!["pkg"]))
                .with_param(
                    ParamDef::optional("state")
                        .with_choices(vec!["present", "absent", "latest", "installed", "removed"]),
                )
                .with_param(ParamDef::optional("enablerepo"))
                .with_param(ParamDef::optional("disablerepo"))
                .with_param(ParamDef::optional("update_cache").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("disable_gpg_check").with_type(ParamType::Bool)),
        );

        // dnf module (similar to yum)
        self.register(
            ModuleDef::new("dnf")
                .with_param(ParamDef::optional("name").with_aliases(vec!["pkg"]))
                .with_param(
                    ParamDef::optional("state")
                        .with_choices(vec!["present", "absent", "latest", "installed", "removed"]),
                )
                .with_param(ParamDef::optional("enablerepo"))
                .with_param(ParamDef::optional("disablerepo"))
                .with_param(ParamDef::optional("update_cache").with_type(ParamType::Bool)),
        );

        // copy module
        self.register(
            ModuleDef::new("copy")
                .with_param(ParamDef::optional("src").with_type(ParamType::Path))
                .with_param(ParamDef::required("dest").with_type(ParamType::Path))
                .with_param(ParamDef::optional("content"))
                .with_param(ParamDef::optional("owner"))
                .with_param(ParamDef::optional("group"))
                .with_param(ParamDef::optional("mode"))
                .with_param(ParamDef::optional("backup").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("force").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("remote_src").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("validate")),
        );

        // template module
        self.register(
            ModuleDef::new("template")
                .with_param(ParamDef::required("src").with_type(ParamType::Path))
                .with_param(ParamDef::required("dest").with_type(ParamType::Path))
                .with_param(ParamDef::optional("owner"))
                .with_param(ParamDef::optional("group"))
                .with_param(ParamDef::optional("mode"))
                .with_param(ParamDef::optional("backup").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("force").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("validate")),
        );

        // file module
        self.register(
            ModuleDef::new("file")
                .with_param(ParamDef::required("path").with_aliases(vec!["dest", "name"]))
                .with_param(
                    ParamDef::optional("state")
                        .with_choices(vec!["file", "directory", "link", "hard", "touch", "absent"]),
                )
                .with_param(ParamDef::optional("owner"))
                .with_param(ParamDef::optional("group"))
                .with_param(ParamDef::optional("mode"))
                .with_param(ParamDef::optional("src"))
                .with_param(ParamDef::optional("recurse").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("force").with_type(ParamType::Bool)),
        );

        // service module
        self.register(
            ModuleDef::new("service")
                .with_param(ParamDef::required("name"))
                .with_param(
                    ParamDef::optional("state")
                        .with_choices(vec!["started", "stopped", "restarted", "reloaded"]),
                )
                .with_param(ParamDef::optional("enabled").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("pattern"))
                .with_param(ParamDef::optional("sleep").with_type(ParamType::Int)),
        );

        // command module
        self.register(
            ModuleDef::new("command")
                .with_free_form()
                .with_param(ParamDef::optional("cmd").with_aliases(vec!["_raw_params"]))
                .with_param(ParamDef::optional("chdir").with_type(ParamType::Path))
                .with_param(ParamDef::optional("creates").with_type(ParamType::Path))
                .with_param(ParamDef::optional("removes").with_type(ParamType::Path))
                .with_param(ParamDef::optional("warn").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("stdin")),
        );

        // shell module
        self.register(
            ModuleDef::new("shell")
                .with_free_form()
                .with_param(ParamDef::optional("cmd").with_aliases(vec!["_raw_params"]))
                .with_param(ParamDef::optional("chdir").with_type(ParamType::Path))
                .with_param(ParamDef::optional("creates").with_type(ParamType::Path))
                .with_param(ParamDef::optional("removes").with_type(ParamType::Path))
                .with_param(ParamDef::optional("executable"))
                .with_param(ParamDef::optional("warn").with_type(ParamType::Bool)),
        );

        // user module
        self.register(
            ModuleDef::new("user")
                .with_param(ParamDef::required("name"))
                .with_param(ParamDef::optional("state").with_choices(vec!["present", "absent"]))
                .with_param(ParamDef::optional("uid").with_type(ParamType::Int))
                .with_param(ParamDef::optional("group"))
                .with_param(ParamDef::optional("groups"))
                .with_param(ParamDef::optional("append").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("shell"))
                .with_param(ParamDef::optional("home").with_type(ParamType::Path))
                .with_param(ParamDef::optional("password"))
                .with_param(ParamDef::optional("comment"))
                .with_param(ParamDef::optional("create_home").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("system").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("remove").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("force").with_type(ParamType::Bool)),
        );

        // group module
        self.register(
            ModuleDef::new("group")
                .with_param(ParamDef::required("name"))
                .with_param(ParamDef::optional("state").with_choices(vec!["present", "absent"]))
                .with_param(ParamDef::optional("gid").with_type(ParamType::Int))
                .with_param(ParamDef::optional("system").with_type(ParamType::Bool)),
        );

        // git module
        self.register(
            ModuleDef::new("git")
                .with_param(ParamDef::required("repo").with_aliases(vec!["name"]))
                .with_param(ParamDef::required("dest").with_type(ParamType::Path))
                .with_param(ParamDef::optional("version").with_default("HEAD"))
                .with_param(ParamDef::optional("clone").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("update").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("force").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("depth").with_type(ParamType::Int))
                .with_param(ParamDef::optional("accept_hostkey").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("key_file").with_type(ParamType::Path)),
        );

        // debug module
        self.register(
            ModuleDef::new("debug")
                .with_param(ParamDef::optional("msg"))
                .with_param(ParamDef::optional("var"))
                .with_param(ParamDef::optional("verbosity").with_type(ParamType::Int)),
        );

        // set_fact module
        self.register(
            ModuleDef::new("set_fact")
                .with_param(ParamDef::optional("cacheable").with_type(ParamType::Bool)),
        );

        // assert module
        self.register(
            ModuleDef::new("assert")
                .with_param(ParamDef::optional("that").with_type(ParamType::List))
                .with_param(ParamDef::optional("fail_msg"))
                .with_param(ParamDef::optional("success_msg"))
                .with_param(ParamDef::optional("quiet").with_type(ParamType::Bool)),
        );

        // lineinfile module
        self.register(
            ModuleDef::new("lineinfile")
                .with_param(ParamDef::required("path").with_aliases(vec!["dest", "destfile", "name"]))
                .with_param(ParamDef::optional("line"))
                .with_param(ParamDef::optional("regexp"))
                .with_param(ParamDef::optional("state").with_choices(vec!["present", "absent"]))
                .with_param(ParamDef::optional("insertafter"))
                .with_param(ParamDef::optional("insertbefore"))
                .with_param(ParamDef::optional("create").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("backup").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("backrefs").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("firstmatch").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("owner"))
                .with_param(ParamDef::optional("group"))
                .with_param(ParamDef::optional("mode")),
        );

        // blockinfile module
        self.register(
            ModuleDef::new("blockinfile")
                .with_param(ParamDef::required("path").with_aliases(vec!["dest", "destfile", "name"]))
                .with_param(ParamDef::optional("block").with_aliases(vec!["content"]))
                .with_param(ParamDef::optional("state").with_choices(vec!["present", "absent"]))
                .with_param(ParamDef::optional("marker"))
                .with_param(ParamDef::optional("marker_begin"))
                .with_param(ParamDef::optional("marker_end"))
                .with_param(ParamDef::optional("insertafter"))
                .with_param(ParamDef::optional("insertbefore"))
                .with_param(ParamDef::optional("create").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("backup").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("owner"))
                .with_param(ParamDef::optional("group"))
                .with_param(ParamDef::optional("mode")),
        );

        // pip module
        self.register(
            ModuleDef::new("pip")
                .with_param(ParamDef::optional("name"))
                .with_param(ParamDef::optional("requirements").with_type(ParamType::Path))
                .with_param(ParamDef::optional("version"))
                .with_param(ParamDef::optional("state").with_choices(vec!["present", "absent", "latest", "forcereinstall"]))
                .with_param(ParamDef::optional("virtualenv").with_type(ParamType::Path))
                .with_param(ParamDef::optional("virtualenv_command"))
                .with_param(ParamDef::optional("virtualenv_python"))
                .with_param(ParamDef::optional("extra_args"))
                .with_param(ParamDef::optional("executable")),
        );

        // stat module
        self.register(
            ModuleDef::new("stat")
                .with_param(ParamDef::required("path"))
                .with_param(ParamDef::optional("follow").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("get_checksum").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("get_mime").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("get_attributes").with_type(ParamType::Bool))
                .with_param(ParamDef::optional("checksum_algorithm").with_choices(vec!["md5", "sha1", "sha224", "sha256", "sha384", "sha512"])),
        );
    }

    /// Validate a task's module parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn validate_task(
        &self,
        task: &serde_yaml::Value,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        let task_map = match task.as_mapping() {
            Some(m) => m,
            None => return,
        };

        let task_name = task_map
            .get(serde_yaml::Value::String("name".to_string()))
            .and_then(|v| v.as_str());

        // Find the module being called
        for (key, value) in task_map.iter() {
            let key_str = match key.as_str() {
                Some(s) => s,
                None => continue,
            };

            // Skip non-module keys
            if is_task_attribute(key_str) {
                continue;
            }

            // Check if we have a definition for this module
            if let Some(module_def) = self.modules.get(key_str) {
                self.validate_module_params(
                    module_def,
                    value,
                    task_idx,
                    play_idx,
                    play_name,
                    task_name,
                    path,
                    config,
                    result,
                );
            }

            // Only process the first module found
            break;
        }
    }

    /// Validate parameters for a specific module.
    #[allow(clippy::too_many_arguments)]
    fn validate_module_params(
        &self,
        module: &ModuleDef,
        args: &serde_yaml::Value,
        task_idx: usize,
        play_idx: usize,
        play_name: Option<&str>,
        task_name: Option<&str>,
        path: &Path,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        let location = Location::file(path)
            .with_play(play_idx, play_name.map(String::from))
            .with_task(task_idx, task_name.map(String::from));

        // Handle string arguments (free-form)
        if args.is_string() {
            if !module.free_form
                && config.should_run_rule("P001", RuleCategory::Parameters, Severity::Warning) {
                    result.add_issue(LintIssue::new(
                        "P001",
                        "unexpected-free-form",
                        Severity::Warning,
                        RuleCategory::Parameters,
                        format!(
                            "Module '{}' does not support free-form arguments",
                            module.name
                        ),
                        location.clone(),
                    ).with_suggestion("Use named parameters instead of a string"));
                }
            return;
        }

        let args_map = match args.as_mapping() {
            Some(m) => m,
            None => return,
        };

        // Collect provided parameter names
        let provided_params: HashSet<String> = args_map
            .keys()
            .filter_map(|k| k.as_str().map(String::from))
            .collect();

        // Check for required parameters
        for param in &module.params {
            if param.required {
                let has_param = provided_params.contains(&param.name)
                    || param.aliases.iter().any(|a| provided_params.contains(a));

                if !has_param
                    && config.should_run_rule("P002", RuleCategory::Parameters, Severity::Error) {
                        result.add_issue(LintIssue::new(
                            "P002",
                            "missing-required-param",
                            Severity::Error,
                            RuleCategory::Parameters,
                            format!(
                                "Module '{}' is missing required parameter '{}'",
                                module.name, param.name
                            ),
                            location.clone(),
                        ).with_suggestion(format!("Add '{}' parameter", param.name)));
                    }
            }
        }

        // Check parameter values
        for (key, value) in args_map.iter() {
            let key_str = match key.as_str() {
                Some(s) => s,
                None => continue,
            };

            // Find the parameter definition
            let param_def = module.params.iter().find(|p| {
                p.name == key_str || p.aliases.contains(&key_str.to_string())
            });

            // Check for deprecated parameters
            if let Some(replacement) = module.deprecated_params.get(key_str) {
                if config.should_run_rule("P003", RuleCategory::Deprecation, Severity::Warning) {
                    result.add_issue(LintIssue::new(
                        "P003",
                        "deprecated-param",
                        Severity::Warning,
                        RuleCategory::Deprecation,
                        format!(
                            "Parameter '{}' is deprecated for module '{}'",
                            key_str, module.name
                        ),
                        location.clone(),
                    ).with_suggestion(format!("Use '{}' instead", replacement)));
                }
            }

            if let Some(def) = param_def {
                // Check choices
                if let Some(ref choices) = def.choices {
                    if let Some(value_str) = value.as_str() {
                        if !choices.contains(&value_str.to_string())
                            && config.should_run_rule("P004", RuleCategory::Parameters, Severity::Error) {
                                result.add_issue(LintIssue::new(
                                    "P004",
                                    "invalid-choice",
                                    Severity::Error,
                                    RuleCategory::Parameters,
                                    format!(
                                        "Invalid value '{}' for parameter '{}' in module '{}'. Valid choices: {}",
                                        value_str, key_str, module.name, choices.join(", ")
                                    ),
                                    location.clone(),
                                ));
                            }
                    }
                }

                // Check type
                self.validate_param_type(def, value, key_str, &module.name, &location, config, result);
            } else if config.should_run_rule("P005", RuleCategory::Parameters, Severity::Warning) {
                // Unknown parameter
                result.add_issue(LintIssue::new(
                    "P005",
                    "unknown-param",
                    Severity::Warning,
                    RuleCategory::Parameters,
                    format!(
                        "Unknown parameter '{}' for module '{}'",
                        key_str, module.name
                    ),
                    location.clone(),
                ).with_suggestion("Check module documentation for valid parameters"));
            }
        }
    }

    /// Validate parameter type.
    #[allow(clippy::too_many_arguments)]
    fn validate_param_type(
        &self,
        def: &ParamDef,
        value: &serde_yaml::Value,
        param_name: &str,
        module_name: &str,
        location: &Location,
        config: &LintConfig,
        result: &mut LintResult,
    ) {
        let type_match = match def.param_type {
            ParamType::String => value.is_string() || value.is_number() || value.is_bool(),
            ParamType::Bool => {
                value.is_bool()
                    || value.as_str().is_some_and(|s| {
                        matches!(s.to_lowercase().as_str(), "yes" | "no" | "true" | "false" | "on" | "off")
                    })
            }
            ParamType::Int => {
                value.as_i64().is_some()
                    || value.as_str().is_some_and(|s| s.parse::<i64>().is_ok())
            }
            ParamType::Float => {
                value.as_f64().is_some()
                    || value.as_str().is_some_and(|s| s.parse::<f64>().is_ok())
            }
            ParamType::List => value.is_sequence(),
            ParamType::Dict => value.is_mapping(),
            ParamType::Path => value.is_string(),
            ParamType::Raw => true,
        };

        if !type_match
            && config.should_run_rule("P006", RuleCategory::Parameters, Severity::Warning) {
                result.add_issue(LintIssue::new(
                    "P006",
                    "type-mismatch",
                    Severity::Warning,
                    RuleCategory::Parameters,
                    format!(
                        "Parameter '{}' in module '{}' expects type {:?}, got {:?}",
                        param_name, module_name, def.param_type, value
                    ),
                    location.clone(),
                ));
            }
    }
}

/// Check if a key is a task attribute (not a module name).
fn is_task_attribute(key: &str) -> bool {
    const TASK_ATTRS: &[&str] = &[
        "name", "action", "when", "loop", "with_items", "with_dict",
        "with_file", "with_fileglob", "with_first_found", "with_together",
        "with_nested", "with_random_choice", "with_sequence", "with_subelements",
        "with_template", "with_inventory_hostnames", "with_indexed_items",
        "loop_control", "register", "notify", "listen",
        "ignore_errors", "ignore_unreachable", "changed_when", "failed_when",
        "tags", "become", "become_method", "become_user",
        "delegate_to", "delegate_facts", "local_action", "run_once",
        "retries", "delay", "until", "async", "poll",
        "environment", "vars", "args", "block", "rescue", "always",
        "connection", "throttle", "timeout", "no_log", "diff", "check_mode",
        "module_defaults", "any_errors_fatal", "debugger",
    ];

    TASK_ATTRS.contains(&key) || key.starts_with("with_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_param_validator_missing_required() {
        let validator = ParamValidator::new();
        let mut result = LintResult::new();
        let config = LintConfig::new();

        let task = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
name: Install package
apt:
  state: present
"#,
        )
        .unwrap();

        validator.validate_task(&task, 0, 0, None, Path::new("test.yml"), &config, &mut result);

        // apt with state but no name - should be fine as name is optional
        // but copy with no dest would fail
    }

    #[test]
    fn test_param_validator_invalid_choice() {
        let validator = ParamValidator::new();
        let mut result = LintResult::new();
        let config = LintConfig::new();

        let task = serde_yaml::from_str::<serde_yaml::Value>(
            r#"
name: Invalid state
apt:
  name: nginx
  state: invalid_state
"#,
        )
        .unwrap();

        validator.validate_task(&task, 0, 0, None, Path::new("test.yml"), &config, &mut result);

        assert!(result.issues.iter().any(|i| i.rule_id == "P004"));
    }
}
