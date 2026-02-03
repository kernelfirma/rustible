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
        let (start, line, column) = offset_for_line_col(source, line, column).unwrap_or((0, 0, 0));
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
        let end = if self.end > self.start {
            self.end
        } else {
            self.start + 1
        };
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
    pub secondary_labels: Vec<(Span, String)>,
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
            secondary_labels: Vec::new(),
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

    pub fn with_secondary_label(mut self, span: Span, message: &str) -> Self {
        self.secondary_labels.push((span, message.to_string()));
        self
    }

    /// Render the diagnostic with the given source and print to stderr.
    pub fn eprint_with_source(&self, source: &str) {
        eprint!("{}", self.render_with_source(Some(source)));
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

            for (sec_span, sec_msg) in &self.secondary_labels {
                report = report.with_label(
                    Label::new((self.file.clone(), sec_span.range()))
                        .with_message(sec_msg.clone())
                        .with_color(Color::Blue),
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
        output.push_str(&format!(
            "{}: {}",
            severity_label(self.severity),
            self.message
        ));
        if self.span.has_line_col() {
            output.push_str(&format!(
                "\n --> {}:{}:{}",
                self.file, self.span.line, self.span.column
            ));
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
///
/// Detects tabs and missing-colon patterns near the error location and
/// attaches auto-fix [`Suggestion`]s when possible.
pub fn yaml_syntax_error(
    path: impl AsRef<Path>,
    source: &str,
    line: usize,
    column: usize,
    message: &str,
) -> RichDiagnostic {
    let span = Span::from_line_col(source, line, column, 1);
    let mut diag = RichDiagnostic::error(
        format!("YAML syntax error: {}", message),
        path.as_ref(),
        span,
    )
    .with_code("E0001")
    .with_label("invalid YAML syntax")
    .with_help("Check indentation, colons, and list markers");

    // Try to get the offending line for pattern detection.
    if let Some(raw_line) = source.lines().nth(line.saturating_sub(1)) {
        // Detect tab characters.
        if let Some(tab_pos) = raw_line.find('\t') {
            let line_start = offset_of_line(source, line);
            let tab_span = Span {
                start: line_start + tab_pos,
                end: line_start + tab_pos + 1,
                line,
                column: tab_pos + 1,
            };
            diag = diag.with_suggestion(Suggestion::new(
                "Replace tab with spaces",
                Some(tab_span),
                Some("  ".to_string()),
            ));
        }

        // Detect missing colon – simple heuristic: a line that looks like
        // `key value` without a colon (trimmed, no leading `-`, not a comment).
        let trimmed = raw_line.trim();
        if !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && !trimmed.starts_with('-')
            && !trimmed.contains(':')
        {
            if let Some(space_idx) = trimmed.find(' ') {
                let fixed = format!("{}: {}", &trimmed[..space_idx], &trimmed[space_idx + 1..]);
                let line_start = offset_of_line(source, line);
                let content_offset = raw_line.len() - raw_line.trim_start().len();
                let fix_span = Span {
                    start: line_start + content_offset,
                    end: line_start + raw_line.len(),
                    line,
                    column: content_offset + 1,
                };
                diag = diag.with_suggestion(Suggestion::new(
                    format!("Add missing colon: `{}`", fixed),
                    Some(fix_span),
                    Some(fixed),
                ));
            }
        }
    }

    diag
}

/// Template syntax error with source span and hint.
///
/// Detects unclosed `{{` / `{%` delimiters near the error location and
/// attaches auto-fix [`Suggestion`]s when possible.
pub fn template_syntax_error(
    path: impl AsRef<Path>,
    source: &str,
    line: usize,
    column: usize,
    message: &str,
) -> RichDiagnostic {
    let span = Span::from_line_col(source, line, column, 1);
    let mut diag = RichDiagnostic::error(
        format!("Template syntax error: {}", message),
        path.as_ref(),
        span,
    )
    .with_code("E0002")
    .with_label("invalid template syntax")
    .with_help("Check {{ }} delimiters and filter names");

    if let Some(raw_line) = source.lines().nth(line.saturating_sub(1)) {
        let line_start = offset_of_line(source, line);

        // Detect unclosed {{ without }}
        if raw_line.contains("{{") && !raw_line.contains("}}") {
            if let Some(pos) = raw_line.find("{{") {
                let expr = raw_line[pos + 2..].trim();
                let replacement = format!("{{{{ {} }}}}", expr);
                let open_span = Span {
                    start: line_start + pos,
                    end: line_start + raw_line.len(),
                    line,
                    column: pos + 1,
                };
                diag = diag.with_suggestion(Suggestion::new(
                    format!("Close the expression: `{}`", replacement),
                    Some(open_span),
                    Some(replacement),
                ));
            }
        }

        // Detect unclosed {%  without %}
        if raw_line.contains("{%") && !raw_line.contains("%}") {
            if let Some(pos) = raw_line.find("{%") {
                let tag_body = raw_line[pos + 2..].trim();
                let replacement = format!("{{% {} %}}", tag_body);
                let open_span = Span {
                    start: line_start + pos,
                    end: line_start + raw_line.len(),
                    line,
                    column: pos + 1,
                };
                diag = diag.with_suggestion(Suggestion::new(
                    format!("Close the block tag: `{}`", replacement),
                    Some(open_span),
                    Some(replacement),
                ));
            }
        }
    }

    diag
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
///
/// When `known_modules` is non-empty, uses [`suggest_similar`] to attach
/// did-you-mean [`Suggestion`]s.
pub fn module_not_found_error(
    path: impl AsRef<Path>,
    module: &str,
    span: Span,
    known_modules: &[&str],
) -> RichDiagnostic {
    let mut diag = RichDiagnostic::error(
        format!("Module '{}' not found", module),
        path.as_ref(),
        span,
    )
    .with_code("E0004")
    .with_label("unknown module")
    .with_help("Check module name or install required collection");

    let suggestions = suggest_similar(module, known_modules);
    if let Some(first) = suggestions.first() {
        diag = diag.with_note(format!("Did you mean '{}' ?", first));
        diag = diag.with_suggestion(Suggestion::new(
            format!("Replace '{}' with '{}'", module, first),
            Some(span),
            Some(first.to_string()),
        ));
    }

    diag
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
///
/// Attaches a [`Suggestion`] showing example YAML for the missing argument.
pub fn missing_required_arg_error(
    path: impl AsRef<Path>,
    module: &str,
    arg: &str,
    span: Span,
) -> RichDiagnostic {
    RichDiagnostic::error(
        format!(
            "Missing required argument '{}' for module '{}'",
            arg, module
        ),
        path.as_ref(),
        span,
    )
    .with_code("E0020")
    .with_label("missing required argument")
    .with_help("Add the required argument to the module call")
    .with_suggestion(Suggestion::new(
        format!("Add '{}: <value>' to the module parameters", arg),
        None,
        None,
    ))
}

/// Connection error diagnostic.
///
/// Pattern-matches on common error messages (refused, timeout, auth) to
/// provide contextual fix suggestions.
pub fn connection_error(
    path: impl AsRef<Path>,
    host: &str,
    message: &str,
    span: Span,
) -> RichDiagnostic {
    let msg_lower = message.to_lowercase();
    let mut diag = RichDiagnostic::error(
        format!("Connection error for {}: {}", host, message),
        path.as_ref(),
        span,
    )
    .with_code("E0030")
    .with_label("connection failed")
    .with_help("Verify host reachability and credentials");

    if msg_lower.contains("refused") {
        diag = diag.with_suggestion(Suggestion::new(
            format!("Verify the SSH/service is running on {}", host),
            None,
            None,
        ));
        diag = diag.with_suggestion(Suggestion::new(
            "Check firewall rules allow the connection port",
            None,
            None,
        ));
    } else if msg_lower.contains("timed out") || msg_lower.contains("timeout") {
        diag = diag.with_suggestion(Suggestion::new(
            format!("Verify {} is reachable (ping or traceroute)", host),
            None,
            None,
        ));
        diag = diag.with_suggestion(Suggestion::new(
            "Check firewall rules and network routing",
            None,
            None,
        ));
    } else if msg_lower.contains("auth") || msg_lower.contains("permission denied") {
        diag = diag.with_suggestion(Suggestion::new(
            "Check SSH key or credentials configuration",
            None,
            None,
        ));
        diag = diag.with_suggestion(Suggestion::new(
            "Verify the remote user has access to the target host",
            None,
            None,
        ));
    }

    diag
}

/// Compute the byte offset of the start of the given 1-based `line` in
/// `source`. Returns 0 if the line is out of range.
fn offset_of_line(source: &str, line: usize) -> usize {
    let mut offset = 0usize;
    for (idx, raw_line) in source.lines().enumerate() {
        if idx + 1 == line {
            return offset;
        }
        offset += raw_line.len() + 1;
    }
    0
}

/// Levenshtein edit distance (simple inline implementation).
fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let a_len = a_bytes.len();
    let b_len = b_bytes.len();

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        for j in 1..=b_len {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

/// Find candidates similar to `target` using prefix/contains matching and
/// Levenshtein edit distance. Returns up to 3 matches sorted by relevance.
fn suggest_similar(target: &str, candidates: &[&str]) -> Vec<String> {
    // Score each candidate – lower is better.
    // Prefix/contains matches get score 0, otherwise use edit distance.
    let max_dist = (target.len() / 2).max(2);
    let mut scored: Vec<(usize, &str)> = candidates
        .iter()
        .filter_map(|c| {
            if c.starts_with(target) || target.starts_with(*c) || c.contains(target) {
                Some((0, *c))
            } else {
                let dist = levenshtein(target, c);
                if dist <= max_dist {
                    Some((dist, *c))
                } else {
                    None
                }
            }
        })
        .collect();

    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
    scored.truncate(3);
    scored.into_iter().map(|(_, s)| s.to_string()).collect()
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

    // --- Levenshtein ---

    #[test]
    fn test_levenshtein_identical() {
        assert_eq!(levenshtein("abc", "abc"), 0);
    }

    #[test]
    fn test_levenshtein_basic() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn test_levenshtein_single_edit() {
        assert_eq!(levenshtein("file", "fiel"), 2); // transposition = 2 ops
        assert_eq!(levenshtein("copy", "cop"), 1);
        assert_eq!(levenshtein("copy", "copyx"), 1);
    }

    // --- suggest_similar with edit distance ---

    #[test]
    fn test_suggest_similar_prefix() {
        let result = suggest_similar("cop", &["copy", "file", "template"]);
        assert!(result.contains(&"copy".to_string()));
    }

    #[test]
    fn test_suggest_similar_edit_distance() {
        // "flie" is edit-distance 2 from "file" – should match
        let result = suggest_similar("flie", &["file", "copy", "template"]);
        assert!(result.contains(&"file".to_string()));
    }

    #[test]
    fn test_suggest_similar_no_match() {
        let result = suggest_similar("zzzzz", &["file", "copy", "template"]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_suggest_similar_max_three() {
        let result = suggest_similar("a", &["a1", "a2", "a3", "a4"]);
        assert!(result.len() <= 3);
    }

    // --- yaml_syntax_error suggestions ---

    #[test]
    fn test_yaml_syntax_error_tab_suggestion() {
        let source = "\tname: foo";
        let diag = yaml_syntax_error("test.yml", source, 1, 1, "tab character");
        assert!(!diag.suggestions.is_empty());
        let tab_sugg = &diag.suggestions[0];
        assert!(tab_sugg.message.contains("tab"));
        assert_eq!(tab_sugg.replacement.as_deref(), Some("  "));
    }

    #[test]
    fn test_yaml_syntax_error_missing_colon() {
        let source = "name foo";
        let diag = yaml_syntax_error("test.yml", source, 1, 1, "missing colon");
        let colon_sugg = diag
            .suggestions
            .iter()
            .find(|s| s.message.contains("colon"));
        assert!(colon_sugg.is_some());
        let sugg = colon_sugg.unwrap();
        assert_eq!(sugg.replacement.as_deref(), Some("name: foo"));
    }

    #[test]
    fn test_yaml_syntax_error_tab_patch_snippet() {
        let source = "\tname: foo";
        let diag = yaml_syntax_error("test.yml", source, 1, 1, "tab character");
        let tab_sugg = &diag.suggestions[0];
        let patch = tab_sugg.patch_snippet(source);
        assert!(patch.is_some());
        let patch_str = patch.unwrap();
        assert!(patch_str.contains("+ "));
        assert!(patch_str.contains("- \tname: foo"));
    }

    // --- template_syntax_error suggestions ---

    #[test]
    fn test_template_unclosed_expression() {
        let source = "{{ foo";
        let diag = template_syntax_error("test.yml", source, 1, 1, "unclosed expression");
        let sugg = diag
            .suggestions
            .iter()
            .find(|s| s.message.contains("Close"));
        assert!(sugg.is_some());
        assert!(sugg.unwrap().replacement.as_ref().unwrap().contains("}}"));
    }

    #[test]
    fn test_template_unclosed_block_tag() {
        let source = "{% if true";
        let diag = template_syntax_error("test.yml", source, 1, 1, "unclosed block");
        let sugg = diag
            .suggestions
            .iter()
            .find(|s| s.message.contains("block tag"));
        assert!(sugg.is_some());
        assert!(sugg.unwrap().replacement.as_ref().unwrap().contains("%}"));
    }

    #[test]
    fn test_template_no_suggestion_when_closed() {
        let source = "{{ foo }}";
        let diag = template_syntax_error("test.yml", source, 1, 1, "other error");
        assert!(diag.suggestions.is_empty());
    }

    // --- module_not_found_error suggestions ---

    #[test]
    fn test_module_not_found_did_you_mean() {
        let diag = module_not_found_error(
            "test.yml",
            "coppy",
            Span::new(0, 5),
            &["copy", "file", "template"],
        );
        assert!(!diag.suggestions.is_empty());
        assert!(diag.suggestions[0].message.contains("copy"));
    }

    #[test]
    fn test_module_not_found_no_known() {
        let diag = module_not_found_error("test.yml", "coppy", Span::new(0, 5), &[]);
        assert!(diag.suggestions.is_empty());
    }

    // --- missing_required_arg_error suggestions ---

    #[test]
    fn test_missing_required_arg_has_suggestion() {
        let diag = missing_required_arg_error("test.yml", "copy", "src", Span::new(0, 4));
        assert_eq!(diag.suggestions.len(), 1);
        assert!(diag.suggestions[0].message.contains("src"));
        assert!(diag.suggestions[0].message.contains("<value>"));
    }

    // --- connection_error suggestions ---

    #[test]
    fn test_connection_error_refused() {
        let diag = connection_error("test.yml", "web01", "Connection refused", Span::new(0, 5));
        assert!(diag.suggestions.len() >= 2);
        assert!(diag.suggestions.iter().any(|s| s.message.contains("SSH")));
        assert!(diag
            .suggestions
            .iter()
            .any(|s| s.message.contains("firewall")));
    }

    #[test]
    fn test_connection_error_timeout() {
        let diag = connection_error("test.yml", "web01", "Connection timed out", Span::new(0, 5));
        assert!(diag.suggestions.len() >= 2);
        assert!(diag
            .suggestions
            .iter()
            .any(|s| s.message.contains("reachable")));
    }

    #[test]
    fn test_connection_error_auth() {
        let diag = connection_error("test.yml", "web01", "Permission denied", Span::new(0, 5));
        assert!(diag.suggestions.len() >= 2);
        assert!(diag
            .suggestions
            .iter()
            .any(|s| s.message.contains("SSH key")));
    }

    #[test]
    fn test_connection_error_generic() {
        let diag = connection_error("test.yml", "web01", "unknown failure", Span::new(0, 5));
        assert!(diag.suggestions.is_empty());
    }

    // --- offset_of_line helper ---

    #[test]
    fn test_offset_of_line() {
        let source = "abc\ndef\nghi";
        assert_eq!(offset_of_line(source, 1), 0);
        assert_eq!(offset_of_line(source, 2), 4);
        assert_eq!(offset_of_line(source, 3), 8);
    }
}
