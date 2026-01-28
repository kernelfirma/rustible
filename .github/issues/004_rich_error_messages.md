# DX: Implement Rich Diagnostic Error Messages with Source Code Context

## Problem Statement
Rustible currently lacks comprehensive error diagnostics that show source code context, suggestions, and helpful hints. This makes debugging difficult, especially for users migrating from Ansible who expect better error messages.

## Current State
- Basic error messages via `thiserror`
- Error chaining with `#[source]`
- No source code snippets in errors
- No suggestions or hints
- No IDE-friendly error reporting

## Ansible's Problem (What We're Improving Upon)
- Cryptic errors like `"The conditional check 'item != openshift_ca_host' failed"`
- Error location says "may be elsewhere in the file"
- No variable suggestions
- Difficult to debug without deep Python knowledge

## Proposed Solution

### Phase 1: Basic Rich Errors (v0.1.x)
1. **Implement ariadne for error reporting**
   ```rust
   // src/diagnostics/rich_errors.rs
   use ariadne::{Color, Label, Report, ReportKind, Source};
   
   pub struct RichDiagnostic {
       pub kind: DiagnosticKind,
       pub message: String,
       pub file: PathBuf,
       pub span: Span,
       pub suggestions: Vec<Suggestion>,
       pub related: Vec<RelatedInfo>,
   }
   
   impl RichDiagnostic {
       pub fn render(&self) -> String {
           let source = std::fs::read_to_string(&self.file)?;
           
           Report::build(ReportKind::Error, &self.file, self.span.start)
               .with_code(self.error_code())
               .with_message(&self.message)
               .with_label(
                   Label::new((&self.file, self.span.clone()))
                       .with_message(&self.hint())
                       .with_color(Color::Red)
               )
               .with_help(self.suggestions.first().map(|s| s.text.as_str()).unwrap_or(""))
               .finish()
               .write_to_string(&mut Source::from(source))
       }
   }
   ```

2. **Error categories**
   - Syntax errors (YAML parsing)
   - Validation errors (module arguments)
   - Execution errors (runtime failures)
   - Connection errors (SSH/network issues)
   - Variable errors (undefined, type mismatches)

### Phase 2: Smart Suggestions (v0.2.x)
1. **Variable name suggestions**
   ```rust
   impl RichDiagnostic {
       fn suggest_variable_name(&self, undefined: &str) -> Option<String> {
           let available_vars = self.get_available_variables();
           let suggestion = available_vars
               .iter()
               .min_by_key(|v| levenshtein_distance(undefined, v))?;
           
           Some(format!("Did you mean '{}'?", suggestion))
       }
   }
   ```

2. **Module name suggestions**
   - Levenshtein distance matching
   - Show similar module names
   - Display module category

3. **Argument validation**
   - Show expected vs actual types
   - List required arguments
   - Suggest argument names for typos

### Phase 3: IDE Integration (v0.3.x)
1. **LSP error diagnostics**
   ```rust
   // src/lsp/diagnostics.rs
   pub struct RustibleLSPDiagnostics {
       server: LanguageServer,
   }
   
   impl RustibleLSPDiagnostics {
       pub fn publish_diagnostics(&self, uri: Url, errors: Vec<RichDiagnostic>) {
           let diagnostics = errors.into_iter().map(|err| Diagnostic {
               range: err.span_to_range(),
               severity: DiagnosticSeverity::ERROR,
               message: err.message,
               related_information: err.related,
               suggestions: err.suggestions,
           }).collect();
           
           self.server.publish_diagnostics(uri, diagnostics).await;
       }
   }
   ```

2. **Error code documentation**
   - Unique error codes for each error type
   - Link to documentation from error messages
   - Searchable error database

## Expected Outcomes
- Clear, actionable error messages
- Source code context in error output
- Helpful suggestions for common mistakes
- IDE-friendly error reporting
- Reduced debugging time

## Success Criteria
- [ ] Ariadne integrated for error rendering
- [ ] All error types support rich diagnostics
- [ ] Variable name suggestions implemented
- [ ] Module name suggestions implemented
- [ ] Type mismatch errors show expected vs actual
- [ ] LSP diagnostics support added
- [ ] Error code documentation complete

## Example Output

### Before
```
error: undefined variable 'wrong_var'
```

### After
```
error[E0042]: undefined variable 'wrong_var'
  --> playbook.yml:15:23
   |
15 |       msg: "{{ wrong_var }}"
   |                 ^^^^^^^^^ not defined in this scope
   |
   = help: did you mean 'var1'?
   = note: available variables: var1, ansible_hostname, inventory_hostname, host_vars
   = see: https://rustible.dev/errors/E0042.html
```

## Implementation Details

### Error Code Format
```
E0XXX: Category-specific errors
  E01XX: Parsing errors
  E02XX: Validation errors
  E03XX: Execution errors
  E04XX: Variable errors
  E05XX: Connection errors
```

### Dependencies
```toml
ariadne = "0.5"
strsim = "0.11"  # For string similarity
```

## Related Issues
- #005: LSP Server Implementation
- #006: Pre-execution Validation
- #007: Module Schema Validation

## Additional Notes
This is a **P0 (Critical)** feature as developer experience is a key differentiator from Ansible. Should be prioritized for v0.1.x release.
