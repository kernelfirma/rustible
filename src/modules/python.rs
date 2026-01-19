//! Python module fallback executor
//!
//! This module enables execution of any Ansible Python module, providing
//! backwards compatibility with the entire Ansible module ecosystem.
//!
//! It uses the AnsiballZ-style bundling format that Ansible uses:
//! 1. Find the Ansible module Python file
//! 2. Bundle it with arguments into a base64-encoded wrapper
//! 3. Transfer to remote host via SSH
//! 4. Execute with Python interpreter
//! 5. Parse JSON result

use super::{ModuleError, ModuleOutput, ModuleParams, ModuleResult};
use crate::connection::{CommandResult, Connection, ExecuteOptions};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use walkdir::WalkDir;

/// Result from Ansible module execution (JSON format)
#[derive(Debug, Deserialize, Serialize)]
pub struct AnsibleModuleResult {
    /// Whether the module changed state
    #[serde(default)]
    pub changed: bool,

    /// Human-readable message
    #[serde(default)]
    pub msg: Option<String>,

    /// Whether the module failed
    #[serde(default)]
    pub failed: bool,

    /// Failure message
    #[serde(default)]
    pub failure_msg: Option<String>,

    /// Whether the task was skipped
    #[serde(default)]
    pub skipped: bool,

    /// Additional return values
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>,
}

/// Python module executor for Ansible backwards compatibility
pub struct PythonModuleExecutor {
    /// Paths to search for Ansible modules
    module_paths: Vec<PathBuf>,

    /// Cache of discovered module locations
    module_cache: HashMap<String, PathBuf>,
}

impl Default for PythonModuleExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonModuleExecutor {
    /// Create a new Python module executor with default search paths
    pub fn new() -> Self {
        let mut module_paths = Vec::new();

        // Standard Ansible module locations
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            // User collections
            module_paths.push(home.join(".ansible/collections"));
            // User modules
            module_paths.push(home.join(".ansible/plugins/modules"));
        }

        // System-wide locations
        module_paths.push(PathBuf::from("/usr/share/ansible/plugins/modules"));
        module_paths.push(PathBuf::from(
            "/usr/lib/python3/dist-packages/ansible/modules",
        ));

        // Check ANSIBLE_LIBRARY environment variable
        if let Some(lib_path) = std::env::var_os("ANSIBLE_LIBRARY") {
            for path in std::env::split_paths(&lib_path) {
                module_paths.push(path);
            }
        }

        Self {
            module_paths,
            module_cache: HashMap::new(),
        }
    }

    /// Add a custom module search path
    pub fn add_module_path(&mut self, path: impl Into<PathBuf>) {
        self.module_paths.push(path.into());
    }

    /// Find an Ansible module by name
    ///
    /// Supports both short names (e.g., "apt") and FQCNs (e.g., "ansible.builtin.apt")
    ///
    /// Searches in order:
    /// 1. Module cache
    /// 2. FQCN resolution in collections (if name contains dots)
    /// 3. User collections (~/.ansible/collections)
    /// 4. User modules (~/.ansible/plugins/modules)
    /// 5. System modules (/usr/share/ansible/...)
    pub fn find_module(&mut self, name: &str) -> Option<PathBuf> {
        // Check cache first
        if let Some(path) = self.module_cache.get(name) {
            if path.exists() {
                return Some(path.clone());
            }
        }

        // Handle fully-qualified collection names (e.g., "ansible.builtin.apt")
        if let Some(path) = self.find_fqcn_module(name) {
            self.module_cache.insert(name.to_string(), path.clone());
            return Some(path);
        }

        // Extract simple module name for non-FQCN search
        let module_name = if name.contains('.') {
            name.rsplit('.').next().unwrap_or(name)
        } else {
            name
        };

        // Search all paths for the module
        for base_path in &self.module_paths {
            if !base_path.exists() {
                continue;
            }

            // Try direct module file
            let direct = base_path.join(format!("{}.py", module_name));
            if direct.exists() {
                debug!("Found module {} at {}", name, direct.display());
                self.module_cache.insert(name.to_string(), direct.clone());
                return Some(direct);
            }

            // Try in subdirectories (Ansible organizes by category)
            if let Ok(entries) = std::fs::read_dir(base_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let module_file = path.join(format!("{}.py", module_name));
                        if module_file.exists() {
                            debug!("Found module {} at {}", name, module_file.display());
                            self.module_cache
                                .insert(name.to_string(), module_file.clone());
                            return Some(module_file);
                        }
                    }
                }
            }
        }

        warn!("Module {} not found in any search path", name);
        None
    }

    /// Find a module using Fully Qualified Collection Name
    ///
    /// FQCN format: namespace.collection.module (e.g., "ansible.builtin.apt")
    /// Resolves to: {collection_path}/ansible_collections/{namespace}/{collection}/plugins/modules/{module}.py
    fn find_fqcn_module(&self, name: &str) -> Option<PathBuf> {
        let parts: Vec<&str> = name.split('.').collect();

        // Need at least 3 parts: namespace.collection.module
        if parts.len() < 3 {
            return None;
        }

        let namespace = parts[0];
        let collection = parts[1];
        let module_name = parts[parts.len() - 1];

        // Handle nested module paths (e.g., ansible.builtin.packaging.apt -> packaging/apt.py)
        let module_subpath = if parts.len() > 3 {
            parts[2..parts.len()].join("/") + ".py"
        } else {
            format!("{}.py", module_name)
        };

        debug!(
            "Resolving FQCN {} -> namespace:{}, collection:{}, module:{}",
            name, namespace, collection, module_name
        );

        // Get collection root paths
        let collection_roots = self.get_collection_roots();

        for root in collection_roots {
            // Standard collection path: {root}/ansible_collections/{namespace}/{collection}/plugins/modules/
            let collection_module_dir = root
                .join("ansible_collections")
                .join(namespace)
                .join(collection)
                .join("plugins")
                .join("modules");

            if collection_module_dir.exists() {
                let module_path = collection_module_dir.join(&module_subpath);
                if module_path.exists() {
                    debug!("Found FQCN module {} at {}", name, module_path.display());
                    return Some(module_path);
                }

                // Also try without subdirectory nesting
                let simple_path = collection_module_dir.join(format!("{}.py", module_name));
                if simple_path.exists() {
                    debug!("Found FQCN module {} at {}", name, simple_path.display());
                    return Some(simple_path);
                }
            }
        }

        None
    }

    /// Get all collection root directories
    fn get_collection_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let mut seen = HashSet::new();

        let mut push_root = |path: PathBuf| {
            let normalized = if path
                .file_name()
                .map(|name| name == "ansible_collections")
                .unwrap_or(false)
            {
                path.parent().unwrap_or(&path).to_path_buf()
            } else {
                path
            };

            if seen.insert(normalized.clone()) {
                roots.push(normalized);
            }
        };

        for path in &self.module_paths {
            let normalized = if path
                .file_name()
                .map(|name| name == "ansible_collections")
                .unwrap_or(false)
            {
                path.parent().unwrap_or(path).to_path_buf()
            } else {
                path.clone()
            };

            if normalized.join("ansible_collections").is_dir() {
                push_root(normalized);
            }
        }

        // User collections
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            push_root(home.join(".ansible/collections"));
        }

        // ANSIBLE_COLLECTIONS_PATH environment variable
        if let Some(collections_path) = std::env::var_os("ANSIBLE_COLLECTIONS_PATH") {
            for path in std::env::split_paths(&collections_path) {
                push_root(path);
            }
        }

        // System-wide collections
        push_root(PathBuf::from("/usr/share/ansible/collections"));
        push_root(PathBuf::from("/etc/ansible/collections"));

        roots
    }

    /// Find the local Ansible library path
    fn find_ansible_library(&self) -> Option<PathBuf> {
        // Try to find it via python3
        let output = std::process::Command::new("python3")
            .args(["-c", "import ansible; print(ansible.__path__[0])"])
            .output()
            .ok()?;

        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let path = PathBuf::from(path_str);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Bundle a module with its arguments and dependencies into a Zip file (AnsiballZ style)
    pub fn bundle(&self, module_path: &Path, args: &ModuleParams) -> ModuleResult<String> {
        let ansible_lib = self.find_ansible_library()
            .ok_or_else(|| ModuleError::ExecutionFailed("Could not find Ansible library locally to bundle. Please ensure 'ansible' is installed on the controller machine.".to_string()))?;

        let args_json = serde_json::to_string(args).map_err(|e| {
            ModuleError::ExecutionFailed(format!("Failed to serialize module arguments: {}", e))
        })?;
        let args_b64 = BASE64.encode(args_json.as_bytes());

        // Prepare in-memory zip
        let mut buffer = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o755);

            // 1. Prepare module Injector as __main__.py
            let module_source = std::fs::read_to_string(module_path).map_err(|e| {
                ModuleError::ExecutionFailed(format!(
                    "Failed to read module {}: {}",
                    module_path.display(),
                    e
                ))
            })?;

            // Prepend args injection to module source
            // We use Base64 for args to avoid escaping issues
            let injection_header = format!(
                r"
import os
import json
import base64
import sys

# Inject arguments
APP_ARGS_B64 = '{}'
os.environ['ANSIBLE_MODULE_ARGS'] = base64.b64decode(APP_ARGS_B64).decode('utf-8')

# Current directory (root of zip) is automatically in sys.path[0] when running as zipapp
# but we explicitly ensure it for safety
if sys.path[0] != os.path.dirname(__file__):
    sys.path.insert(0, os.path.dirname(__file__))

",
                args_b64.as_str()
            );

            let final_main = format!("{}\n{}", injection_header, module_source);

            zip.start_file("__main__.py", options)
                .map_err(|e| ModuleError::ExecutionFailed(format!("Zip error: {}", e)))?;
            zip.write_all(final_main.as_bytes())?;

            // 2. Add ansible/module_utils
            // We walk the module_utils directory and add everything
            // This ensures common utils like basic.py are available
            let module_utils_path = ansible_lib.join("module_utils");
            if module_utils_path.exists() {
                // Determine the root for relative paths (the parent of 'ansible' dir)
                // path: /usr/lib/python3/dist-packages/ansible
                // parent: /usr/lib/python3/dist-packages
                let lib_root = ansible_lib.parent().unwrap_or(&ansible_lib);

                for entry in WalkDir::new(&module_utils_path) {
                    let entry = entry.map_err(|e| {
                        ModuleError::ExecutionFailed(format!("Failed to walk module_utils: {}", e))
                    })?;
                    let path = entry.path();

                    if path.is_file() {
                        // We want 'ansible/module_utils/...' in the zip
                        // If path is /usr/lib/.../ansible/module_utils/basic.py
                        // and lib_root is /usr/lib/...
                        // rel_path is ansible/module_utils/basic.py
                        if let Ok(rel_path) = path.strip_prefix(lib_root) {
                            let name = rel_path.to_string_lossy().into_owned();
                            zip.start_file(name, options).map_err(|e| {
                                ModuleError::ExecutionFailed(format!("Zip error: {}", e))
                            })?;
                            let content = std::fs::read(path).map_err(|e| {
                                ModuleError::ExecutionFailed(format!(
                                    "Failed to read {}: {}",
                                    path.display(),
                                    e
                                ))
                            })?;
                            zip.write_all(&content)?;
                        }
                    }
                }

                // Ensure ansible/__init__.py exists
                zip.start_file("ansible/__init__.py", options)
                    .map_err(|e| ModuleError::ExecutionFailed(format!("Zip error: {}", e)))?;
                zip.write_all(b"")?;
            } else {
                return Err(ModuleError::ExecutionFailed(format!(
                    "ansible/module_utils not found at {}",
                    module_utils_path.display()
                )));
            }

            zip.finish().map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to finish zip: {}", e))
            })?;
        }

        // Base64 encode the zip payload
        let zip_b64 = BASE64.encode(&buffer);

        // Create the wrapper script that executes the zip
        let wrapper = format!(
            r"#!/usr/bin/env python
# -*- coding: utf-8 -*-
# Rustible AnsiballZ-compatible runner
import sys
import os
import base64
import tempfile
import runpy

# The Zipapp payload (Ansible module + modules_utils)
PAYLOAD = '{zip_b64}'
ARGS_B64 = '{args_b64}'

# Provide module args for debugging/compatibility (also set inside the zip)
os.environ['ANSIBLE_MODULE_ARGS'] = base64.b64decode(ARGS_B64).decode('utf-8')

def main():
    # Create temp file for the zipapp
    fd, path = tempfile.mkstemp(suffix='.zip', prefix='rustible_')
    os.close(fd)
    
    try:
        # Write payload to disk
        with open(path, 'wb') as f:
            f.write(base64.b64decode(PAYLOAD))
            
        # Execute the zipapp in-process
        # This is equivalent to 'python path/to.zip'
        sys.path.insert(0, path)
        runpy.run_path(path, run_name='__main__')
        
    except Exception as e:
        import json
        print(json.dumps({{'failed': True, 'msg': str(e)}}))
    finally:
        try:
            if os.path.exists(path):
                os.remove(path)
        except:
            pass

if __name__ == '__main__':
    main()
"
        );

        Ok(wrapper)
    }

    /// Execute a Python module on a remote connection
    pub async fn execute(
        &mut self,
        conn: &dyn Connection,
        module_name: &str,
        args: &ModuleParams,
        python_interpreter: &str,
    ) -> ModuleResult<ModuleOutput> {
        // Find the module
        let module_path = self.find_module(module_name)
            .ok_or_else(|| ModuleError::ModuleNotFound(format!(
                "Ansible module '{}' not found. Ensure Ansible is installed or check ANSIBLE_LIBRARY path.",
                module_name
            )))?;

        // Bundle the module
        let wrapper = self.bundle(&module_path, args)?;

        debug!(
            "Executing Python module {} via {} ({} bytes)",
            module_name,
            python_interpreter,
            wrapper.len()
        );

        // Execute on remote host
        // We pipe the script directly to Python for efficiency (like Ansible pipelining)
        let command = format!("{} -c {}", python_interpreter, shell_escape(&wrapper));

        let result = conn
            .execute(&command, Some(ExecuteOptions::new()))
            .await
            .map_err(|e| {
                ModuleError::ExecutionFailed(format!("Failed to execute Python module: {}", e))
            })?;

        // Parse the result
        self.parse_result(&result, module_name)
    }

    /// Parse the JSON result from Python module execution
    fn parse_result(
        &self,
        result: &CommandResult,
        module_name: &str,
    ) -> ModuleResult<ModuleOutput> {
        let stdout = result.stdout.trim();

        // Try to find JSON in the output (skip any non-JSON preamble)
        let json_start = stdout.find('{');
        let json_str = match json_start {
            Some(pos) => &stdout[pos..],
            None => stdout,
        };

        // Parse the JSON result
        let parsed: AnsibleModuleResult = serde_json::from_str(json_str).map_err(|e| {
            // If JSON parsing fails, check if it's a command error
            if result.exit_code != 0 {
                ModuleError::ExecutionFailed(format!(
                    "Module {} failed with exit code {}: {}",
                    module_name,
                    result.exit_code,
                    result.stderr.trim()
                ))
            } else {
                ModuleError::ExecutionFailed(format!(
                    "Failed to parse module {} output as JSON: {}. Output: {}",
                    module_name, e, stdout
                ))
            }
        })?;

        // Convert to ModuleOutput
        if parsed.failed {
            return Err(ModuleError::ExecutionFailed(
                parsed.msg.unwrap_or_else(|| "Module failed".to_string()),
            ));
        }

        let msg = parsed
            .msg
            .unwrap_or_else(|| format!("Module {} executed successfully", module_name));

        let mut output = if parsed.changed {
            ModuleOutput::changed(msg)
        } else {
            ModuleOutput::ok(msg)
        };

        // Add additional data from module result
        for (key, value) in parsed.data {
            // Skip internal keys
            if !matches!(key.as_str(), "changed" | "failed" | "msg" | "skipped") {
                output = output.with_data(key, value);
            }
        }

        Ok(output)
    }
}

/// Escape a string for shell execution
fn shell_escape(s: &str) -> String {
    // Use Python's ability to handle base64 to avoid shell escaping issues
    let b64 = BASE64.encode(s.as_bytes());
    format!(
        "\"import base64,sys;exec(base64.b64decode('{}').decode())\"",
        b64
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_executor_creation() {
        let executor = PythonModuleExecutor::new();
        assert!(!executor.module_paths.is_empty());
    }

    #[test]
    fn test_bundle_generation() {
        let executor = PythonModuleExecutor::new();
        let temp_module = std::env::temp_dir().join("test_module.py");
        std::fs::write(
            &temp_module,
            "def main(): import json; print(json.dumps({'changed': True}))",
        )
        .unwrap();

        let mut args = HashMap::new();
        args.insert("name".to_string(), serde_json::json!("test"));

        // Only run this test if ansible is available
        if executor.find_ansible_library().is_some() {
            let bundle = executor.bundle(&temp_module, &args).unwrap();
            assert!(bundle.contains("PAYLOAD"));
            assert!(bundle.contains("runpy.run_path"));
        }

        std::fs::remove_file(&temp_module).ok();
    }

    #[test]
    fn test_parse_success_result() {
        let executor = PythonModuleExecutor::new();
        let result = CommandResult {
            exit_code: 0,
            stdout: r#"{"changed": true, "msg": "Package installed"}"#.to_string(),
            stderr: String::new(),
            success: true,
        };

        let output = executor.parse_result(&result, "apt").unwrap();
        assert!(output.changed);
        assert!(output.msg.contains("Package installed"));
    }

    #[test]
    fn test_parse_failed_result() {
        let executor = PythonModuleExecutor::new();
        let result = CommandResult {
            exit_code: 0,
            stdout: r#"{"failed": true, "msg": "Permission denied"}"#.to_string(),
            stderr: String::new(),
            success: true,
        };

        let err = executor.parse_result(&result, "apt").unwrap_err();
        assert!(matches!(err, ModuleError::ExecutionFailed(_)));
    }

    #[test]
    fn test_fqcn_parsing() {
        let executor = PythonModuleExecutor::new();

        // Test that FQCN with less than 3 parts returns None
        assert!(executor.find_fqcn_module("apt").is_none());
        assert!(executor.find_fqcn_module("builtin.apt").is_none());

        // Full FQCN format is parsed (module won't exist but path resolution works)
        // This tests the parsing logic, not actual file existence
    }

    #[test]
    fn test_collection_roots() {
        let executor = PythonModuleExecutor::new();
        let roots = executor.get_collection_roots();

        // Should have at least user and system collection paths
        assert!(!roots.is_empty());

        // Should include standard system paths
        let has_system_path = roots.iter().any(|p| {
            p.to_string_lossy()
                .contains("/usr/share/ansible/collections")
                || p.to_string_lossy().contains("/etc/ansible/collections")
        });
        assert!(has_system_path);
    }
}
