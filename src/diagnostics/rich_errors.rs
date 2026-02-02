use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichDiagnostic {
    pub message: String,
}

impl RichDiagnostic {
    pub fn render_with_source(&self, _source: Option<&str>) -> String {
        self.message.clone()
    }
}

pub fn yaml_syntax_error(_path: PathBuf, _content: &str, _line: usize, _col: usize, msg: &str) -> RichDiagnostic {
    RichDiagnostic { message: msg.to_string() }
}

pub fn template_syntax_error(_file: &str, _source: &str, _line: usize, _col: usize, msg: &str) -> RichDiagnostic {
    RichDiagnostic { message: msg.to_string() }
}

// Add other stubs if needed based on imports in mod.rs
// connection_error, invalid_module_args_error, missing_required_arg_error,
// module_not_found_error, undefined_variable_error
// DiagnosticSeverity, ErrorCodeInfo, ErrorCodeRegistry, RelatedInfo, Span, Suggestion

pub fn connection_error(_host: &str, msg: &str) -> RichDiagnostic {
    RichDiagnostic { message: msg.to_string() }
}

pub fn invalid_module_args_error(_module: &str, msg: &str) -> RichDiagnostic {
    RichDiagnostic { message: msg.to_string() }
}

pub fn missing_required_arg_error(_module: &str, _arg: &str) -> RichDiagnostic {
    RichDiagnostic { message: format!("Missing required argument: {}", _arg) }
}

pub fn module_not_found_error(_module: &str) -> RichDiagnostic {
    RichDiagnostic { message: format!("Module not found: {}", _module) }
}

pub fn undefined_variable_error(_var: &str) -> RichDiagnostic {
    RichDiagnostic { message: format!("Undefined variable: {}", _var) }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCodeInfo {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCodeRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedInfo {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    pub message: String,
}
