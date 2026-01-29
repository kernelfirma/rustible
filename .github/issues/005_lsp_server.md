# DX: Implement LSP Server for IDE Integration (Autocompletion, Hover, Validation)

## Problem Statement
Neither Ansible nor Terraform have first-class Language Server Protocol (LSP) support. This is a significant opportunity for Rustible to provide superior developer experience with real-time validation, autocompletion, and in-editor documentation.

## Current State
- No LSP server implementation
- No IDE integration
- No real-time validation
- No autocompletion support
- No hover documentation

## Proposed Solution

### Phase 1: Basic LSP Server (v0.1.x)
1. **LSP server scaffold**
   ```rust
   // src/lsp/server.rs
   use tower_lsp::{LspService, Server};
   
   pub struct RustibleLanguageServer {
       module_registry: Arc<ModuleRegistry>,
       document_cache: Arc<RwLock<HashMap<Url, ParsedPlaybook>>>,
       inventory_cache: Arc<RwLock<Option<Inventory>>>,
   }
   
   #[tower_lsp::async_trait]
   impl LanguageServer for RustibleLanguageServer {
       async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
           Ok(InitializeResult {
               capabilities: ServerCapabilities {
                   text_document_sync: Some(TextDocumentSyncCapability::Kind(
                       TextDocumentSyncKind::FULL
                   )),
                   completion_provider: Some(CompletionOptions {
                       resolve_provider: Some(false),
                       trigger_characters: Some(vec![" ".to_string(), ":".to_string()]),
                       ..Default::default()
                   }),
                   hover_provider: Some(HoverProviderCapability::Simple(true)),
                   definition_provider: Some(DefinitionProviderCapability::Simple(true)),
                   ..Default::default()
               },
               ..Default::default()
           })
       }
   }
   ```

2. **Document parsing and caching**
   - Parse playbooks on open/change
   - Cache parsed AST
   - Track inventory and role references

### Phase 2: Autocompletion (v0.1.x)
1. **Module name completion**
   ```rust
   async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
       let uri = params.text_document_position.text_document.uri;
       let position = params.text_document_position.position;
       
       let doc = self.document_cache.read().await.get(&uri)?;
       let context = doc.get_context_at(position);
       
       let completions = match context {
           Context::ModuleName => self.module_completions(),
           Context::ModuleArg(module) => self.module_arg_completions(module),
           Context::Variable => self.variable_completions(&doc),
           Context::HostPattern => self.host_completions(),
           _ => vec![],
       };
       
       Ok(Some(CompletionResponse::Array(completions)))
   }
   
   fn module_completions(&self) -> Vec<CompletionItem> {
       self.module_registry
           .get_all()
           .into_iter()
           .map(|module| CompletionItem {
               label: module.name().to_string(),
               kind: Some(CompletionItemKind::FUNCTION),
               detail: Some(module.description().to_string()),
               documentation: Some(Documentation::MarkupContent(MarkupContent {
                   kind: MarkupKind::Markdown,
                   value: module.documentation(),
               })),
               ..Default::default()
           })
           .collect()
   }
   ```

2. **Module argument completion**
   - Show required vs optional arguments
   - Type hints for values
   - Default values

3. **Variable completion**
   - Show available variables in scope
   - Include inventory variables
   - Show variable types

### Phase 3: Hover Documentation (v0.2.x)
1. **Module documentation**
   ```rust
   async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
       let uri = params.text_document_position_params.text_document.uri;
       let position = params.text_document_position_params.position;
       
       let doc = self.document_cache.read().await.get(&uri)?;
       if let Some(module_name) = doc.get_module_at(position) {
           let module = self.module_registry.get(&module_name)?;
           return Ok(Some(Hover {
               contents: HoverContents::Markup(MarkupContent {
                   kind: MarkupKind::Markdown,
                   value: format!(
                       "# {}\n\n{}\n\n## Arguments\n\n{}",
                       module.name(),
                       module.description(),
                       module.arguments_documentation()
                   ),
               }),
               range: Some(doc.get_module_range(position)),
           }));
       }
       
       Ok(None)
   }
   ```

2. **Variable hover**
   - Show variable value
   - Show variable source (inventory, playbook, facts)
   - Display variable type

### Phase 4: Go-to-Definition (v0.2.x)
1. **Role references**
   - Navigate to role definition
   - Show role tasks, handlers, files

2. **Include/import references**
   - Navigate to included files
   - Show task definitions

3. **Variable definitions**
   - Navigate to variable definition
   - Show variable scope

### Phase 5: Real-time Validation (v0.2.x)
1. **Syntax validation**
   - Validate YAML structure
   - Check indentation
   - Validate block structure

2. **Semantic validation**
   - Module existence
   - Module arguments
   - Variable references
   - Type checking

## Expected Outcomes
- VS Code extension with autocompletion
- Real-time error highlighting
- In-editor documentation
- Go-to-definition support
- Significantly improved developer experience

## Success Criteria
- [ ] LSP server implemented
- [ ] Module name autocompletion working
- [ ] Module argument autocompletion working
- [ ] Variable autocompletion working
- [ ] Hover documentation for modules
- [ ] Hover documentation for variables
- [ ] Go-to-definition for roles
- [ ] Go-to-definition for includes
- [ ] Real-time validation errors
- [ ] VS Code extension published
- [ ] Vim/Neovim plugin available
- [ ] IntelliJ/PyCharm plugin available

## Implementation Details

### VS Code Extension
```json
{
  "name": "rustible",
  "displayName": "Rustible",
  "description": "Rustible Language Server",
  "version": "0.1.0",
  "engines": {
    "vscode": "^1.80.0"
  },
  "categories": ["Languages"],
  "contributes": {
    "languages": [
      {
        "id": "ansible",
        "aliases": ["Ansible", "ansible"],
        "extensions": [".yml", ".yaml"],
        "configuration": "./language-configuration.json"
      }
    ],
    "grammars": [
      {
        "language": "ansible",
        "scopeName": "source.yaml.ansible",
        "path": "./syntaxes/ansible.tmLanguage.json"
      }
    ]
  }
}
```

### Dependencies
```toml
tower-lsp = "0.20"
tokio = { version = "1.35", features = ["full", "io-util"] }
```

## Related Issues
- #004: Rich Error Messages
- #006: Pre-execution Validation
- #007: Module Schema Validation

## Additional Notes
This is a **P1 (High)** feature that provides significant competitive advantage. Neither Ansible nor Terraform have first-class LSP support. Should be targeted for v0.1.x MVP with v0.2.x full feature set.
