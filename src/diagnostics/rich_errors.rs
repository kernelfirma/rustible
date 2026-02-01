use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RichDiagnostic {
    pub message: String,
}

impl RichDiagnostic {
    pub fn error(msg: &str, _file: &str, _span: Span) -> Self {
        Self { message: msg.to_string() }
    }
    pub fn warning(msg: &str, _file: &str, _span: Span) -> Self {
        Self { message: msg.to_string() }
    }
    pub fn with_code(self, _code: &str) -> Self { self }
    pub fn with_label(self, _label: &str) -> Self { self }
    pub fn with_secondary_label(self, _span: Span, _label: &str) -> Self { self }
    pub fn with_note(self, _note: &str) -> Self { self }
    pub fn with_help(self, _help: &str) -> Self { self }
    pub fn eprint_with_source(&self, _source: &str) {}
    pub fn render_with_source(&self, _source: Option<&str>) -> String {
        self.message.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn from_line_col(_source: &str, _line: usize, _col: usize, _len: usize) -> Self {
        Self { start: 0, end: 0 }
    }
}

pub fn connection_error(_host: &str, _port: u16, error: &str) -> RichDiagnostic {
    RichDiagnostic { message: error.to_string() }
}
pub fn invalid_module_args_error(_module: &str, error: &str, _span: Span) -> RichDiagnostic {
    RichDiagnostic { message: error.to_string() }
}
pub fn missing_required_arg_error(_module: &str, arg: &str, _span: Span) -> RichDiagnostic {
    RichDiagnostic { message: format!("Missing arg: {}", arg) }
}
pub fn module_not_found_error(_file: &str, _source: &str, _line: usize, _col: usize, name: &str, _candidates: &[&str]) -> RichDiagnostic {
    RichDiagnostic { message: format!("Module not found: {}", name) }
}
pub fn template_syntax_error(_file: &str, _source: &str, _line: usize, _col: usize, error: &str) -> RichDiagnostic {
    RichDiagnostic { message: error.to_string() }
}
pub fn undefined_variable_error(_file: &str, _source: &str, _line: usize, _col: usize, var: &str, _candidates: &[&str]) -> RichDiagnostic {
    RichDiagnostic { message: format!("Undefined variable: {}", var) }
}
pub fn yaml_syntax_error<P: AsRef<Path>>(_file: P, _source: &str, _line: usize, _col: usize, error: &str) -> RichDiagnostic {
    RichDiagnostic { message: error.to_string() }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiagnosticSeverity { Error, Warning }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCodeInfo;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCodeRegistry;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelatedInfo;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion;
