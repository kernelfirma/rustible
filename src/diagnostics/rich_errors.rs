//! Rich diagnostics and error reporting helpers.
//!
//! This module provides structured diagnostics with spans, labels, hints,
//! and optional auto-fix suggestions. Rendering uses `ariadne` for
//! source-aware error output.

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

use ariadne::{Color, Label, Report, ReportKind, Source};

/// Severity level for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Note,
    Help,
}

/// Source span (byte offsets + optional line/column information).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

impl Span {
    /// Create a span from byte offsets. Line/column are unknown.
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
            line: 0,
            column: 0,
        }
    }

    /// Create a span from line/column (1-based) with a given length.
    pub fn from_line_col(source: &str, line: usize, column: usize, len: usize) -> Self {
        let (start, line, column) = offset_for_line_col(source, line, column)
            .unwrap_or((0, 0, 0));
        let end = start.saturating_add(len.max(1));
        Self {
            start,
            end,
            line,
            column,
        }
    }

    /// Range for ariadne labels.
    pub fn range(&self) -> Range<usize> {
        let end = if self.end > self.start { self.end } else { self.start + 1 };
        self.start..end
    }

    /// Whether this span has line/column information.
    pub fn has_line_col(&self) -> bool {
        self.line > 0 && self.column > 0
    }
}

fn offset_for_line_col(source: &str, line: usize, column: usize) -> Option<(usize, usize, usize)> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut offset = 0usize;
    for (idx, raw_line) in source.lines().enumerate() {
        let current_line = idx + 1;
        if current_line == line {
            let mut col_offset = 0usize;
            for (cidx, _) in raw_line.char_indices() {
                if col_offset + 1 == column {
                    return Some((offset + cidx, line, column));
                }
                col_offset += 1;
            }
            return Some((offset + raw_line.len(), line, column));
        }
        offset += raw_line.len() + 1; // +1 for '\n'
    }
    None
}

/// Auto-fix suggestion with optional replacement.
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub message: String,
    pub span: Option<Span>,
    pub replacement: Option<String>,
}

impl Suggestion {
    pub fn new(
        message: impl Into<String>,
        span: Option<Span>,
        replacement: Option<String>,
    ) -> Self {
        Self {
            message: message.into(),
            span,
            replacement,
        }
    }

    fn patch_snippet(&self, source: &str) -> Option<String> {
        let span = self.span?;
        let replacement = self.replacement.as_ref()?;

        // Locate the line containing the span start.
        let mut offset = 0usize;
        for (line_idx, raw_line) in source.lines().enumerate() {
            let line_start = offset;
            let line_end = offset + raw_line.len();
            if span.start >= line_start && span.start <= line_end {
                let rel_start = span.start.saturating_sub(line_start);
                let rel_end = span.end.saturating_sub(line_start).min(raw_line.len());
                let mut new_line = raw_line.to_string();
                if rel_start <= new_line.len() && rel_end <= new_line.len() && rel_start <= rel_end
                {
                    new_line.replace_range(rel_start..rel_end, replacement);
                } else {
                    return None;
                }
                let header = format!("@@ line {} @@", line_idx + 1);
                return Some(format!("{}\n- {}\n+ {}", header, raw_line, new_line));
            }
            offset += raw_line.len() + 1;
        }
        None
    }
}

/// Additional related info (secondary spans).
#[derive(Debug, Clone)]
pub struct RelatedInfo {
    pub message: String,
    pub file: String,
    pub span: Span,
}

impl RelatedInfo {
    pub fn new(message: impl Into<String>, file: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            file: file.into(),
            span,
        }
    }
}

/// Rich diagnostic with context and hints.
#[derive(Debug, Clone)]
pub struct RichDiagnostic {
    pub message: String,
    pub code: Option<String>,
    pub severity: DiagnosticSeverity,
    pub file: String,
    pub span: Span,
    pub label: Option<String>,
    pub help: Option<String>,
    pub notes: Vec<String>,
    pub related: Vec<RelatedInfo>,
    pub suggestions: Vec<Suggestion>,
}

impl RichDiagnostic {
    pub fn error(message: impl Into<String>, file: impl AsRef<Path>, span: Span) -> Self {
        Self::new(DiagnosticSeverity::Error, message, file, span)
    }

    pub fn warning(message: impl Into<String>, file: impl AsRef<Path>, span: Span) -> Self {
        Self::new(DiagnosticSeverity::Warning, message, file, span)
    }

    pub fn note(message: impl Into<String>, file: impl AsRef<Path>, span: Span) -> Self {
        Self::new(DiagnosticSeverity::Note, message, file, span)
    }

    pub fn help(message: impl Into<String>, file: impl AsRef<Path>, span: Span) -> Self {
        Self::new(DiagnosticSeverity::Help, message, file, span)
    }

    fn new(
        severity: DiagnosticSeverity,
        message: impl Into<String>,
        file: impl AsRef<Path>,
        span: Span,
    ) -> Self {
        Self {
            message: message.into(),
            code: None,
            severity,
            file: file.as_ref().to_string_lossy().to_string(),
            span,
            label: None,
            help: None,
            notes: Vec::new(),
            related: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_related(mut self, related: RelatedInfo) -> Self {
        self.related.push(related);
        self
    }

    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }

    pub fn render(&self) -> String {
        self.render_with_source(None)
    }

    pub fn render_with_source(&self, source_text: Option<&str>) -> String {
        if let Some(source) = source_text {
            let kind = match self.severity {
                DiagnosticSeverity::Error => ReportKind::Error,
                DiagnosticSeverity::Warning => ReportKind::Warning,
                DiagnosticSeverity::Note | DiagnosticSeverity::Help => ReportKind::Advice,
            };

            let color = match self.severity {
                DiagnosticSeverity::Error => Color::Red,
                DiagnosticSeverity::Warning => Color::Yellow,
                DiagnosticSeverity::Note => Color::Blue,
                DiagnosticSeverity::Help => Color::Green,
            };

            let span = (self.file.clone(), self.span.range());
            let mut report = Report::build(kind, span).with_message(self.message.clone());

            if let Some(code) = &self.code {
                report = report.with_code(code.clone());
            }

            if let Some(label) = &self.label {
                report = report.with_label(
                    Label::new((self.file.clone(), self.span.range()))
                        .with_message(label.clone())
                        .with_color(color),
                );
            }

            for note in &self.notes {
                report = report.with_note(note.clone());
            }

            if let Some(help) = &self.help {
                report = report.with_help(help.clone());
            }

            let mut buffer = Vec::new();
            if report
                .finish()
                .write((self.file.clone(), Source::from(source)), &mut buffer)
                .is_ok()
            {
                let mut output = String::from_utf8_lossy(&buffer).to_string();
                self.append_suggestions(&mut output, Some(source));
                return output;
            }
        }

        // Fallback: no source available
        let mut output = String::new();
        if let Some(code) = &self.code {
            output.push_str(&format!("{} ", code));
        }
        output.push_str(&format!("{}: {}", severity_label(self.severity), self.message));
        if self.span.has_line_col() {
            output.push_str(&format!("\n --> {}:{}:{}", self.file, self.span.line, self.span.column));
        } else {
            output.push_str(&format!("\n --> {}", self.file));
        }
        if let Some(label) = &self.label {
            output.push_str(&format!("\n = note: {}", label));
        }
        if let Some(help) = &self.help {
            output.push_str(&format!("\n = help: {}", help));
        }
        for note in &self.notes {
            output.push_str(&format!("\n = note: {}", note));
        }
        self.append_suggestions(&mut output, None);
        output
    }

    fn append_suggestions(&self, output: &mut String, source: Option<&str>) {
        if self.suggestions.is_empty() {
            return;
        }

        output.push_str("\n\nSuggestions:");
        for (idx, suggestion) in self.suggestions.iter().enumerate() {
            output.push_str(&format!("\n  {}. {}", idx + 1, suggestion.message));
            if let Some(src) = source {
                if let Some(patch) = suggestion.patch_snippet(src) {
                    output.push_str("\n\n");
                    output.push_str(&patch);
                }
            }
        }
    }
}

fn severity_label(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Error => "error",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Note => "note",
        DiagnosticSeverity::Help => "help",
    }
}

/// Detailed information about a diagnostic error code.
#[derive(Debug, Clone)]
pub struct ErrorCodeInfo {
    pub code: String,
    pub title: String,
    pub explanation: String,
    pub causes: Vec<String>,
    pub fixes: Vec<String>,
}

/// Registry of known error codes.
pub struct ErrorCodeRegistry {
    codes: HashMap<String, ErrorCodeInfo>,
}

impl ErrorCodeRegistry {
    pub fn new() -> Self {
        let mut codes = HashMap::new();

        insert_code(
            &mut codes,
            "E0001",
            "YAML syntax error",
            "The playbook could not be parsed as valid YAML.",
            vec![
                "Incorrect indentation or missing ':'".to_string(),
                "Unbalanced brackets or quotes".to_string(),
            ],
            vec![
                "Check indentation and YAML structure".to_string(),
                "Validate with a YAML linter".to_string(),
            ],
        );

        insert_code(
            &mut codes,
            "E0002",
            "Template syntax error",
            "A Jinja/Templating expression contains invalid syntax.",
            vec![
                "Missing closing brace".to_string(),
                "Invalid filter or function usage".to_string(),
            ],
            vec![
                "Fix the template expression".to_string(),
                "Check filter names and arguments".to_string(),
            ],
        );

        insert_code(
            &mut codes,
            "E0003",
            "Undefined variable",
            "A variable was referenced but never defined in the playbook context.",
            vec![
                "Typo in variable name".to_string(),
                "Variable defined in a different scope".to_string(),
            ],
            vec![
                "Define the variable in vars or inventory".to_string(),
                "Fix the variable spelling".to_string(),
            ],
        );

        insert_code(
            &mut codes,
            "E0004",
            "Module not found",
            "The requested module is not available in native Rustible modules or Ansible fallback.",
            vec![
                "Module name typo".to_string(),
                "Missing collection/module installation".to_string(),
            ],
            vec![
                "Check the module name".to_string(),
                "Install the required Ansible collection".to_string(),
            ],
        );

        insert_code(
            &mut codes,
            "E0010",
            "Tabs in YAML",
            "YAML does not allow tab characters for indentation.",
            vec!["Tabs used for indentation".to_string()],
            vec!["Replace tabs with spaces".to_string()],
        );

        insert_code(
            &mut codes,
            "E0020",
            "Missing required module argument",
            "A required module argument is missing.",
            vec!["Parameter omitted".to_string()],
            vec!["Add the missing parameter".to_string()],
        );

        insert_code(
            &mut codes,
            "E0030",
            "Invalid module argument",
            "A module argument has an invalid type or value.",
            vec!["Wrong type".to_string(), "Unsupported value".to_string()],
            vec!["Fix the argument type/value".to_string()],
        );

        Self { codes }
    }

    pub fn get(&self, code: &str) -> Option<&ErrorCodeInfo> {
        self.codes.get(code)
    }
}

fn insert_code(
    map: &mut HashMap<String, ErrorCodeInfo>,
    code: &str,
    title: &str,
    explanation: &str,
    causes: Vec<String>,
    fixes: Vec<String>,
) {
    map.insert(
        code.to_string(),
        ErrorCodeInfo {
            code: code.to_string(),
            title: title.to_string(),
            explanation: explanation.to_string(),
            causes,
            fixes,
        },
    );
}

// ---------------------------------------------------------------------------
// Convenience builders for common diagnostics
// ---------------------------------------------------------------------------

/// YAML syntax error with source span and hint.
pub fn yaml_syntax_error(
    path: impl AsRef<Path>,
    source: &str,
    line: usize,
    column: usize,
    message: &str,
) -> RichDiagnostic {
    RichDiagnostic::error(
        format!("YAML syntax error: {}", message),
        path.as_ref(),
        Span::from_line_col(source, line, column, 1),
    )
    .with_code("E0001")
    .with_label("invalid YAML syntax")
    .with_help("Check indentation, colons, and list markers")
}

/// Template syntax error with source span and hint.
pub fn template_syntax_error(
    path: impl AsRef<Path>,
    source: &str,
    line: usize,
    column: usize,
    message: &str,
) -> RichDiagnostic {
    RichDiagnostic::error(
        format!("Template syntax error: {}", message),
        path.as_ref(),
        Span::from_line_col(source, line, column, 1),
    )
    .with_code("E0002")
    .with_label("invalid template syntax")
    .with_help("Check {{ }} delimiters and filter names")
}

/// Undefined variable error with suggestions.
pub fn undefined_variable_error(
    path: impl AsRef<Path>,
    source: &str,
    line: usize,
    column: usize,
    var_name: &str,
    candidates: &[&str],
) -> RichDiagnostic {
    let mut diag = RichDiagnostic::error(
        format!("Undefined variable '{}': not found in scope", var_name),
        path.as_ref(),
        Span::from_line_col(source, line, column, var_name.len()),
    )
    .with_code("E0003")
    .with_label("undefined variable")
    .with_help("Define the variable in vars, inventory, or extra-vars");

    let suggestions = suggest_similar(var_name, candidates);
    if let Some(first) = suggestions.first() {
        diag = diag.with_note(format!("Did you mean '{}' ?", first));
        diag = diag.with_suggestion(Suggestion::new(
            format!("Replace '{}' with '{}'", var_name, first),
            Some(Span::from_line_col(source, line, column, var_name.len())),
            Some(first.to_string()),
        ));
    }

    diag
}

/// Module not found diagnostic.
pub fn module_not_found_error(
    path: impl AsRef<Path>,
    module: &str,
    span: Span,
) -> RichDiagnostic {
    RichDiagnostic::error(
        format!("Module '{}' not found", module),
        path.as_ref(),
        span,
    )
    .with_code("E0004")
    .with_label("unknown module")
    .with_help("Check module name or install required collection")
}

/// Invalid module argument diagnostic.
pub fn invalid_module_args_error(
    path: impl AsRef<Path>,
    module: &str,
    message: &str,
    span: Span,
) -> RichDiagnostic {
    RichDiagnostic::error(
        format!("Invalid arguments for '{}': {}", module, message),
        path.as_ref(),
        span,
    )
    .with_code("E0030")
    .with_label("invalid module arguments")
    .with_help("Check argument types and supported values")
}

/// Missing required argument diagnostic.
pub fn missing_required_arg_error(
    path: impl AsRef<Path>,
    module: &str,
    arg: &str,
    span: Span,
) -> RichDiagnostic {
    RichDiagnostic::error(
        format!("Missing required argument '{}' for module '{}'", arg, module),
        path.as_ref(),
        span,
    )
    .with_code("E0020")
    .with_label("missing required argument")
    .with_help("Add the required argument to the module call")
}

/// Connection error diagnostic.
pub fn connection_error(
    path: impl AsRef<Path>,
    host: &str,
    message: &str,
    span: Span,
) -> RichDiagnostic {
    RichDiagnostic::error(
        format!("Connection error for {}: {}", host, message),
        path.as_ref(),
        span,
    )
    .with_code("E0030")
    .with_label("connection failed")
    .with_help("Verify host reachability and credentials")
}

fn suggest_similar(target: &str, candidates: &[&str]) -> Vec<String> {
    let mut matches: Vec<String> = candidates
        .iter()
        .filter(|c| c.starts_with(target) || target.starts_with(*c) || c.contains(target))
        .map(|c| c.to_string())
        .collect();

    matches.sort();
    matches.truncate(3);
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_from_line_col() {
        let source = "line1\nline2\nline3";
        let span = Span::from_line_col(source, 2, 2, 3);
        assert!(span.start > 0);
        assert!(span.end > span.start);
    }

    #[test]
    fn test_render_without_source() {
        let diag = RichDiagnostic::error("test", "file.yml", Span::new(0, 0));
        let rendered = diag.render();
        assert!(rendered.contains("test"));
    }
}
