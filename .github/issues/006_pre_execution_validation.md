# DX: Implement Pre-execution Validation with Schema Checking

## Problem Statement
Rustible currently validates playbooks at execution time, which means errors are discovered late and require re-running entire playbooks. Pre-execution validation would catch errors early, similar to `terraform validate`.

## Current State
- Validation happens during execution
- YAML syntax errors caught early
- Module errors caught late (at execution time)
- No schema-based validation
- No static analysis of playbook logic

## Proposed Solution

### Phase 1: Schema-based Validation (v0.1.x)
1. **JSON Schema generation for modules**
   ```rust
   // src/validation/schema_validator.rs
   pub struct ModuleSchemaGenerator {
       registry: Arc<ModuleRegistry>,
   }
   
   impl ModuleSchemaGenerator {
       pub fn generate_schema(&self, module_name: &str) -> Result<JSONSchema> {
           let module = self.registry.get(module_name)?;
           
           let mut properties = json::Map::new();
           for arg in module.arguments() {
               properties.insert(
                   arg.name.clone(),
                   self.arg_to_schema(arg)?
               );
           }
           
           Ok(JSONSchema {
               r#type: Some("object".to_string()),
               properties: Some(properties),
               required: Some(
                   module.arguments()
                       .iter()
                       .filter(|arg| arg.required)
                       .map(|arg| arg.name.clone())
                       .collect()
               ),
               ..Default::default()
           })
       }
   }
   ```

2. **Playbook validation command**
   ```bash
   rustible validate playbook.yml
   ```

   Output:
   ```
   ✓ Syntax: Valid
   ✓ Inventory: All hosts found
   ✗ Module arguments: 3 errors
   
   Error in playbook.yml:15:23
     Module 'apt' requires argument 'name'
     
   Error in playbook.yml:20:15
     Module 'file' has invalid type for 'mode': expected string, got integer
     
   Error in playbook.yml:25:10
     Unknown module 'unknown_module'
     Did you mean 'systemd_module'?
   ```

### Phase 2: Static Analysis (v0.2.x)
1. **Variable reference validation**
   ```rust
   pub struct VariableValidator {
       variables: HashSet<String>,
       inventory: Option<Inventory>,
   }
   
   impl VariableValidator {
       pub fn validate_references(&self, playbook: &Playbook) -> Vec<ValidationError> {
           let mut errors = Vec::new();
           
           for task in playbook.all_tasks() {
               for ref in task.variable_references() {
                   if !self.variables.contains(ref) {
                       errors.push(ValidationError {
                           location: task.location.clone(),
                           message: format!("Undefined variable: {}", ref),
                           suggestion: self.suggest_variable(ref),
                       });
                   }
               }
           }
           
           errors
       }
   }
   ```

2. **Dependency cycle detection**
   - Detect circular dependencies in handlers
   - Detect circular role includes
   - Validate task ordering

3. **Best practices checking**
   - Check for common anti-patterns
   - Suggest optimizations
   - Security warnings (e.g., `shell` module with untrusted input)

### Phase 3: Integration Points (v0.2.x)
1. **CI/CD validation**
   ```yaml
   # .github/workflows/rustible-validate.yml
   name: Validate Playbooks
   on: [push, pull_request]
   jobs:
     validate:
       runs-on: ubuntu-latest
       steps:
         - uses: actions/checkout@v3
         - uses: rustible/install-action@v1
         - run: rustible validate playbooks/
   ```

2. **Pre-commit hook**
   ```bash
   # .pre-commit-config.yaml
   repos:
     - repo: local
       hooks:
         - id: rustible-validate
           name: Validate playbooks
           entry: rustible validate
           language: system
           files: \.ya?ml$
   ```

3. **IDE integration**
   - Run validation on file save
   - Show errors in editor
   - Provide quick fixes

## Expected Outcomes
- Early error detection (before execution)
- Faster feedback loop
- Reduced debugging time
- Better CI/CD integration
- Improved playbook quality

## Success Criteria
- [ ] Schema generation for all built-in modules
- [ ] Playbook validation command implemented
- [ ] Variable reference validation
- [ ] Dependency cycle detection
- [ ] Best practices linter
- [ ] CI/CD integration examples
- [ ] Pre-commit hook support
- [ ] IDE validation integration
- [ ] 100% of syntax errors caught before execution
- [ ] 80% of semantic errors caught before execution

## Implementation Details

### Validation Report
```rust
pub struct ValidationReport {
    pub syntax: ValidationStatus,
    pub inventory: ValidationStatus,
    pub modules: ValidationStatus,
    pub variables: ValidationStatus,
    pub dependencies: ValidationStatus,
    pub best_practices: ValidationStatus,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub suggestions: Vec<Suggestion>,
}

pub struct ValidationError {
    pub location: Location,
    pub message: String,
    pub code: String,
    pub suggestion: Option<String>,
    pub severity: ErrorSeverity,
}
```

### Configuration
```toml
[validation]
strict_mode = false
check_best_practices = true
check_security = true
fail_on_warnings = false
```

## Related Issues
- #004: Rich Error Messages
- #005: LSP Server Implementation
- #007: Module Schema Validation

## Additional Notes
This is a **P1 (High)** feature that significantly improves developer experience. Should be targeted for v0.1.x MVP with v0.2.x full feature set.
