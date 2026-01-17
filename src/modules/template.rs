//! Template module - Render templates with Minijinja (Jinja2 compatible)
//!
//! This module renders Jinja2 templates and copies the result
//! to a destination file. Supports both local and remote execution via async connections.

use super::{
    Diff, Module, ModuleClassification, ModuleContext, ModuleError, ModuleOutput, ModuleParams,
    ModuleResult, ParamExt,
};
use crate::connection::TransferOptions;
use crate::template::TEMPLATE_ENGINE;
use crate::utils::shell_escape;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Module for rendering templates
pub struct TemplateModule;

impl TemplateModule {
    fn build_context(
        context: &ModuleContext,
        extra_vars: Option<&serde_json::Value>,
    ) -> serde_json::Value {
        let mut ctx_map = serde_json::Map::new();

        // Add variables
        for (key, value) in &context.vars {
            ctx_map.insert(key.clone(), value.clone());
        }

        // Add facts
        ctx_map.insert(
            "ansible_facts".to_string(),
            serde_json::json!(&context.facts),
        );
        for (key, value) in &context.facts {
            ctx_map.insert(key.clone(), value.clone());
        }

        // Add extra variables if provided
        if let Some(serde_json::Value::Object(vars)) = extra_vars {
            for (key, value) in vars {
                ctx_map.insert(key.clone(), value.clone());
            }
        }

        serde_json::Value::Object(ctx_map)
    }

    async fn execute_remote(
        conn: Arc<dyn crate::connection::Connection + Send + Sync>,
        dest_path: PathBuf,
        dest: String,
        src_name: String,
        rendered: String,
        backup: bool,
        backup_suffix: String,
        mode: Option<u32>,
        check_mode: bool,
        diff_mode: bool,
    ) -> ModuleResult<ModuleOutput> {
        let current_content = if conn.path_exists(&dest_path).await.unwrap_or(false) {
            conn.download_content(&dest_path)
                .await
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        } else {
            None
        };

        let needs_update = match &current_content {
            Some(content) => content != &rendered,
            None => true,
        };

        if !needs_update {
            // Check if only permissions need updating
            let perm_changed = if let Some(m) = mode {
                if let Ok(stat) = conn.stat(&dest_path).await {
                    (stat.mode & 0o7777) != m
                } else {
                    false
                }
            } else {
                false
            };

            if perm_changed {
                if check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would change permissions on '{}'",
                        dest
                    )));
                }
                // Set permissions via chmod command on remote
                let chmod_cmd = format!("chmod {:o} {}", mode.unwrap(), shell_escape(&dest));
                conn.execute(&chmod_cmd, None).await.map_err(|e| {
                    ModuleError::ExecutionFailed(format!("Failed to set permissions: {}", e))
                })?;
                return Ok(ModuleOutput::changed(format!(
                    "Changed permissions on '{}'",
                    dest
                )));
            }

            return Ok(ModuleOutput::ok(format!(
                "Template '{}' is already up to date",
                dest
            )));
        }

        // In check mode, return what would happen
        if check_mode {
            let diff = if diff_mode {
                let before = current_content.unwrap_or_default();
                Some(Diff::new(before, rendered.clone()))
            } else {
                None
            };

            let mut output = ModuleOutput::changed(format!(
                "Would render template '{}' to '{}'",
                src_name, dest
            ));

            if let Some(d) = diff {
                output = output.with_diff(d);
            }

            return Ok(output);
        }

        // Create backup if requested (via remote command)
        let backup_file = if backup && current_content.is_some() {
            let backup_path = format!("{}{}", dest, backup_suffix);
            let cp_cmd = format!("cp {} {}", shell_escape(&dest), shell_escape(&backup_path));
            conn.execute(&cp_cmd, None).await.map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to create backup: {}", e))
            })?;
            Some(backup_path)
        } else {
            None
        };

        // Build transfer options
        let transfer_opts = TransferOptions {
            mode,
            create_dirs: true,
            backup: false, // We already handled backup above
            ..Default::default()
        };

        // Upload rendered content to remote
        conn.upload_content(rendered.as_bytes(), &dest_path, Some(transfer_opts))
            .await
            .map_err(|e| ModuleError::ExecutionFailed(format!("Failed to upload template: {}", e)))?;

        let mut output =
            ModuleOutput::changed(format!("Rendered template '{}' to '{}'", src_name, dest));

        if let Some(backup_path) = backup_file {
            output = output.with_data("backup_file", serde_json::json!(backup_path));
        }

        // Get file info from remote
        if let Ok(stat) = conn.stat(&dest_path).await {
            output = output
                .with_data("dest", serde_json::json!(dest))
                .with_data("src", serde_json::json!(src_name))
                .with_data("size", serde_json::json!(stat.size))
                .with_data(
                    "mode",
                    serde_json::json!(format!("{:o}", stat.mode & 0o7777)),
                )
                .with_data("uid", serde_json::json!(stat.uid))
                .with_data("gid", serde_json::json!(stat.gid));
        }

        Ok(output)
    }

    fn render_template(
        template_content: &str,
        context: &serde_json::Value,
    ) -> ModuleResult<String> {
        // Use the shared global TemplateEngine
        TEMPLATE_ENGINE
            .render_with_json(template_content, context)
            .map_err(|e| ModuleError::TemplateError(format!("Failed to render template: {}", e)))
    }

    fn create_backup(dest: &Path, backup_suffix: &str) -> ModuleResult<Option<String>> {
        if dest.exists() {
            let backup_path = format!("{}{}", dest.display(), backup_suffix);
            fs::copy(dest, &backup_path)?;
            Ok(Some(backup_path))
        } else {
            Ok(None)
        }
    }

    #[cfg(unix)]
    fn set_permissions(path: &Path, mode: Option<u32>) -> ModuleResult<bool> {
        if let Some(mode) = mode {
            let current = fs::metadata(path)?.permissions().mode() & 0o7777;
            if current != mode {
                fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    #[cfg(not(unix))]
    fn set_permissions(_path: &Path, _mode: Option<u32>) -> ModuleResult<bool> {
        // Permission modes are not supported on Windows
        Ok(false)
    }

    /// Execute template rendering locally (when no connection is present)
    #[allow(clippy::too_many_arguments)]
    fn execute_local(
        _params: &ModuleParams,
        context: &ModuleContext,
        rendered: &str,
        src_name: &str,
        dest_path: &Path,
        backup: bool,
        backup_suffix: &str,
        mode: Option<u32>,
    ) -> ModuleResult<ModuleOutput> {
        let src = src_name;
        let dest = dest_path.to_string_lossy();

        // Check if dest needs updating
        let needs_update = if dest_path.exists() {
            let current_content = fs::read_to_string(dest_path)?;
            current_content != rendered
        } else {
            true
        };

        if !needs_update {
            // Check if only permissions need updating
            #[cfg(unix)]
            let perm_changed = if let Some(m) = mode {
                if dest_path.exists() {
                    let current = fs::metadata(dest_path)?.permissions().mode() & 0o7777;
                    current != m
                } else {
                    false
                }
            } else {
                false
            };
            #[cfg(not(unix))]
            let perm_changed = false;

            if perm_changed {
                if context.check_mode {
                    return Ok(ModuleOutput::changed(format!(
                        "Would change permissions on '{}'",
                        dest
                    )));
                }
                Self::set_permissions(dest_path, mode)?;
                return Ok(ModuleOutput::changed(format!(
                    "Changed permissions on '{}'",
                    dest
                )));
            }

            return Ok(ModuleOutput::ok(format!(
                "Template '{}' is already up to date",
                dest
            )));
        }

        // In check mode, return what would happen
        if context.check_mode {
            let diff = if context.diff_mode {
                let before = if dest_path.exists() {
                    fs::read_to_string(dest_path).unwrap_or_default()
                } else {
                    String::new()
                };
                Some(Diff::new(before, rendered.to_string()))
            } else {
                None
            };

            let mut output =
                ModuleOutput::changed(format!("Would render template '{}' to '{}'", src, dest));

            if let Some(d) = diff {
                output = output.with_diff(d);
            }

            return Ok(output);
        }

        // Create backup if requested
        let backup_file = if backup {
            Self::create_backup(dest_path, backup_suffix)?
        } else {
            None
        };

        // Create parent directories if needed
        if let Some(parent) = dest_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        // Write rendered content
        fs::write(dest_path, rendered)?;

        // Set permissions
        let perm_changed = Self::set_permissions(dest_path, mode)?;

        let mut output =
            ModuleOutput::changed(format!("Rendered template '{}' to '{}'", src, dest));

        if let Some(backup_path) = backup_file {
            output = output.with_data("backup_file", serde_json::json!(backup_path));
        }

        if perm_changed {
            output = output.with_data("mode_changed", serde_json::json!(true));
        }

        // Add file info to output
        let meta = fs::metadata(dest_path)?;
        output = output
            .with_data("dest", serde_json::json!(dest))
            .with_data("src", serde_json::json!(src))
            .with_data("size", serde_json::json!(meta.len()));

        // Unix-specific file metadata
        #[cfg(unix)]
        {
            output = output
                .with_data(
                    "mode",
                    serde_json::json!(format!("{:o}", meta.permissions().mode() & 0o7777)),
                )
                .with_data("uid", serde_json::json!(meta.uid()))
                .with_data("gid", serde_json::json!(meta.gid()));
        }

        Ok(output)
    }
}

impl Module for TemplateModule {
    fn name(&self) -> &'static str {
        "template"
    }

    fn description(&self) -> &'static str {
        "Render Jinja2 templates to a destination (using Minijinja)"
    }

    fn classification(&self) -> ModuleClassification {
        ModuleClassification::NativeTransport
    }

    fn required_params(&self) -> &[&'static str] {
        &["dest"] // src or content is required, but we check that in execute
    }

    fn execute(
        &self,
        params: &ModuleParams,
        context: &ModuleContext,
    ) -> ModuleResult<ModuleOutput> {
        let src = params.get_string("src")?;
        let content = params.get_string("content")?;
        let dest = params.get_string_required("dest")?;
        let dest_path = Path::new(&dest);
        let backup = params.get_bool_or("backup", false);
        let backup_suffix = params
            .get_string("backup_suffix")?
            .unwrap_or_else(|| "~".to_string());
        let mode = params.get_u32("mode")?;
        let extra_vars = params.get("vars");

        // Get template content from either src file or content parameter
        let (template_content, src_name) = match (&src, &content) {
            (Some(src_path_str), _) => {
                let src_path = Path::new(src_path_str);
                if !src_path.exists() {
                    return Err(ModuleError::ExecutionFailed(format!(
                        "Template source '{}' does not exist",
                        src_path_str
                    )));
                }
                let content = fs::read_to_string(src_path).map_err(ModuleError::Io)?;
                (content, src_path_str.clone())
            }
            (None, Some(content_str)) => (content_str.clone(), "<inline>".to_string()),
            (None, None) => {
                return Err(ModuleError::MissingParameter(
                    "Either 'src' or 'content' is required".to_string(),
                ));
            }
        };
        let _src_path = Path::new(&src_name);

        // Build context and render
        let ctx = Self::build_context(context, extra_vars);
        let rendered = Self::render_template(&template_content, &ctx)?;

        // Check if we have a connection for remote execution
        if let Some(ref conn) = context.connection {
            // Remote execution via async connection
            let conn = conn.clone();
            let dest_path = dest_path.to_path_buf();
            let dest = dest.clone();
            let src_name = src_name.clone();
            let rendered = rendered.clone();
            let backup_suffix = backup_suffix.clone();
            let check_mode = context.check_mode;
            let diff_mode = context.diff_mode;

            std::thread::scope(|s| {
                s.spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|e| {
                            ModuleError::ExecutionFailed(format!(
                                "Failed to create runtime: {}",
                                e
                            ))
                        })?;

                    rt.block_on(Self::execute_remote(
                        conn,
                        dest_path,
                        dest,
                        src_name,
                        rendered,
                        backup,
                        backup_suffix,
                        mode,
                        check_mode,
                        diff_mode,
                    ))
                })
                .join()
                .map_err(|_| ModuleError::ExecutionFailed("Thread panicked".to_string()))?
            })
        } else {
            // Local execution (no connection)
            Self::execute_local(
                params,
                context,
                &rendered,
                &src_name,
                dest_path,
                backup,
                &backup_suffix,
                mode,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_template_basic() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.txt.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "Hello, {{ name }}!").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), serde_json::json!("World"));

        let context = ModuleContext::default().with_vars(vars);
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_template_with_loops() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.txt.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "{% for item in items %}{{ item }}\n{% endfor %}").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert(
            "items".to_string(),
            serde_json::json!(["one", "two", "three"]),
        );

        let context = ModuleContext::default().with_vars(vars);
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "one\ntwo\nthree\n");
    }

    #[test]
    fn test_template_with_conditionals() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.txt.j2");
        let dest = temp.path().join("output.txt");

        fs::write(
            &src,
            "{% if enabled %}Feature enabled{% else %}Feature disabled{% endif %}",
        )
        .unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("enabled".to_string(), serde_json::json!(true));

        let context = ModuleContext::default().with_vars(vars);
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "Feature enabled");
    }

    #[test]
    fn test_template_idempotent() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.txt.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "Hello, {{ name }}!").unwrap();
        fs::write(&dest, "Hello, World!").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), serde_json::json!("World"));

        let context = ModuleContext::default().with_vars(vars);
        let result = module.execute(&params, &context).unwrap();

        assert!(!result.changed);
    }

    #[test]
    fn test_template_check_mode() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.txt.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "Hello, {{ name }}!").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), serde_json::json!("World"));

        let context = ModuleContext::default()
            .with_vars(vars)
            .with_check_mode(true);
        let result = module.check(&params, &context).unwrap();

        assert!(result.changed);
        assert!(result.msg.contains("Would render"));
        assert!(!dest.exists()); // File should not be created in check mode
    }

    #[test]
    fn test_template_filters() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("template.txt.j2");
        let dest = temp.path().join("output.txt");

        fs::write(&src, "{{ name | upper }}").unwrap();

        let module = TemplateModule;
        let mut params: ModuleParams = HashMap::new();
        params.insert("src".to_string(), serde_json::json!(src.to_str().unwrap()));
        params.insert(
            "dest".to_string(),
            serde_json::json!(dest.to_str().unwrap()),
        );

        let mut vars = HashMap::new();
        vars.insert("name".to_string(), serde_json::json!("hello"));

        let context = ModuleContext::default().with_vars(vars);
        let result = module.execute(&params, &context).unwrap();

        assert!(result.changed);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "HELLO");
    }
}
