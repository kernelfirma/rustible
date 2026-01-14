//! Include_vars module - Load variables from YAML/JSON files into playbook scope
//!
//! This module loads variables from external files during playbook execution.
//! Variables are loaded at IncludeVars precedence level (16), which is higher
//! than most variable sources but lower than set_facts and extra_vars.

use super::{
    Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParallelizationHint, ParamExt,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Module for loading variables from files
pub struct IncludeVarsModule;

impl IncludeVarsModule {
    /// Load variables from a single file
    fn load_from_file(&self, path: &Path) -> ModuleResult<HashMap<String, Value>> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to read file {}: {}", path.display(), e))
        })?;

        // Try to parse as YAML first, fallback to JSON if that fails
        let vars: HashMap<String, serde_yaml::Value> = match serde_yaml::from_str(&content) {
            Ok(v) => v,
            Err(yaml_err) => {
                // YAML failed, try JSON
                match serde_json::from_str::<HashMap<String, Value>>(&content) {
                    Ok(json_vars) => {
                        // Convert JSON values to YAML values
                        let mut yaml_vars = HashMap::new();
                        for (key, value) in json_vars {
                            let yaml_value = serde_yaml::to_value(&value).map_err(|e| {
                                ModuleError::ParseError(format!(
                                    "Failed to convert JSON to YAML: {}",
                                    e
                                ))
                            })?;
                            yaml_vars.insert(key, yaml_value);
                        }
                        yaml_vars
                    }
                    Err(_json_err) => {
                        return Err(ModuleError::ParseError(format!(
                            "Failed to parse {} as YAML or JSON: {}",
                            path.display(),
                            yaml_err
                        )));
                    }
                }
            }
        };

        // Convert YAML values to JSON values for compatibility
        let mut json_vars = HashMap::new();
        for (key, value) in vars {
            let json_value = serde_json::to_value(&value).map_err(|e| {
                ModuleError::ParseError(format!("Failed to convert variable {}: {}", key, e))
            })?;
            json_vars.insert(key, json_value);
        }

        Ok(json_vars)
    }

    /// Load variables from all files in a directory
    fn load_from_directory(&self, dir_path: &Path) -> ModuleResult<HashMap<String, Value>> {
        if !dir_path.is_dir() {
            return Err(ModuleError::InvalidParameter(format!(
                "{} is not a directory",
                dir_path.display()
            )));
        }

        let mut all_vars = HashMap::new();

        // Read directory entries
        let entries = std::fs::read_dir(dir_path).map_err(|e| {
            ModuleError::ExecutionFailed(format!(
                "Failed to read directory {}: {}",
                dir_path.display(),
                e
            ))
        })?;

        // Sort entries by name for predictable ordering
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && (p.extension() == Some("yml".as_ref())
                        || p.extension() == Some("yaml".as_ref())
                        || p.extension() == Some("json".as_ref()))
            })
            .collect();

        files.sort();

        // Load each file and merge variables
        for file_path in files {
            let file_vars = self.load_from_file(&file_path)?;
            all_vars.extend(file_vars);
        }

        Ok(all_vars)
    }

    /// Resolve a path relative to the playbook or as absolute
    fn resolve_path(&self, path: &str, context: &ModuleContext) -> ModuleResult<PathBuf> {
        let path_buf = PathBuf::from(path);

        // If absolute path, use it directly
        if path_buf.is_absolute() {
            if !path_buf.exists() {
                return Err(ModuleError::ExecutionFailed(format!(
                    "File or directory not found: {}",
                    path_buf.display()
                )));
            }
            return Ok(path_buf);
        }

        // Relative path: resolve from work_dir or current directory
        let base_dir = if let Some(work_dir) = &context.work_dir {
            PathBuf::from(work_dir)
        } else {
            std::env::current_dir().map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to get current directory: {}", e))
            })?
        };

        let resolved = base_dir.join(path_buf);
        if !resolved.exists() {
            return Err(ModuleError::ExecutionFailed(format!(
                "File or directory not found: {}",
                resolved.display()
            )));
        }

        Ok(resolved)
    }
}

impl Module for IncludeVarsModule {
    fn name(&self) -> &'static str {
        "include_vars"
    }

    fn description(&self) -> &'static str {
        "Load variables from YAML or JSON files into the playbook scope"
    }

    fn classification(&self) -> ModuleClassification {
        // LocalLogic because this runs on the control node only
        // It reads variable files but doesn't need remote host access
        ModuleClassification::LocalLogic
    }

    fn parallelization_hint(&self) -> ParallelizationHint {
        // Can run in parallel since it only reads local files
        ParallelizationHint::FullyParallel
    }

    fn required_params(&self) -> &[&'static str] {
        // Either 'file' or 'dir' is required, but we validate that separately
        &[]
    }

    fn validate_params(&self, params: &ModuleParams) -> ModuleResult<()> {
        // Must have either 'file' or 'dir' parameter
        let has_file = params.contains_key("file");
        let has_dir = params.contains_key("dir");

        if !has_file && !has_dir {
            return Err(ModuleError::InvalidParameter(
                "Either 'file' or 'dir' parameter is required".to_string(),
            ));
        }

        if has_file && has_dir {
            return Err(ModuleError::InvalidParameter(
                "Cannot specify both 'file' and 'dir' parameters".to_string(),
            ));
        }

        // Validate depth if present
        if let Some(depth) = params.get_i64("depth")? {
            if depth < 0 {
                return Err(ModuleError::InvalidParameter(
                    "depth parameter must be non-negative".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        // Note: The actual variable loading into VarStore happens in the executor
        // because this module needs to modify the RuntimeContext's variable store.
        // This module implementation validates parameters and returns structured data
        // that the executor can use.

        let vars: HashMap<String, Value>;
        let source: String;

        if let Some(file_path) = params.get_string("file")? {
            // Load from a single file
            let resolved_path = self.resolve_path(&file_path, context)?;
            vars = self.load_from_file(&resolved_path)?;
            source = resolved_path.display().to_string();
        } else if let Some(dir_path) = params.get_string("dir")? {
            // Load from all files in a directory
            let resolved_path = self.resolve_path(&dir_path, context)?;
            vars = self.load_from_directory(&resolved_path)?;
            source = format!("{}/*.yml", resolved_path.display());
        } else {
            // This shouldn't happen due to validate_params, but handle it anyway
            return Err(ModuleError::InvalidParameter(
                "Either 'file' or 'dir' parameter is required".to_string(),
            ));
        }

        // Check if we should scope variables under a name
        let scoped_vars = if let Some(name) = params.get_string("name")? {
            let mut scoped = HashMap::new();
            scoped.insert(name.clone(), Value::Object(vars.into_iter().collect()));
            scoped
        } else {
            vars
        };

        let var_count = if params.get_string("name")?.is_some() {
            1 // All vars scoped under one key
        } else {
            scoped_vars.len()
        };

        let message = format!("Loaded {} variable(s) from {}", var_count, source);

        // include_vars doesn't change system state, but by convention it's reported as "ok"
        let mut output = ModuleOutput::ok(message);
        output.data = scoped_vars;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_include_vars_validation() {
        let module = IncludeVarsModule;

        // Valid: file parameter
        let mut params: ModuleParams = HashMap::new();
        params.insert("file".to_string(), Value::String("vars.yml".to_string()));
        assert!(module.validate_params(&params).is_ok());

        // Valid: dir parameter
        let mut params: ModuleParams = HashMap::new();
        params.insert("dir".to_string(), Value::String("vars/".to_string()));
        assert!(module.validate_params(&params).is_ok());

        // Invalid: no parameters
        let empty_params: ModuleParams = HashMap::new();
        assert!(module.validate_params(&empty_params).is_err());

        // Invalid: both file and dir
        let mut params: ModuleParams = HashMap::new();
        params.insert("file".to_string(), Value::String("vars.yml".to_string()));
        params.insert("dir".to_string(), Value::String("vars/".to_string()));
        assert!(module.validate_params(&params).is_err());
    }

    #[test]
    fn test_include_vars_from_yaml_file() {
        let module = IncludeVarsModule;
        let temp_dir = TempDir::new().unwrap();
        let vars_file = temp_dir.path().join("test_vars.yml");

        // Create test YAML file
        let mut file = std::fs::File::create(&vars_file).unwrap();
        write!(
            file,
            r#"
app_name: "my_app"
app_version: "1.0.0"
app_port: 8080
app_debug: true
"#
        )
        .unwrap();

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "file".to_string(),
            Value::String(vars_file.to_str().unwrap().to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Loaded 4 variable(s)"));
        assert!(result.data.contains_key("app_name"));
        assert!(result.data.contains_key("app_version"));
        assert!(result.data.contains_key("app_port"));
        assert!(result.data.contains_key("app_debug"));

        // Verify values
        assert_eq!(
            result.data.get("app_name"),
            Some(&Value::String("my_app".to_string()))
        );
        assert_eq!(
            result.data.get("app_port"),
            Some(&Value::Number(8080.into()))
        );
    }

    #[test]
    fn test_include_vars_from_json_file() {
        let module = IncludeVarsModule;
        let temp_dir = TempDir::new().unwrap();
        let vars_file = temp_dir.path().join("test_vars.json");

        // Create test JSON file
        let mut file = std::fs::File::create(&vars_file).unwrap();
        write!(
            file,
            r#"{{
    "database_host": "localhost",
    "database_port": 5432,
    "database_name": "mydb"
}}"#
        )
        .unwrap();

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "file".to_string(),
            Value::String(vars_file.to_str().unwrap().to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("database_host"));
        assert!(result.data.contains_key("database_port"));
        assert!(result.data.contains_key("database_name"));
    }

    #[test]
    fn test_include_vars_from_directory() {
        let module = IncludeVarsModule;
        let temp_dir = TempDir::new().unwrap();
        let vars_dir = temp_dir.path().join("vars");
        std::fs::create_dir(&vars_dir).unwrap();

        // Create multiple variable files
        let file1 = vars_dir.join("app.yml");
        let mut f = std::fs::File::create(&file1).unwrap();
        write!(f, "app_name: 'test'\napp_version: '1.0'").unwrap();

        let file2 = vars_dir.join("database.yml");
        let mut f = std::fs::File::create(&file2).unwrap();
        write!(f, "db_host: 'localhost'\ndb_port: 5432").unwrap();

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "dir".to_string(),
            Value::String(vars_dir.to_str().unwrap().to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("app_name"));
        assert!(result.data.contains_key("app_version"));
        assert!(result.data.contains_key("db_host"));
        assert!(result.data.contains_key("db_port"));
    }

    #[test]
    fn test_include_vars_with_name_scoping() {
        let module = IncludeVarsModule;
        let temp_dir = TempDir::new().unwrap();
        let vars_file = temp_dir.path().join("scoped.yml");

        let mut file = std::fs::File::create(&vars_file).unwrap();
        write!(
            file,
            r#"
key1: "value1"
key2: "value2"
"#
        )
        .unwrap();

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "file".to_string(),
            Value::String(vars_file.to_str().unwrap().to_string()),
        );
        params.insert("name".to_string(), Value::String("config".to_string()));

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.msg.contains("Loaded 1 variable(s)"));
        assert!(result.data.contains_key("config"));

        // Verify scoping
        if let Some(Value::Object(config)) = result.data.get("config") {
            assert!(config.contains_key("key1"));
            assert!(config.contains_key("key2"));
        } else {
            panic!("Expected config to be an object");
        }
    }

    #[test]
    fn test_include_vars_file_not_found() {
        let module = IncludeVarsModule;

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "file".to_string(),
            Value::String("/nonexistent/file.yml".to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context);

        assert!(result.is_err());
        assert!(matches!(result, Err(ModuleError::ExecutionFailed(_))));
    }

    #[test]
    fn test_include_vars_invalid_yaml() {
        let module = IncludeVarsModule;
        let temp_dir = TempDir::new().unwrap();
        let vars_file = temp_dir.path().join("invalid.yml");

        let mut file = std::fs::File::create(&vars_file).unwrap();
        write!(file, "invalid: yaml: content: here: ::::").unwrap();

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "file".to_string(),
            Value::String(vars_file.to_str().unwrap().to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context);

        assert!(result.is_err());
        assert!(matches!(result, Err(ModuleError::ParseError(_))));
    }

    #[test]
    fn test_include_vars_complex_structure() {
        let module = IncludeVarsModule;
        let temp_dir = TempDir::new().unwrap();
        let vars_file = temp_dir.path().join("complex.yml");

        let mut file = std::fs::File::create(&vars_file).unwrap();
        write!(
            file,
            r#"
database:
  host: "localhost"
  port: 5432
  credentials:
    username: "admin"
    password: "secret"

servers:
  - name: "web1"
    ip: "192.168.1.10"
  - name: "web2"
    ip: "192.168.1.11"
"#
        )
        .unwrap();

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "file".to_string(),
            Value::String(vars_file.to_str().unwrap().to_string()),
        );

        let context = ModuleContext::default();
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
        assert!(result.data.contains_key("database"));
        assert!(result.data.contains_key("servers"));
    }

    #[test]
    fn test_include_vars_check_mode() {
        let module = IncludeVarsModule;
        let temp_dir = TempDir::new().unwrap();
        let vars_file = temp_dir.path().join("check.yml");

        let mut file = std::fs::File::create(&vars_file).unwrap();
        write!(file, "test_var: 'test_value'").unwrap();

        let mut params: ModuleParams = HashMap::new();
        params.insert(
            "file".to_string(),
            Value::String(vars_file.to_str().unwrap().to_string()),
        );

        let context = ModuleContext::default().with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        // In check mode, include_vars still works
        assert!(!result.changed);
        assert!(result.data.contains_key("test_var"));
    }

    #[test]
    fn test_include_vars_module_classification() {
        let module = IncludeVarsModule;
        assert_eq!(module.classification(), ModuleClassification::LocalLogic);
        assert_eq!(
            module.parallelization_hint(),
            ParallelizationHint::FullyParallel
        );
    }
}
