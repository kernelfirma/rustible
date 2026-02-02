# Error Handling and Type Safety in Rustible

Rustible catches configuration errors **before** execution through compile-time schema validation, pre-flight checks, and safe parameter extraction. When errors do occur, structured diagnostics with source-context rendering pinpoint the exact location and suggest fixes.

## Compile-Time Validation

### Schema Validation (`SchemaValidator`)

`SchemaValidator` (`src/parser/schema.rs`) validates playbook structure and module arguments against JSON schemas before any task runs:

- **Required arguments** — verifies mandatory parameters are present (e.g. `copy` requires `src` and `dest`)
- **Mutually exclusive options** — rejects conflicting parameters in the same task
- **Type checking** — ensures values match expected types (string, bool, int, list, dict, path)
- **Deprecated syntax** — warns about outdated patterns with migration guidance
- **Built-in schemas** for core modules: `file`, `copy`, `template`, `service`, `package`, `debug`, `set_fact`, `command`, `shell`

### Pre-Flight Validator

`Validator` (`src/validation/mod.rs`) performs a full pre-execution sweep of playbooks:

| Check | What it catches |
|---|---|
| YAML syntax | Structural errors, bad indentation, invalid types |
| Module arguments | Unknown parameters, wrong types, missing required args |
| Undefined variables | References to variables not in scope, with typo suggestions |
| Handler references | `notify:` pointing to non-existent handlers |
| Deprecated modules | Modules marked for removal, with replacement suggestions |

Validation is configurable via `ValidationConfig` — you can set strictness levels and enable/disable individual checks.

## Type Safety Features

### Parameter Definitions (`ParamDef`)

`ParamDef` (`src/lint/params.rs`) provides structured parameter definitions for modules:

```rust
ParamDef::new("src")
    .required()
    .with_type(ParamType::Path)
    .with_choices(&[])
    .with_aliases(&["source"])
```

Supported types: `String`, `Bool`, `Int`, `Float`, `List`, `Dict`, `Path`, `Raw`.

### Input Validation

Security-focused validation functions prevent injection attacks at system boundaries:

**Path safety** (`src/security/path.rs`):
- `validate_path_no_traversal()` — rejects `..`, null bytes, newlines
- `validate_path_within_base()` — ensures paths don't escape a base directory
- `validate_path_strict()` — comprehensive check with length limits
- `check_sensitive_path()` — blocks access to `/etc/shadow`, `/root/.ssh`, etc.
- `sanitize_filename()` — removes dangerous characters

**Command injection prevention** (`src/security/validation.rs`):
- `BecomeValidator::validate_username()` — POSIX compliance + shell metacharacter detection
- `BecomeValidator::validate_path()` — shell safety checks for escalation paths
- `BecomeValidator::validate_env_value()` — newline/null byte injection detection
- `BecomeValidator::validate_flags()` — detects dangerous patterns in privilege escalation flags

**General input** (`src/security/input.rs`):
- `validate_hostname()`, `validate_identifier()`, `validate_url()`, `sanitize_shell_arg()`

All validation functions return `SecurityResult<T>`.

## Rich Error Reporting

### Source-Context Rendering

`RichDiagnostic` (`src/diagnostics/rich_errors.rs`) uses [ariadne](https://crates.io/crates/ariadne) to render errors with source context, pointing at the exact line and column:

```
error[E0003]: Undefined variable 'wrong_port': not found in scope
  --> playbook.yml:21:25
   |
21 |         msg: "Port is {{ wrong_port }}"
   |                         ^^^^^^^^^^ undefined variable
   |
   = note: Did you mean 'http_port' ?
   = help: Define the variable in vars, inventory, or extra-vars
```

Features:
- **Severity levels**: Error, Warning, Note, Help — each with distinct colors
- **Error codes**: Registry of known codes (`E0001` through `E0030`) with explanations, causes, and fixes
- **Primary and secondary labels**: Highlight multiple related spans in one report
- **Notes and help text**: Contextual guidance attached to each diagnostic

### Auto-Fix Suggestions

The `Suggestion` type attaches actionable fixes to diagnostics, with optional span and replacement text. When rendered, suggestions show a unified diff of the proposed change:

```
Suggestions:
  1. Replace tab with spaces

@@ line 3 @@
- 	hosts: all
+   hosts: all
```

Convenience builders automatically detect fixable patterns:
- `yaml_syntax_error()` — detects tabs (suggests spaces) and missing colons
- `template_syntax_error()` — detects unclosed `{{`/`{%` delimiters
- `undefined_variable_error()` — suggests similar variable names via Levenshtein distance
- `module_not_found_error()` — suggests similar module names
- `missing_required_arg_error()` — shows example YAML for the missing parameter
- `connection_error()` — pattern-matches on "refused", "timeout", "permission denied" to give targeted advice

### Error Code Registry

`ErrorCodeRegistry` maps error codes to detailed explanations:

| Code | Meaning |
|------|---------|
| E0001 | YAML syntax error |
| E0002 | Template syntax error |
| E0003 | Undefined variable |
| E0004 | Module not found |
| E0010 | Tabs in YAML |
| E0020 | Missing required module argument |
| E0030 | Invalid module argument |

Each entry includes: title, explanation, common causes, and suggested fixes.

## Comparison with Ansible

| Aspect | Ansible | Rustible |
|--------|---------|----------|
| **When errors appear** | At runtime, mid-execution | Pre-execution via schema and pre-flight validation |
| **Error format** | Python tracebacks and string messages | Structured diagnostics with source context and spans |
| **Variable typos** | Runtime `undefined variable` error after tasks may have already run | Caught before execution with Levenshtein-based "did you mean?" suggestions |
| **Module arguments** | Runtime `Unsupported parameters` error | Schema-validated before any task executes |
| **Fix suggestions** | None — user must diagnose manually | Auto-fix `Suggestion` with diff preview |
| **Error types** | Untyped string errors | Typed `RichDiagnostic` with severity, code, span, labels |
| **Security validation** | Limited (some basic checks) | Comprehensive path traversal, command injection, and input validation |

Rustible's approach means a playbook that will fail is rejected immediately with precise diagnostics, rather than failing partway through execution and leaving the system in a partially-configured state.
