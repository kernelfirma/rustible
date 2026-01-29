//! Pre-execution validation for playbooks
//!
//! This module provides comprehensive validation that catches errors before
//! execution begins, including schema validation, module argument checking,
//! variable verification, and syntax validation.

use crate::diagnostics::RichDiagnostic;
use crate::error::{Error, Result};
use crate::parser::schema::SchemaValidator;
use crate::playbook::Playbook;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Validation configuration
#[derive(Debug, Clone, Default)]
pub struct ValidationConfig {
    /// Validate YAML syntax
    pub validate_syntax: bool,
    /// Validate module arguments against schema
    pub validate_module_args: bool,
    /// Check for undefined variables
    pub check_undefined_vars: bool,
    /// Validate handler references
    pub validate_handlers: bool,
    /// Check for deprecated modules
    pub check_deprecated: bool,
    /// Severity level for warnings
    pub warning_severity: WarningSeverity,
}

/// Severity level for validation warnings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSeverity {
    /// Treat warnings as errors
    Error,
    /// Show warnings but don't fail
    Warning,
    /// Ignore warnings
    Ignore,
}

impl Default for WarningSeverity {
    fn default() -> Self {
        Self::Warning
    }
}

impl ValidationConfig {
    /// Create a new validation config
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable all validation checks
    pub fn strict() -> Self {
        Self {
            validate_syntax: true,
            validate_module_args: true,
            check_undefined_vars: true,
            validate_handlers: true,
            check_deprecated: true,
            warning_severity: WarningSeverity::Error,
        }
    }

    /// Set syntax validation
    pub fn with_syntax_validation(mut self, enabled: bool) -> Self {
        self.validate_syntax = enabled;
        self
    }

    /// Set module argument validation
    pub fn with_module_args_validation(mut self, enabled: bool) -> Self {
        self.validate_module_args = enabled;
        self
    }

    /// Set undefined variable checking
    pub fn with_undefined_var_checking(mut self, enabled: bool) -> Self {
        self.check_undefined_vars = enabled;
        self
    }

    /// Set handler validation
    pub fn with_handler_validation(mut self, enabled: bool) -> Self {
        self.validate_handlers = enabled;
        self
    }

    /// Set deprecated module checking
    pub fn with_deprecated_checking(mut self, enabled: bool) -> Self {
        self.check_deprecated = enabled;
        self
    }

    /// Set warning severity
    pub fn with_warning_severity(mut self, severity: WarningSeverity) -> Self {
        self.warning_severity = severity;
        self
    }
}

/// Validation result with diagnostics
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether validation passed
    pub passed: bool,
    /// Errors found
    pub errors: Vec<RichDiagnostic>,
    /// Warnings found
    pub warnings: Vec<RichDiagnostic>,
    /// Informational messages
    pub info: Vec<String>,
}

impl ValidationResult {
    /// Create a new validation result
    pub fn new() -> Self {
        Self {
            passed: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
        }
    }

    /// Add an error
    pub fn add_error(&mut self, error: RichDiagnostic) {
        self.passed = false;
        self.errors.push(error);
    }

    /// Add a warning
    pub fn add_warning(&mut self, warning: RichDiagnostic) {
        self.warnings.push(warning);
    }

    /// Add an info message
    pub fn add_info(&mut self, message: String) {
        self.info.push(message);
    }

    /// Merge another validation result
    pub fn merge(&mut self, other: ValidationResult) {
        if !other.passed {
            self.passed = false;
        }
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
        self.info.extend(other.info);
    }

    /// Format the validation result
    pub fn format(&self) -> String {
        let mut output = String::new();

        if self.passed {
            output.push_str("✓ Validation passed");
        } else {
            output.push_str("✗ Validation failed");
        }

        if !self.errors.is_empty() {
            output.push_str(&format!("\n\nErrors ({}):", self.errors.len()));
            for error in &self.errors {
                output.push_str(&format!("\n\n{}", error.render()));
            }
        }

        if !self.warnings.is_empty() {
            output.push_str(&format!("\n\nWarnings ({}):", self.warnings.len()));
            for warning in &self.warnings {
                output.push_str(&format!("\n\n{}", warning.render()));
            }
        }

        if !self.info.is_empty() {
            output.push_str(&format!("\n\nInfo ({}):", self.info.len()));
            for info in &self.info {
                output.push_str(&format!("\n  - {}", info));
            }
        }

        output
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Pre-execution validator
pub struct Validator {
    config: ValidationConfig,
    schema_validator: SchemaValidator,
}

impl Validator {
    /// Create a new validator
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            config,
            schema_validator: SchemaValidator::new(),
        }
    }

    /// Create a validator with default configuration
    pub fn default() -> Self {
        Self::new(ValidationConfig::new())
    }

    /// Create a strict validator
    pub fn strict() -> Self {
        Self::new(ValidationConfig::strict())
    }

    /// Validate a playbook
    pub fn validate(&self, playbook_path: &Path, playbook: &Playbook) -> Result<ValidationResult> {
        let mut result = ValidationResult::new();

        // Read playbook source for diagnostics
        let source = std::fs::read_to_string(playbook_path).unwrap_or_else(|_| String::new());

        // Validate syntax
        if self.config.validate_syntax {
            result.merge(self.validate_syntax(playbook_path, &source));
        }

        // Validate module arguments
        if self.config.validate_module_args {
            result.merge(self.validate_module_args(playbook_path, &source, playbook));
        }

        // Check for undefined variables
        if self.config.check_undefined_vars {
            result.merge(self.check_undefined_vars(playbook_path, &source, playbook));
        }

        // Validate handlers
        if self.config.validate_handlers {
            result.merge(self.validate_handlers(playbook_path, &source, playbook));
        }

        // Check for deprecated modules
        if self.config.check_deprecated {
            result.merge(self.check_deprecated(playbook_path, &source, playbook));
        }

        // Convert warnings to errors if configured
        if self.config.warning_severity == WarningSeverity::Error && !self.warnings.is_empty() {
            result.errors.extend(result.warnings.clone());
            result.warnings.clear();
            result.passed = false;
        }

        Ok(result)
    }

    /// Validate YAML syntax
    fn validate_syntax(&self, playbook_path: &Path, source: &str) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Check for common YAML syntax issues
        for (line_num, line) in source.lines().enumerate() {
            let line_num = line_num + 1;

            // Check for tabs (YAML doesn't allow tabs)
            if line.contains('\t') {
                let diag = RichDiagnostic::error(
                    "tabs found in YAML - use spaces instead",
                    playbook_path,
                    crate::diagnostics::Span::from_line_col(source, line_num, 1, 1),
                )
                .with_code("E0010")
                .with_label("tab character found here")
                .with_help("replace tabs with 2 spaces");
                result.add_error(diag);
            }

            // Check for trailing whitespace
            if line.ends_with(' ') || line.ends_with('\t') {
                // Warning only
            }
        }

        result
    }

    /// Validate module arguments
    fn validate_module_args(
        &self,
        playbook_path: &Path,
        source: &str,
        playbook: &Playbook,
    ) -> ValidationResult {
        let mut result = ValidationResult::new();

        for (play_idx, play) in playbook.plays.iter().enumerate() {
            for (task_idx, task) in play.tasks.iter().enumerate() {
                if let Some(module_name) = task.get_module_name() {
                    // Validate module arguments against schema
                    if let Err(diag) = self.schema_validator.validate_task_args(
                        playbook_path,
                        source,
                        task,
                        module_name,
                    ) {
                        result.add_error(diag);
                    }
                }
            }
        }

        result
    }

    /// Check for undefined variables
    fn check_undefined_vars(
        &self,
        playbook_path: &Path,
        source: &str,
        playbook: &Playbook,
    ) -> ValidationResult {
        let mut result = ValidationResult::new();

        // Collect all defined variables
        let mut defined_vars = HashSet::new();
        for play in &playbook.plays {
            if let Some(vars) = &play.vars {
                for key in vars.keys() {
                    defined_vars.insert(key.clone());
                }
            }
        }

        // Collect all used variables
        let mut used_vars = HashSet::new();
        for (line_num, line) in source.lines().enumerate() {
            // Find {{ var }} patterns
            let mut chars = line.char_indices();
            while let Some((start, _)) = chars.find(|(_, c)| *c == '{') {
                if let Some((_, _)) = chars.next() {
                    if let Some((_, c)) = chars.next() {
                        if *c == '{' {
                            // Found opening {{, look for closing }}
                            let var_start = line_num + 1;
                            let var_content: String = chars
                                .by_ref()
                                .take_while(|(_, c)| *c != '}')
                                .map(|(_, c)| c)
                                .collect();

                            if !var_content.is_empty() {
                                // Extract variable name (simplified)
                                let var_name = var_content
                                    .trim()
                                    .split_whitespace()
                                    .next()
                                    .unwrap_or("")
                                    .to_string();

                                if !var_name.is_empty() && !var_name.contains('|') {
                                    used_vars.insert(var_name);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Check for undefined variables
        for var in &used_vars {
            if !defined_vars.contains(var) && !is_builtin_var(var) {
                let line_num = find_var_line(source, var);
                if let Some(line) = line_num {
                    let diag = crate::diagnostics::undefined_variable_error(
                        playbook_path,
                        source,
                        line,
                        source
                            .lines()
                            .nth(line - 1)
                            .unwrap_or("")
                            .find(var)
                            .unwrap_or(0)
                            + 1,
                        var,
                        &defined_vars.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                    );
                    result.add_error(diag);
                }
            }
        }

        result
    }

    /// Validate handler references
    fn validate_handlers(
        &self,
        playbook_path: &Path,
        source: &str,
        playbook: &Playbook,
    ) -> ValidationResult {
        let mut result = ValidationResult::new();

        for play in &playbook.plays {
            // Collect all defined handlers
            let mut defined_handlers = HashSet::new();
            for handler in &play.handlers {
                defined_handlers.insert(handler.name.clone());
            }

            // Check handler notifications in tasks
            for (task_idx, task) in play.tasks.iter().enumerate() {
                if let Some(notify) = task.get_notify() {
                    for handler_name in notify {
                        if !defined_handlers.contains(handler_name) {
                            let line_num = task_idx + 1; // Simplified
                            let diag = RichDiagnostic::error(
                                format!("handler '{}' not found", handler_name),
                                playbook_path,
                                crate::diagnostics::Span::new(0, 0),
                            )
                            .with_code("E0005")
                            .with_label("handler not defined")
                            .with_note(&format!(
                                "available handlers: {}",
                                defined_handlers
                                    .iter()
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                            result.add_error(diag);
                        }
                    }
                }
            }
        }

        result
    }

    /// Check for deprecated modules
    fn check_deprecated(
        &self,
        playbook_path: &Path,
        source: &str,
        playbook: &Playbook,
    ) -> ValidationResult {
        let mut result = ValidationResult::new();

        // List of deprecated modules
        let deprecated_modules = vec![
            ("apt_key", "Use 'apt_repository' instead"),
            (
                "docker_container",
                "Use 'community.docker.docker_container' instead",
            ),
            (
                "docker_image",
                "Use 'community.docker.docker_image' instead",
            ),
        ];

        for play in &playbook.plays {
            for (task_idx, task) in play.tasks.iter().enumerate() {
                if let Some(module_name) = task.get_module_name() {
                    for (dep_mod, suggestion) in &deprecated_modules {
                        if module_name == *dep_mod {
                            let line_num = task_idx + 1; // Simplified
                            let diag = RichDiagnostic::warning(
                                format!("module '{}' is deprecated", module_name),
                                playbook_path,
                                crate::diagnostics::Span::new(0, 0),
                            )
                            .with_code("W0001")
                            .with_label("deprecated module")
                            .with_help(suggestion);
                            result.add_warning(diag);
                        }
                    }
                }
            }
        }

        result
    }
}

impl Default for Validator {
    fn default() -> Self {
        Self::new(ValidationConfig::default())
    }
}

/// Check if a variable is a built-in Ansible variable
fn is_builtin_var(var: &str) -> bool {
    let builtins = [
        "ansible_hostname",
        "inventory_hostname",
        "hostvars",
        "groups",
        "group_names",
        "play_hosts",
        "ansible_version",
        "ansible_facts",
        "ansible_system",
        "ansible_os_family",
        "ansible_distribution",
        "ansible_user_id",
        "ansible_user_gid",
        "ansible_user_dir",
        "ansible_user_shell",
        "item", // loop variable
        "host", // loop variable
    ];
    builtins.contains(&var)
}

/// Find the line number where a variable is used
fn find_var_line(source: &str, var: &str) -> Option<usize> {
    for (line_num, line) in source.lines().enumerate() {
        if line.contains(&format!("{{{{{}}}}}", var)) || line.contains(&format!("{{ {} }}", var)) {
            return Some(line_num + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_config() {
        let config = ValidationConfig::strict();
        assert!(config.validate_syntax);
        assert!(config.validate_module_args);
    }

    #[test]
    fn test_validation_result() {
        let mut result = ValidationResult::new();
        assert!(result.passed);

        let diag = RichDiagnostic::error(
            "test error",
            "test.yml",
            crate::diagnostics::Span::new(0, 0),
        );
        result.add_error(diag);
        assert!(!result.passed);
    }

    #[test]
    fn test_builtin_vars() {
        assert!(is_builtin_var("ansible_hostname"));
        assert!(is_builtin_var("inventory_hostname"));
        assert!(!is_builtin_var("my_custom_var"));
    }
}
