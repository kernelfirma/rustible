//! Template Lookup Plugin
//!
//! Renders a template file using MiniJinja and returns the rendered string.
//! Similar to Ansible's `template` lookup plugin.
//!
//! # Usage
//!
//! ```yaml
//! # Render a template file
//! content: "{{ lookup('template', '/path/to/template.j2') }}"
//! ```
//!
//! The template is rendered with the variables available in the lookup context.

use super::{Lookup, LookupContext, LookupError, LookupResult};
use minijinja::Environment;
use std::fs;
use std::path::{Path, PathBuf};

/// Template lookup plugin for rendering Jinja2 template files via MiniJinja
#[derive(Debug, Clone, Default)]
pub struct TemplateLookup;

impl TemplateLookup {
    /// Create a new TemplateLookup instance
    pub fn new() -> Self {
        Self
    }

    /// Resolve a path relative to the base directory if provided
    fn resolve_path(&self, path: &str, context: &LookupContext) -> PathBuf {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else if let Some(ref base) = context.base_dir {
            base.join(&path)
        } else {
            path
        }
    }

    /// Validate that a path is safe to read
    fn validate_path(&self, path: &Path) -> LookupResult<()> {
        if path.to_string_lossy().contains('\0') {
            return Err(LookupError::InvalidArguments(
                "Path contains null byte".to_string(),
            ));
        }
        Ok(())
    }
}

impl Lookup for TemplateLookup {
    fn name(&self) -> &'static str {
        "template"
    }

    fn description(&self) -> &'static str {
        "Renders a template file using MiniJinja and returns the rendered string"
    }

    fn lookup(&self, args: &[&str], context: &LookupContext) -> LookupResult<Vec<String>> {
        if args.is_empty() {
            return Err(LookupError::MissingArgument(
                "template file path required".to_string(),
            ));
        }

        let mut results = Vec::new();

        for arg in args {
            // Skip option arguments
            if arg.contains('=') {
                continue;
            }

            let path = self.resolve_path(arg, context);
            self.validate_path(&path)?;

            // Read the template file
            let template_content = fs::read_to_string(&path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    LookupError::FileNotFound(path.clone())
                } else if e.kind() == std::io::ErrorKind::PermissionDenied {
                    LookupError::PermissionDenied(path.display().to_string())
                } else {
                    LookupError::Io(e)
                }
            })?;

            // Render the template using MiniJinja
            let mut env = Environment::new();
            env.add_template("_lookup", &template_content)
                .map_err(|e| LookupError::ParseError(format!("Template parse error: {}", e)))?;

            let tmpl = env
                .get_template("_lookup")
                .map_err(|e| LookupError::Other(format!("Failed to get template: {}", e)))?;

            let rendered = tmpl.render(&context.vars).map_err(|e| {
                LookupError::Other(format!("Template render error: {}", e))
            })?;

            results.push(rendered);
        }

        if results.is_empty() && context.fail_on_error {
            return Err(LookupError::Other(
                "No templates could be rendered".to_string(),
            ));
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_template_lookup_basic() {
        let mut temp = NamedTempFile::new().unwrap();
        write!(temp, "Hello, {{{{ name }}}}!").unwrap();

        let lookup = TemplateLookup::new();
        let mut vars = HashMap::new();
        vars.insert(
            "name".to_string(),
            serde_json::Value::String("World".to_string()),
        );
        let context = LookupContext::new().with_vars(vars);

        let result = lookup
            .lookup(&[temp.path().to_str().unwrap()], &context)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Hello, World!");
    }

    #[test]
    fn test_template_lookup_no_variables() {
        let mut temp = NamedTempFile::new().unwrap();
        write!(temp, "Static content with no variables").unwrap();

        let lookup = TemplateLookup::new();
        let context = LookupContext::default();

        let result = lookup
            .lookup(&[temp.path().to_str().unwrap()], &context)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Static content with no variables");
    }

    #[test]
    fn test_template_lookup_missing_path() {
        let lookup = TemplateLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&[], &context);
        assert!(matches!(result, Err(LookupError::MissingArgument(_))));
    }

    #[test]
    fn test_template_lookup_file_not_found() {
        let lookup = TemplateLookup::new();
        let context = LookupContext::default();

        let result = lookup.lookup(&["/nonexistent/template.j2"], &context);
        assert!(matches!(result, Err(LookupError::FileNotFound(_))));
    }

    #[test]
    fn test_template_lookup_with_jinja_constructs() {
        let mut temp = NamedTempFile::new().unwrap();
        write!(
            temp,
            "{{% for item in items %}}{{{{ item }}}} {{% endfor %}}"
        )
        .unwrap();

        let lookup = TemplateLookup::new();
        let mut vars = HashMap::new();
        vars.insert(
            "items".to_string(),
            serde_json::json!(["a", "b", "c"]),
        );
        let context = LookupContext::new().with_vars(vars);

        let result = lookup
            .lookup(&[temp.path().to_str().unwrap()], &context)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "a b c ");
    }
}
