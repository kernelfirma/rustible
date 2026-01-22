//! Rich error diagnostics with Rust-compiler-style output
//!
//! This module provides beautiful, actionable error messages that show:
//! - Exact source location with line/column highlighting
//! - Code snippets with context lines
//! - Helpful suggestions and "did you mean?" hints
//! - Related information from other files
//!
//! # Example Output
//!
//! ```text
//! error[E0042]: undefined variable 'wrong_var'
//!   --> playbook.yml:15:23
//!    |
//! 15 |       msg: "{{ wrong_var }}"
//!    |                 ^^^^^^^^^ not defined in this scope
//!    |
//!    = help: did you mean 'var1'?
//!    = note: available variables: var1, ansible_hostname, inventory_hostname
//! ```

use ariadne::{Cache, CharSet, Color, Config, Label, Report, ReportKind, Source};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::ops::Range;
use std::path::PathBuf;

/// A span representing a range in a source file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    /// Start byte offset
    pub start: usize,
    /// End byte offset
    pub end: usize,
}

impl Span {
    /// Create a new span
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Create a span from a line and column (1-indexed)
    pub fn from_line_col(source: &str, line: usize, col: usize, len: usize) -> Self {
        let mut offset = 0;
        for (i, line_content) in source.lines().enumerate() {
            if i + 1 == line {
                let start = offset + col.saturating_sub(1);
                let end = start + len;
                return Self::new(start, end);
            }
            offset += line_content.len() + 1; // +1 for newline
        }
        Self::new(0, 0)
    }

    /// Convert to a Range for ariadne
    pub fn to_range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// Severity level for diagnostics
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// Error - execution cannot continue
    Error,
    /// Warning - potential issue but execution can continue
    Warning,
    /// Note - informational message
    Note,
    /// Help - suggestion for fixing an issue
    Help,
}

impl DiagnosticSeverity {
    fn to_report_kind(self) -> ReportKind<'static> {
        match self {
            DiagnosticSeverity::Error => ReportKind::Error,
            DiagnosticSeverity::Warning => ReportKind::Warning,
            DiagnosticSeverity::Note => ReportKind::Advice,
            DiagnosticSeverity::Help => ReportKind::Advice,
        }
    }

    fn color(self) -> Color {
        match self {
            DiagnosticSeverity::Error => Color::Red,
            DiagnosticSeverity::Warning => Color::Yellow,
            DiagnosticSeverity::Note => Color::Cyan,
            DiagnosticSeverity::Help => Color::Green,
        }
    }
}

/// A suggestion for fixing an error
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// The suggestion text
    pub text: String,
    /// Optional replacement code
    pub replacement: Option<String>,
    /// Span where the replacement should be applied
    pub span: Option<Span>,
}

impl Suggestion {
    /// Create a new text-only suggestion
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            replacement: None,
            span: None,
        }
    }

    /// Create a suggestion with a replacement
    pub fn with_replacement(
        text: impl Into<String>,
        replacement: impl Into<String>,
        span: Span,
    ) -> Self {
        Self {
            text: text.into(),
            replacement: Some(replacement.into()),
            span: Some(span),
        }
    }
}

/// Related information for a diagnostic
#[derive(Debug, Clone)]
pub struct RelatedInfo {
    /// File path
    pub file: PathBuf,
    /// Span in the related file
    pub span: Span,
    /// Message explaining the relation
    pub message: String,
}

impl RelatedInfo {
    /// Create new related information
    pub fn new(file: impl Into<PathBuf>, span: Span, message: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            span,
            message: message.into(),
        }
    }
}

/// A rich diagnostic error with source code context
#[derive(Debug, Clone)]
pub struct RichDiagnostic {
    /// Error code (e.g., "E0042")
    pub code: Option<String>,
    /// Severity level
    pub severity: DiagnosticSeverity,
    /// Primary error message
    pub message: String,
    /// Source file path
    pub file: PathBuf,
    /// Primary span in the source
    pub span: Span,
    /// Label for the primary span
    pub label: Option<String>,
    /// Additional labels (secondary highlights)
    pub secondary_labels: Vec<(Span, String)>,
    /// Help/note messages
    pub notes: Vec<String>,
    /// Suggestions for fixing
    pub suggestions: Vec<Suggestion>,
    /// Related information from other locations
    pub related: Vec<RelatedInfo>,
}

impl RichDiagnostic {
    /// Create a new error diagnostic
    pub fn error(message: impl Into<String>, file: impl Into<PathBuf>, span: Span) -> Self {
        Self {
            code: None,
            severity: DiagnosticSeverity::Error,
            message: message.into(),
            file: file.into(),
            span,
            label: None,
            secondary_labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
            related: Vec::new(),
        }
    }

    /// Create a new warning diagnostic
    pub fn warning(message: impl Into<String>, file: impl Into<PathBuf>, span: Span) -> Self {
        Self {
            code: None,
            severity: DiagnosticSeverity::Warning,
            message: message.into(),
            file: file.into(),
            span,
            label: None,
            secondary_labels: Vec::new(),
            notes: Vec::new(),
            suggestions: Vec::new(),
            related: Vec::new(),
        }
    }

    /// Set the error code
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Set the primary label
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Add a secondary label
    pub fn with_secondary_label(mut self, span: Span, label: impl Into<String>) -> Self {
        self.secondary_labels.push((span, label.into()));
        self
    }

    /// Add a note
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Add a help message
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.notes.push(format!("help: {}", help.into()));
        self
    }

    /// Add a suggestion
    pub fn with_suggestion(mut self, suggestion: Suggestion) -> Self {
        self.suggestions.push(suggestion);
        self
    }

    /// Add related information
    pub fn with_related(mut self, related: RelatedInfo) -> Self {
        self.related.push(related);
        self
    }

    /// Render the diagnostic to a colored string
    pub fn render(&self) -> String {
        self.render_with_source(None)
    }

    /// Render with explicit source content (avoids file I/O)
    pub fn render_with_source(&self, source_override: Option<&str>) -> String {
        let source = source_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| std::fs::read_to_string(&self.file).unwrap_or_default());

        let file_id = self.file.display().to_string();
        let mut output = Vec::new();

        // Create the span tuple (file_id, range) for ariadne
        let primary_span = (file_id.clone(), self.span.to_range());

        let mut builder = Report::build(self.severity.to_report_kind(), primary_span.clone())
            .with_config(
                Config::default()
                    .with_char_set(CharSet::Unicode)
                    .with_tab_width(2),
            )
            .with_message(&self.message);

        // Add error code if present
        if let Some(code) = &self.code {
            builder = builder.with_code(code);
        }

        // Primary label
        let primary_label = Label::new(primary_span)
            .with_message(self.label.as_deref().unwrap_or("here"))
            .with_color(self.severity.color());
        builder = builder.with_label(primary_label);

        // Secondary labels
        for (span, msg) in &self.secondary_labels {
            let label = Label::new((file_id.clone(), span.to_range()))
                .with_message(msg)
                .with_color(Color::Blue);
            builder = builder.with_label(label);
        }

        // Notes and help messages
        for note in &self.notes {
            builder = builder.with_note(note);
        }

        // Suggestions
        for suggestion in &self.suggestions {
            if let Some(replacement) = &suggestion.replacement {
                builder = builder.with_help(format!("{}: `{}`", suggestion.text, replacement));
            } else {
                builder = builder.with_help(&suggestion.text);
            }
        }

        let report = builder.finish();

        // Create a simple cache with our source
        let mut cache = SimpleCache::new();
        cache.insert(file_id.clone(), source);

        report.write(cache, &mut output).ok();

        String::from_utf8(output).unwrap_or_else(|_| self.message.clone())
    }

    /// Render to stderr
    pub fn eprint(&self) {
        eprint!("{}", self.render());
    }

    /// Render to stderr with explicit source
    pub fn eprint_with_source(&self, source: &str) {
        eprint!("{}", self.render_with_source(Some(source)));
    }
}

/// Simple cache implementation for ariadne
struct SimpleCache {
    files: HashMap<String, Source<String>>,
}

impl SimpleCache {
    fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    fn insert(&mut self, name: String, content: String) {
        self.files.insert(name, Source::from(content));
    }
}

#[allow(refining_impl_trait)]
impl Cache<String> for SimpleCache {
    type Storage = String;

    fn fetch(
        &mut self,
        id: &String,
    ) -> Result<&Source<Self::Storage>, Box<dyn std::fmt::Debug + '_>> {
        self.files
            .get(id)
            .ok_or_else(|| Box::new(format!("File not found: {}", id)) as Box<dyn std::fmt::Debug>)
    }

    fn display<'a>(&self, id: &'a String) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(id.clone()))
    }
}

// ============================================================================
// Diagnostic Builders for Common Error Types
// ============================================================================

/// Builder for undefined variable errors
pub fn undefined_variable_error(
    file: impl Into<PathBuf>,
    source: &str,
    line: usize,
    col: usize,
    var_name: &str,
    available_vars: &[&str],
) -> RichDiagnostic {
    let span = Span::from_line_col(source, line, col, var_name.len());

    let mut diag = RichDiagnostic::error(
        format!("undefined variable '{}'", var_name),
        file,
        span,
    )
    .with_code("E0001")
    .with_label("not defined in this scope");

    // Find similar variable names for "did you mean?" suggestions
    if let Some(suggestion) = find_similar(var_name, available_vars) {
        diag = diag.with_help(format!("did you mean '{}'?", suggestion));
    }

    // Show available variables
    if !available_vars.is_empty() {
        let vars_list = available_vars.join(", ");
        diag = diag.with_note(format!("available variables: {}", vars_list));
    }

    diag
}

/// Builder for module not found errors
pub fn module_not_found_error(
    file: impl Into<PathBuf>,
    source: &str,
    line: usize,
    col: usize,
    module_name: &str,
    available_modules: &[&str],
) -> RichDiagnostic {
    let span = Span::from_line_col(source, line, col, module_name.len());

    let mut diag = RichDiagnostic::error(
        format!("module '{}' not found", module_name),
        file,
        span,
    )
    .with_code("E0002")
    .with_label("unknown module");

    // Find similar module names
    if let Some(suggestion) = find_similar(module_name, available_modules) {
        diag = diag.with_help(format!("did you mean '{}'?", suggestion));
    }

    diag
}

/// Builder for invalid module argument errors
pub fn invalid_module_args_error(
    file: impl Into<PathBuf>,
    source: &str,
    line: usize,
    col: usize,
    module_name: &str,
    arg_name: &str,
    expected: &str,
    actual: &str,
) -> RichDiagnostic {
    let span = Span::from_line_col(source, line, col, arg_name.len());

    RichDiagnostic::error(
        format!(
            "invalid argument '{}' for module '{}'",
            arg_name, module_name
        ),
        file,
        span,
    )
    .with_code("E0003")
    .with_label(format!("expected {}, found {}", expected, actual))
    .with_note(format!(
        "module '{}' requires '{}' to be of type {}",
        module_name, arg_name, expected
    ))
}

/// Builder for YAML syntax errors
pub fn yaml_syntax_error(
    file: impl Into<PathBuf>,
    source: &str,
    line: usize,
    col: usize,
    message: &str,
) -> RichDiagnostic {
    let span = Span::from_line_col(source, line, col, 1);

    RichDiagnostic::error(format!("syntax error: {}", message), file, span)
        .with_code("E0010")
        .with_label("invalid syntax here")
        .with_help("check YAML indentation (use 2 spaces, not tabs)")
}

/// Builder for template syntax errors
pub fn template_syntax_error(
    file: impl Into<PathBuf>,
    source: &str,
    line: usize,
    col: usize,
    message: &str,
) -> RichDiagnostic {
    let span = Span::from_line_col(source, line, col, 1);

    RichDiagnostic::error(format!("template error: {}", message), file, span)
        .with_code("E0020")
        .with_label("template syntax error")
        .with_help("Jinja2 expressions use {{ }} for values and {% %} for statements")
}

/// Builder for connection errors
pub fn connection_error(
    file: impl Into<PathBuf>,
    source: &str,
    line: usize,
    host: &str,
    message: &str,
) -> RichDiagnostic {
    // Find where the host is defined in the source
    let span = find_host_span(source, host, line);

    RichDiagnostic::error(
        format!("failed to connect to '{}'", host),
        file,
        span,
    )
    .with_code("E0030")
    .with_label(format!("connection failed: {}", message))
    .with_help("verify the host is reachable and SSH credentials are correct")
    .with_note("use `rustible ping` to test connectivity")
}

/// Builder for missing required argument errors
pub fn missing_required_arg_error(
    file: impl Into<PathBuf>,
    source: &str,
    line: usize,
    module_name: &str,
    missing_args: &[&str],
) -> RichDiagnostic {
    let span = Span::from_line_col(source, line, 1, module_name.len() + 1);

    let args_list = missing_args.join(", ");
    RichDiagnostic::error(
        format!(
            "missing required argument(s) for module '{}': {}",
            module_name, args_list
        ),
        file,
        span,
    )
    .with_code("E0004")
    .with_label("missing required arguments")
    .with_help(format!(
        "add the following arguments: {}",
        missing_args
            .iter()
            .map(|a| format!("{}: <value>", a))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Find a similar string using Levenshtein distance
fn find_similar<'a>(target: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let target_lower = target.to_lowercase();
    candidates
        .iter()
        .filter_map(|&candidate| {
            let distance = levenshtein_distance(&target_lower, &candidate.to_lowercase());
            if distance <= 3 {
                Some((candidate, distance))
            } else {
                None
            }
        })
        .min_by_key(|(_, d)| *d)
        .map(|(s, _)| s)
}

/// Calculate Levenshtein distance between two strings
fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = std::cmp::min(
                std::cmp::min(matrix[i - 1][j] + 1, matrix[i][j - 1] + 1),
                matrix[i - 1][j - 1] + cost,
            );
        }
    }

    matrix[a_len][b_len]
}

/// Find the span of a host definition in source
fn find_host_span(source: &str, host: &str, default_line: usize) -> Span {
    for (i, line) in source.lines().enumerate() {
        if line.contains(host) {
            if let Some(col) = line.find(host) {
                return Span::from_line_col(source, i + 1, col + 1, host.len());
            }
        }
    }
    Span::from_line_col(source, default_line, 1, 1)
}

// ============================================================================
// Error Code Registry
// ============================================================================

/// Registry of error codes with descriptions
pub struct ErrorCodeRegistry {
    codes: HashMap<String, ErrorCodeInfo>,
}

/// Information about an error code
#[derive(Debug, Clone)]
pub struct ErrorCodeInfo {
    /// Error code (e.g., "E0001")
    pub code: String,
    /// Short description
    pub title: String,
    /// Detailed explanation
    pub explanation: String,
    /// Common causes
    pub causes: Vec<String>,
    /// How to fix
    pub fixes: Vec<String>,
}

impl ErrorCodeRegistry {
    /// Create the default error code registry
    pub fn new() -> Self {
        let mut codes = HashMap::new();

        codes.insert(
            "E0001".to_string(),
            ErrorCodeInfo {
                code: "E0001".to_string(),
                title: "Undefined Variable".to_string(),
                explanation: "A variable was referenced but not defined in the current scope."
                    .to_string(),
                causes: vec![
                    "Typo in variable name".to_string(),
                    "Variable defined in a different scope (e.g., different play)".to_string(),
                    "Variable file not loaded".to_string(),
                ],
                fixes: vec![
                    "Check spelling of variable name".to_string(),
                    "Define the variable in vars, host_vars, or group_vars".to_string(),
                    "Use 'vars_files' to load external variable files".to_string(),
                ],
            },
        );

        codes.insert(
            "E0002".to_string(),
            ErrorCodeInfo {
                code: "E0002".to_string(),
                title: "Module Not Found".to_string(),
                explanation: "The specified module does not exist or is not available.".to_string(),
                causes: vec![
                    "Typo in module name".to_string(),
                    "Module requires a feature flag".to_string(),
                    "Module from a collection that isn't installed".to_string(),
                ],
                fixes: vec![
                    "Check module spelling".to_string(),
                    "Use 'rustible galaxy install' to install required collections".to_string(),
                    "Check available modules with 'rustible doc -l'".to_string(),
                ],
            },
        );

        codes.insert(
            "E0003".to_string(),
            ErrorCodeInfo {
                code: "E0003".to_string(),
                title: "Invalid Module Argument".to_string(),
                explanation: "An argument passed to a module has an invalid type or value."
                    .to_string(),
                causes: vec![
                    "Wrong data type (e.g., string instead of boolean)".to_string(),
                    "Invalid value for an enum argument".to_string(),
                    "Missing quotes around string values".to_string(),
                ],
                fixes: vec![
                    "Check module documentation for expected types".to_string(),
                    "Use 'rustible doc <module>' to see argument specifications".to_string(),
                ],
            },
        );

        codes.insert(
            "E0004".to_string(),
            ErrorCodeInfo {
                code: "E0004".to_string(),
                title: "Missing Required Argument".to_string(),
                explanation: "A required argument was not provided to a module.".to_string(),
                causes: vec![
                    "Forgot to include a required parameter".to_string(),
                    "Typo in argument name (so it wasn't recognized)".to_string(),
                ],
                fixes: vec![
                    "Add the missing required argument".to_string(),
                    "Check module documentation for required arguments".to_string(),
                ],
            },
        );

        codes.insert(
            "E0010".to_string(),
            ErrorCodeInfo {
                code: "E0010".to_string(),
                title: "YAML Syntax Error".to_string(),
                explanation: "The playbook contains invalid YAML syntax.".to_string(),
                causes: vec![
                    "Incorrect indentation".to_string(),
                    "Missing colons after keys".to_string(),
                    "Tabs instead of spaces".to_string(),
                    "Unquoted special characters".to_string(),
                ],
                fixes: vec![
                    "Use consistent 2-space indentation".to_string(),
                    "Run 'rustible check' to validate syntax".to_string(),
                    "Quote strings containing special characters".to_string(),
                ],
            },
        );

        codes.insert(
            "E0020".to_string(),
            ErrorCodeInfo {
                code: "E0020".to_string(),
                title: "Template Syntax Error".to_string(),
                explanation: "A Jinja2 template expression contains invalid syntax.".to_string(),
                causes: vec![
                    "Unmatched {{ or }}".to_string(),
                    "Invalid filter name".to_string(),
                    "Undefined variable in template".to_string(),
                ],
                fixes: vec![
                    "Check for balanced {{ }} and {% %} pairs".to_string(),
                    "Verify filter names are correct".to_string(),
                    "Define all variables used in templates".to_string(),
                ],
            },
        );

        codes.insert(
            "E0030".to_string(),
            ErrorCodeInfo {
                code: "E0030".to_string(),
                title: "Connection Error".to_string(),
                explanation: "Failed to establish a connection to a target host.".to_string(),
                causes: vec![
                    "Host is unreachable (network issue)".to_string(),
                    "SSH key not authorized".to_string(),
                    "Wrong port or connection method".to_string(),
                    "Firewall blocking connection".to_string(),
                ],
                fixes: vec![
                    "Verify host is reachable: ping <host>".to_string(),
                    "Check SSH key is in authorized_keys".to_string(),
                    "Verify connection settings in inventory".to_string(),
                    "Use 'rustible ping -i inventory.yml' to test connectivity".to_string(),
                ],
            },
        );

        Self { codes }
    }

    /// Get information about an error code
    pub fn get(&self, code: &str) -> Option<&ErrorCodeInfo> {
        self.codes.get(code)
    }

    /// Explain an error code
    pub fn explain(&self, code: &str) -> Option<String> {
        self.get(code).map(|info| {
            let mut output = String::new();
            writeln!(output, "{}: {}", info.code, info.title).ok();
            writeln!(output).ok();
            writeln!(output, "{}", info.explanation).ok();
            writeln!(output).ok();
            writeln!(output, "Common causes:").ok();
            for cause in &info.causes {
                writeln!(output, "  - {}", cause).ok();
            }
            writeln!(output).ok();
            writeln!(output, "How to fix:").ok();
            for fix in &info.fixes {
                writeln!(output, "  - {}", fix).ok();
            }
            output
        })
    }
}

impl Default for ErrorCodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_from_line_col() {
        let source = "line 1\nline 2\nline 3";
        let span = Span::from_line_col(source, 2, 1, 4);
        assert_eq!(span.start, 7);
        assert_eq!(span.end, 11);
    }

    #[test]
    fn test_levenshtein_distance() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
        assert_eq!(levenshtein_distance("", "abc"), 3);
    }

    #[test]
    fn test_find_similar() {
        let candidates = &["var1", "var2", "hostname", "inventory_hostname"];
        assert_eq!(find_similar("var", candidates), Some("var1"));
        assert_eq!(find_similar("hostnam", candidates), Some("hostname"));
        assert_eq!(find_similar("xyz", candidates), None);
    }

    #[test]
    fn test_undefined_variable_error_render() {
        let source = r#"- name: Test play
  hosts: all
  tasks:
    - name: Debug
      debug:
        msg: "{{ wrong_var }}""#;

        let diag = undefined_variable_error(
            "playbook.yml",
            source,
            6,
            14,
            "wrong_var",
            &["var1", "var2"],
        );

        let output = diag.render_with_source(Some(source));
        assert!(output.contains("undefined variable"));
        assert!(output.contains("wrong_var"));
    }

    #[test]
    fn test_error_code_registry() {
        let registry = ErrorCodeRegistry::new();
        assert!(registry.get("E0001").is_some());
        assert!(registry.get("E9999").is_none());

        let explanation = registry.explain("E0001").unwrap();
        assert!(explanation.contains("Undefined Variable"));
    }

    #[test]
    fn test_rich_diagnostic_builder() {
        let diag = RichDiagnostic::error("test error", "test.yml", Span::new(0, 10))
            .with_code("E0001")
            .with_label("error here")
            .with_help("try this fix")
            .with_note("additional info");

        assert_eq!(diag.code, Some("E0001".to_string()));
        assert_eq!(diag.notes.len(), 2); // help is added as note
        assert!(diag.suggestions.is_empty());
    }
}
