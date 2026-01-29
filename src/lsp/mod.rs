//! LSP (Language Server Protocol) server for IDE integration
//!
//! This module provides LSP capabilities for Rustible playbooks including:
//! - Autocompletion for modules, variables, and handlers
//! - Hover documentation
//! - Go to definition
//! - Diagnostics (errors, warnings)
//! - Code actions
//! - Symbol information

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::diagnostics::RichDiagnostic;
use crate::modules::ModuleRegistry;

/// LSP configuration
#[derive(Debug, Clone)]
pub struct LspConfig {
    /// Enable autocompletion
    pub enable_completion: bool,
    /// Enable hover
    pub enable_hover: bool,
    /// Enable go to definition
    pub enable_goto_definition: bool,
    /// Enable diagnostics
    pub enable_diagnostics: bool,
    /// Maximum number of completion items
    pub max_completion_items: usize,
}

impl Default for LspConfig {
    fn default() -> Self {
        Self {
            enable_completion: true,
            enable_hover: true,
            enable_goto_definition: true,
            enable_diagnostics: true,
            max_completion_items: 100,
        }
    }
}

/// Document position (0-indexed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Position {
    /// Line number (0-indexed)
    pub line: usize,
    /// Character offset (0-indexed, UTF-16)
    pub character: usize,
}

/// Document range
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// Start position
    pub start: Position,
    /// End position
    pub end: Position,
}

/// Text document identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextDocumentIdentifier {
    /// Document URI
    pub uri: String,
}

/// Text document item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDocumentItem {
    /// Document URI
    pub uri: String,
    /// Language identifier
    pub language_id: String,
    /// Version number
    pub version: i32,
    /// Document content
    pub text: String,
}

/// Completion item kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompletionItemKind {
    Text,
    Method,
    Function,
    Constructor,
    Field,
    Variable,
    Class,
    Interface,
    Module,
    Property,
    Unit,
    Value,
    Enum,
    Keyword,
    Snippet,
    Color,
    File,
    Reference,
    Folder,
    EnumMember,
    Constant,
    Struct,
    Event,
    Operator,
    TypeParameter,
}

/// Completion item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    /// Label for this completion item
    pub label: String,
    /// Kind of completion item
    pub kind: Option<CompletionItemKind>,
    /// Detail for this item
    pub detail: Option<String>,
    /// Documentation for this item
    pub documentation: Option<String>,
    /// Preselect this item
    pub preselect: Option<bool>,
    /// Sort text for this item
    pub sort_text: Option<String>,
    /// Filter text for this item
    pub filter_text: Option<String>,
    /// Insert text for this item
    pub insert_text: Option<String>,
    /// Text edit for this item
    pub text_edit: Option<TextEdit>,
}

/// Text edit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    /// Range to replace
    pub range: Range,
    /// New text
    pub new_text: String,
}

/// Hover response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hover {
    /// Hover contents
    pub contents: HoverContents,
    /// Range for the hover
    pub range: Option<Range>,
}

/// Hover contents
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HoverContents {
    /// Scalar content
    Scalar(String),
    /// Markup content
    Markup(MarkupContent),
}

/// Markup content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkupContent {
    /// Kind of markup
    pub kind: MarkupKind,
    /// Content
    pub value: String,
}

/// Markup kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkupKind {
    PlainText,
    Markdown,
}

/// Diagnostic severity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// Diagnostic
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Range for this diagnostic
    pub range: Range,
    /// Severity
    pub severity: Option<DiagnosticSeverity>,
    /// Code
    pub code: Option<String>,
    /// Source
    pub source: Option<String>,
    /// Message
    pub message: String,
    /// Related information
    pub related_information: Option<Vec<DiagnosticRelatedInformation>>,
}

/// Diagnostic related information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticRelatedInformation {
    /// Location
    pub location: Location,
    /// Message
    pub message: String,
}

/// Location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// URI
    pub uri: String,
    /// Range
    pub range: Range,
}

/// Code action kind
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeActionKind(String);

/// Code action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAction {
    /// Title
    pub title: String,
    /// Kind
    pub kind: Option<CodeActionKind>,
    /// Diagnostics
    pub diagnostics: Option<Vec<Diagnostic>>,
    /// Edit
    pub edit: Option<WorkspaceEdit>,
    /// Is preferred
    pub is_preferred: Option<bool>,
}

/// Workspace edit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceEdit {
    /// Changes
    pub changes: Option<HashMap<String, Vec<TextEdit>>>,
}

/// Symbol kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    File = 1,
    Module = 2,
    Namespace = 3,
    Package = 4,
    Class = 5,
    Method = 6,
    Property = 7,
    Field = 8,
    Constructor = 9,
    Enum = 10,
    Interface = 11,
    Function = 12,
    Variable = 13,
    Constant = 14,
    String = 15,
    Number = 16,
    Boolean = 17,
    Array = 18,
    Object = 19,
    Key = 20,
    Null = 21,
    EnumMember = 22,
    Struct = 23,
    Event = 24,
    Operator = 25,
    TypeParameter = 26,
}

/// Document symbol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSymbol {
    /// Name
    pub name: String,
    /// Kind
    pub kind: SymbolKind,
    /// Deprecated
    pub deprecated: Option<bool>,
    /// Range
    pub range: Range,
    /// Selection range
    pub selection_range: Range,
    /// Children
    pub children: Option<Vec<DocumentSymbol>>,
}

/// LSP server
pub struct LspServer {
    /// Configuration
    config: LspConfig,
    /// Open documents
    documents: Arc<RwLock<HashMap<String, TextDocumentItem>>>,
    /// Module registry
    module_registry: ModuleRegistry,
    /// Diagnostics cache
    diagnostics: Arc<RwLock<HashMap<String, Vec<Diagnostic>>>>,
}

impl LspServer {
    /// Create a new LSP server
    pub fn new(config: LspConfig) -> Self {
        Self {
            config,
            documents: Arc::new(RwLock::new(HashMap::new())),
            module_registry: ModuleRegistry::with_builtins(),
            diagnostics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(LspConfig::default())
    }

    /// Open a document
    pub async fn open_document(&self, item: TextDocumentItem) {
        let uri = item.uri.clone();
        let mut docs = self.documents.write().await;
        docs.insert(uri.clone(), item);
        
        // Compute diagnostics
        if self.config.enable_diagnostics {
            self.compute_diagnostics(&uri).await;
        }
    }

    /// Close a document
    pub async fn close_document(&self, uri: &str) {
        let mut docs = self.documents.write().await;
        docs.remove(uri);
        
        let mut diags = self.diagnostics.write().await;
        diags.remove(uri);
    }

    /// Change document content
    pub async fn change_document(&self, uri: &str, new_content: String) {
        let mut docs = self.documents.write().await;
        if let Some(doc) = docs.get_mut(uri) {
            doc.text = new_content;
        }
        
        // Recompute diagnostics
        if self.config.enable_diagnostics {
            self.compute_diagnostics(uri).await;
        }
    }

    /// Get completion items
    pub async fn get_completion(&self, uri: &str, position: Position) -> Vec<CompletionItem> {
        if !self.config.enable_completion {
            return Vec::new();
        }

        let docs = self.documents.read().await;
        let doc = match docs.get(uri) {
            Some(d) => d,
            None => return Vec::new(),
        };

        let mut items = Vec::new();

        // Add module completions
        items.extend(self.get_module_completions());

        // Add variable completions
        items.extend(self.get_variable_completions(&doc.text));

        // Add keyword completions
        items.extend(self.get_keyword_completions());

        // Limit number of items
        if items.len() > self.config.max_completion_items {
            items.truncate(self.config.max_completion_items);
        }

        items
    }

    /// Get module completions
    fn get_module_completions(&self) -> Vec<CompletionItem> {
        let modules = self.module_registry.names();
        
        modules.into_iter().map(|name| CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::Module),
            detail: Some("Module".to_string()),
            documentation: Some(format!("Module: {}", name)),
            sort_text: Some(format!("0_{}", name)),
            ..Default::default()
        }).collect()
    }

    /// Get variable completions
    fn get_variable_completions(&self, content: &str) -> Vec<CompletionItem> {
        let mut variables = Vec::new();
        
        // Collect variables from content
        // Simplified implementation
        let builtins = vec![
            "ansible_hostname",
            "inventory_hostname",
            "hostvars",
            "groups",
            "group_names",
            "play_hosts",
            "ansible_version",
            "ansible_facts",
        ];
        
        for var in builtins {
            variables.push(CompletionItem {
                label: var.to_string(),
                kind: Some(CompletionItemKind::Variable),
                detail: Some("Built-in variable".to_string()),
                sort_text: Some(format!("1_{}", var)),
                ..Default::default()
            });
        }
        
        variables
    }

    /// Get keyword completions
    fn get_keyword_completions(&self) -> Vec<CompletionItem> {
        let keywords = vec![
            ("hosts", "Target hosts pattern"),
            ("tasks", "Task list"),
            ("vars", "Variables"),
            ("handlers", "Handlers"),
            ("name", "Task name"),
            ("become", "Privilege escalation"),
            ("when", "Conditional expression"),
            ("with_items", "Loop over items"),
            ("notify", "Notify handlers"),
        ];
        
        keywords.into_iter().map(|(keyword, desc)| CompletionItem {
            label: keyword.to_string(),
            kind: Some(CompletionItemKind::Keyword),
            detail: Some("Keyword".to_string()),
            documentation: Some(desc.to_string()),
            sort_text: Some(format!("2_{}", keyword)),
            ..Default::default()
        }).collect()
    }

    /// Get hover information
    pub async fn get_hover(&self, uri: &str, position: Position) -> Option<Hover> {
        if !self.config.enable_hover {
            return None;
        }

        let docs = self.documents.read().await;
        let doc = docs.get(uri)?;

        // Get the word at position
        let word = self.get_word_at_position(&doc.text, position)?;
        
        // Determine what kind of hover to provide
        if self.module_registry.contains(&word) {
            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("## Module: `{}`\n\nDocumentation for this module.", word),
                }),
                range: None,
            })
        } else {
            Some(Hover {
                contents: HoverContents::Scalar(format!("Variable: `{}`", word)),
                range: None,
            })
        }
    }

    /// Get word at position
    fn get_word_at_position(&self, text: &str, position: Position) -> Option<String> {
        let lines: Vec<&str> = text.lines().collect();
        
        if position.line >= lines.len() {
            return None;
        }
        
        let line = lines[position.line];
        let chars: Vec<char> = line.chars().collect();
        
        if position.character >= chars.len() {
            return None;
        }
        
        // Find word boundaries
        let start = chars[..position.character]
            .iter()
            .rposition(|c| !c.is_alphanumeric() && *c != '_')
            .map(|i| i + 1)
            .unwrap_or(0);
        
        let end = chars[position.character..]
            .iter()
            .position(|c| !c.is_alphanumeric() && *c != '_')
            .map(|i| position.character + i)
            .unwrap_or(chars.len());
        
        Some(chars[start..end].iter().collect())
    }

    /// Get diagnostics
    pub async fn get_diagnostics(&self, uri: &str) -> Vec<Diagnostic> {
        let diags = self.diagnostics.read().await;
        diags.get(uri).cloned().unwrap_or_default()
    }

    /// Compute diagnostics for a document
    async fn compute_diagnostics(&self, uri: &str) {
        let docs = self.documents.read().await;
        let doc = match docs.get(uri) {
            Some(d) => d.clone(),
            None => return,
        };
        drop(docs);

        // Convert RichDiagnostics to LSP Diagnostics
        let rich_diags = self.analyze_document(&doc);
        
        let lsp_diags: Vec<Diagnostic> = rich_diags.into_iter()
            .map(|d| self.convert_diagnostic(uri, d))
            .collect();
        
        let mut diags = self.diagnostics.write().await;
        diags.insert(uri.to_string(), lsp_diags);
    }

    /// Analyze document for issues
    fn analyze_document(&self, doc: &TextDocumentItem) -> Vec<RichDiagnostic> {
        // Simplified analysis - in real implementation would use full parser
        let mut diags = Vec::new();
        
        for (line_num, line) in doc.text.lines().enumerate() {
            // Check for tabs
            if line.contains('\t') {
                diags.push(RichDiagnostic::warning(
                    "tabs found - use spaces instead",
                    &doc.uri,
                    crate::diagnostics::Span::from_line_col(&doc.text, line_num + 1, 1, 1),
                ).with_label("tab character"));
            }
        }
        
        diags
    }

    /// Convert RichDiagnostic to LSP Diagnostic
    fn convert_diagnostic(&self, uri: &str, diag: RichDiagnostic) -> Diagnostic {
        Diagnostic {
            range: Range {
                start: Position {
                    line: 0, // Would parse from Span
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            },
            severity: match diag.severity {
                crate::diagnostics::DiagnosticSeverity::Error => Some(DiagnosticSeverity::Error),
                crate::diagnostics::DiagnosticSeverity::Warning => Some(DiagnosticSeverity::Warning),
                crate::diagnostics::DiagnosticSeverity::Note => Some(DiagnosticSeverity::Information),
                crate::diagnostics::DiagnosticSeverity::Help => Some(DiagnosticSeverity::Hint),
            },
            code: diag.code,
            source: Some("rustible".to_string()),
            message: diag.message,
            related_information: None,
        }
    }

    /// Get document symbols
    pub async fn get_document_symbols(&self, uri: &str) -> Vec<DocumentSymbol> {
        let docs = self.documents.read().await;
        let doc = match docs.get(uri) {
            Some(d) => d,
            None => return Vec::new(),
        };

        let mut symbols = Vec::new();
        
        // Parse plays
        for (line_num, line) in doc.text.lines().enumerate() {
            if line.trim_start().starts_with("- name:") || line.trim_start().starts_with("name:") {
                let name = line.split(':').nth(1).unwrap_or("Unnamed").trim();
                symbols.push(DocumentSymbol {
                    name: name.to_string(),
                    kind: SymbolKind::Function,
                    deprecated: None,
                    range: Range {
                        start: Position { line: line_num, character: 0 },
                        end: Position { line: line_num + 1, character: 0 },
                    },
                    selection_range: Range {
                        start: Position { line: line_num, character: 0 },
                        end: Position { line: line_num + 1, character: 0 },
                    },
                    children: None,
                });
            }
        }
        
        symbols
    }

    /// Get code actions
    pub async fn get_code_actions(&self, uri: &str, range: Range) -> Vec<CodeAction> {
        let diags = self.get_diagnostics(uri).await;
        let mut actions = Vec::new();
        
        for diag in diags {
            if diag.range.start.line >= range.start.line && diag.range.end.line <= range.end.line {
                actions.push(CodeAction {
                    title: format!("Fix: {}", diag.message),
                    kind: None,
                    diagnostics: Some(vec![diag.clone()]),
                    edit: None,
                    is_preferred: Some(true),
                });
            }
        }
        
        actions
    }
}

impl Default for LspServer {
    fn default() -> Self {
        Self::new(LspConfig::default())
    }
}

// Default implementations for structs
impl Default for CompletionItem {
    fn default() -> Self {
        Self {
            label: String::new(),
            kind: None,
            detail: None,
            documentation: None,
            preselect: None,
            sort_text: None,
            filter_text: None,
            insert_text: None,
            text_edit: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lsp_server() {
        let server = LspServer::default();
        
        let item = TextDocumentItem {
            uri: "file:///test.yml".to_string(),
            language_id: "yaml".to_string(),
            version: 1,
            text: "- hosts: all\n  tasks:\n    - name: Test\n      ping:".to_string(),
        };
        
        server.open_document(item).await;
        
        let completions = server.get_completion("file:///test.yml", Position { line: 3, character: 6 }).await;
        assert!(!completions.is_empty());
    }

    #[test]
    fn test_position() {
        let pos = Position { line: 5, character: 10 };
        assert_eq!(pos.line, 5);
        assert_eq!(pos.character, 10);
    }
}
